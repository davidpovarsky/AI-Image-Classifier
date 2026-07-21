#![forbid(unsafe_code)]

use interprocess::local_socket::{
    Listener, ListenerNonblockingMode, ListenerOptions, Name, Stream, prelude::*,
};
#[cfg(unix)]
use interprocess::{local_socket::GenericFilePath, os::unix::local_socket::ListenerOptionsExt};
#[cfg(windows)]
use interprocess::{
    local_socket::GenericNamespaced,
    os::windows::{local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor},
};
use secure_storage::{RateLimiter, hash_admin_password, verify_admin_password};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use supervisor_ipc::{
    Envelope, Request, Response, ResponseStatus, StructuredError, read_frame, write_frame,
};
use uuid::Uuid;
#[cfg(windows)]
use widestring::U16CString;

const SOCKET_NAME: &str = "local-ai-image-filter.supervisor.v1";
#[cfg(unix)]
const UNIX_SOCKET_PATH: &str = "/var/run/local-ai-image-filter/supervisor.sock";
const MAX_CLOCK_SKEW_SECONDS: i64 = 120;
const AUTHORIZATION_TTL_SECONDS: i64 = 120;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceStatus {
    state: String,
    engine_state: String,
    capture_backend: String,
    policy_name: String,
    policy_revision: u64,
    model_status: String,
    certificate_status: String,
    license_status: String,
    last_policy_update: Option<String>,
    last_application_update_check: Option<String>,
    degraded_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServiceConfiguration {
    engine_executable: PathBuf,
    engine_config: PathBuf,
    capture_mode: String,
}

#[derive(Debug, Clone)]
struct Authorization {
    scope: String,
    peer_process_id: u32,
    expires_at: i64,
}

struct Runtime {
    state_directory: PathBuf,
    status: ServiceStatus,
    engine: Option<Child>,
    password_hash: Option<String>,
    rate_limiter: RateLimiter,
    authorizations: HashMap<String, Authorization>,
    seen_nonces: HashSet<String>,
    pending_uninstall_token: Option<Authorization>,
}

impl Runtime {
    fn load(state_directory: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&state_directory)?;
        let password_path = state_directory.join("administrator-password.phc");
        let password_hash = fs::read_to_string(password_path).ok();
        let rate_limiter = fs::read(state_directory.join("authentication-state.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let state = if password_hash.is_some() {
            "stopped"
        } else {
            "needsOnboarding"
        };
        Ok(Self {
            state_directory,
            status: ServiceStatus {
                state: state.into(),
                engine_state: "stopped".into(),
                capture_backend: "notConfigured".into(),
                policy_name: "last-known-good".into(),
                policy_revision: 0,
                model_status: "notVerified".into(),
                certificate_status: "notVerified".into(),
                license_status: "unactivated".into(),
                last_policy_update: None,
                last_application_update_check: None,
                degraded_reason: None,
            },
            engine: None,
            password_hash,
            rate_limiter,
            authorizations: HashMap::new(),
            seen_nonces: HashSet::new(),
            pending_uninstall_token: None,
        })
    }

    fn refresh_engine_state(&mut self) {
        let exited = self
            .engine
            .as_mut()
            .and_then(|child| child.try_wait().ok().flatten());
        if let Some(status) = exited {
            self.engine = None;
            self.status.state = "degraded".into();
            self.status.engine_state = "exited".into();
            self.status.capture_backend = "disabled".into();
            self.status.degraded_reason = Some(format!("engine exited with {status}"));
        }
    }

    fn status_value(&mut self) -> serde_json::Value {
        self.refresh_engine_state();
        serde_json::to_value(&self.status).expect("service status serialization cannot fail")
    }

    fn handle(
        &mut self,
        request: Request,
        peer_process_id: u32,
        now: i64,
    ) -> Result<serde_json::Value, StructuredError> {
        match request {
            Request::GetStatus
            | Request::GetHealth
            | Request::GetEffectivePolicySummary
            | Request::GetDiagnosticsSummary => Ok(self.status_value()),
            Request::SetAdminPassword { password } => {
                if self.password_hash.is_some() {
                    return Err(error(
                        "alreadyConfigured",
                        "administrator password already exists",
                        false,
                    ));
                }
                validate_password(&password)?;
                let hash = hash_admin_password(password.into()).map_err(|_| {
                    error(
                        "passwordHashFailed",
                        "administrator password could not be hashed",
                        false,
                    )
                })?;
                atomic_write(
                    &self.state_directory.join("administrator-password.phc"),
                    hash.as_bytes(),
                )
                .map_err(|_| {
                    error(
                        "stateWriteFailed",
                        "protected state could not be written",
                        true,
                    )
                })?;
                self.password_hash = Some(hash);
                self.status.state = "needsActivation".into();
                Ok(self.status_value())
            }
            Request::VerifyAdminPassword { password, scope } => {
                self.rate_limiter
                    .check(now)
                    .map_err(|_| error("temporarilyLocked", "too many password failures", true))?;
                let hash = self.password_hash.as_deref().ok_or_else(|| {
                    error(
                        "notOnboarded",
                        "administrator password is not configured",
                        false,
                    )
                })?;
                let valid = verify_admin_password(password.into(), hash).map_err(|_| {
                    error(
                        "passwordVerificationFailed",
                        "password verification failed",
                        false,
                    )
                })?;
                if !valid {
                    self.rate_limiter.record_failure(now);
                    self.persist_rate_limiter()?;
                    return Err(error(
                        "invalidPassword",
                        "administrator password is invalid",
                        true,
                    ));
                }
                self.rate_limiter.record_success();
                self.persist_rate_limiter()?;
                let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
                self.authorizations.insert(
                    token.clone(),
                    Authorization {
                        scope,
                        peer_process_id,
                        expires_at: now + AUTHORIZATION_TTL_SECONDS,
                    },
                );
                Ok(serde_json::Value::String(token))
            }
            Request::StartFiltering => self.start_engine(),
            Request::StopFiltering { authorization } => {
                self.consume_authorization(&authorization, "stop", peer_process_id, now)?;
                self.stop_engine()
            }
            Request::PauseFiltering { authorization, .. } => {
                self.consume_authorization(&authorization, "pause", peer_process_id, now)?;
                self.stop_engine()
            }
            Request::RepairNetworkConfiguration { authorization } => {
                self.consume_authorization(&authorization, "repair", peer_process_id, now)?;
                self.status.capture_backend = "disabled".into();
                self.status.degraded_reason = None;
                Ok(self.status_value())
            }
            Request::RequestUninstallAuthorization { authorization } => {
                self.consume_authorization(&authorization, "uninstall", peer_process_id, now)?;
                self.stop_engine()?;
                let token = format!("uninstall-{}", Uuid::new_v4().simple());
                self.pending_uninstall_token = Some(Authorization {
                    scope: "uninstall-finalize".into(),
                    peer_process_id: 0,
                    expires_at: now + AUTHORIZATION_TTL_SECONDS,
                });
                atomic_write(
                    &self.state_directory.join("uninstall-authorization.token"),
                    token.as_bytes(),
                )
                .map_err(|_| {
                    error(
                        "stateWriteFailed",
                        "uninstall authorization could not be persisted",
                        false,
                    )
                })?;
                self.authorizations.insert(
                    token,
                    self.pending_uninstall_token
                        .clone()
                        .expect("pending uninstall authorization exists"),
                );
                self.status.state = "updating".into();
                Ok(self.status_value())
            }
            Request::PrepareUninstall { authorization } => {
                self.consume_authorization(&authorization, "uninstall-finalize", 0, now)?;
                self.pending_uninstall_token = None;
                fs::remove_file(self.state_directory.join("uninstall-authorization.token"))
                    .or_else(|error| {
                        if error.kind() == io::ErrorKind::NotFound {
                            Ok(())
                        } else {
                            Err(error)
                        }
                    })
                    .map_err(|_| {
                        error(
                            "stateWriteFailed",
                            "uninstall authorization could not be consumed",
                            false,
                        )
                    })?;
                self.status.state = "stopped".into();
                self.status.capture_backend = "disabled".into();
                Ok(self.status_value())
            }
            Request::DeactivateDevice { authorization } => {
                self.consume_authorization(&authorization, "deactivate", peer_process_id, now)?;
                self.status.license_status = "unactivated".into();
                self.status.state = "needsActivation".into();
                Ok(self.status_value())
            }
            Request::CheckPolicyUpdate => Err(error(
                "policyClientUnavailable",
                "no verified TUF repository is configured; the last-known-good policy remains active",
                true,
            )),
            Request::ResumeFiltering => self.start_engine(),
            Request::ActivateLicense { .. }
            | Request::ImportOfflineLicense { .. }
            | Request::ApplyPolicyAssignment { .. }
            | Request::ExportSupportBundle { .. }
            | Request::InstallOrRepairCertificate { .. } => Err(error(
                "operationNotConfigured",
                "this production operation requires installer-provisioned credentials and configuration",
                false,
            )),
        }
    }

    fn persist_rate_limiter(&self) -> Result<(), StructuredError> {
        let bytes = serde_json::to_vec(&self.rate_limiter).map_err(|_| {
            error(
                "stateWriteFailed",
                "authentication state could not be serialized",
                false,
            )
        })?;
        atomic_write(
            &self.state_directory.join("authentication-state.json"),
            &bytes,
        )
        .map_err(|_| {
            error(
                "stateWriteFailed",
                "authentication state could not be persisted",
                true,
            )
        })
    }

    fn consume_authorization(
        &mut self,
        token: &str,
        scope: &str,
        peer_process_id: u32,
        now: i64,
    ) -> Result<(), StructuredError> {
        let authorization = self.authorizations.remove(token).ok_or_else(|| {
            error(
                "authorizationInvalid",
                "authorization is unknown or already used",
                false,
            )
        })?;
        if authorization.scope != scope
            || authorization.peer_process_id != peer_process_id
            || authorization.expires_at < now
        {
            return Err(error(
                "authorizationInvalid",
                "authorization scope, caller, or expiry is invalid",
                false,
            ));
        }
        Ok(())
    }

    fn start_engine(&mut self) -> Result<serde_json::Value, StructuredError> {
        self.refresh_engine_state();
        if self.engine.is_some() {
            return Ok(self.status_value());
        }
        if self.password_hash.is_none() {
            return Err(error(
                "notOnboarded",
                "administrator onboarding is incomplete",
                false,
            ));
        }
        if self.status.license_status == "unactivated" {
            return Err(error(
                "notActivated",
                "a verified license is required before filtering",
                false,
            ));
        }
        let configuration = load_service_configuration(&self.state_directory)?;
        let mut command = Command::new(&configuration.engine_executable);
        command
            .arg("run")
            .arg("--config")
            .arg(&configuration.engine_config)
            .arg("--mode")
            .arg(&configuration.capture_mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| {
            error(
                "engineStartFailed",
                "engine process could not be started",
                true,
            )
        })?;
        thread::sleep(Duration::from_millis(750));
        if child
            .try_wait()
            .map_err(|_| {
                error(
                    "engineHealthFailed",
                    "engine process health could not be read",
                    true,
                )
            })?
            .is_some()
        {
            return Err(error(
                "engineHealthFailed",
                "engine exited during startup",
                true,
            ));
        }
        self.engine = Some(child);
        self.status.state = "running".into();
        self.status.engine_state = "running".into();
        self.status.capture_backend = configuration.capture_mode;
        self.status.degraded_reason = None;
        Ok(self.status_value())
    }

    fn stop_engine(&mut self) -> Result<serde_json::Value, StructuredError> {
        // Capture is declared disabled before process termination so a regular-proxy
        // backend can restore its exact snapshot before this point.
        self.status.capture_backend = "disabled".into();
        if let Some(mut child) = self.engine.take() {
            child
                .kill()
                .and_then(|()| child.wait().map(|_| ()))
                .map_err(|_| {
                    error(
                        "engineStopFailed",
                        "engine process could not be stopped",
                        true,
                    )
                })?;
        }
        self.status.state = "stopped".into();
        self.status.engine_state = "stopped".into();
        Ok(self.status_value())
    }
}

fn validate_password(password: &str) -> Result<(), StructuredError> {
    if password.len() < 12 || password.len() > 4096 {
        Err(error(
            "passwordPolicy",
            "password must contain between 12 and 4096 characters",
            false,
        ))
    } else {
        Ok(())
    }
}

fn load_service_configuration(
    state_directory: &Path,
) -> Result<ServiceConfiguration, StructuredError> {
    let path = state_directory.join("supervisor-config.json");
    let bytes = fs::read(path).map_err(|_| {
        error(
            "serviceNotConfigured",
            "supervisor configuration is unavailable",
            false,
        )
    })?;
    let configuration: ServiceConfiguration = serde_json::from_slice(&bytes).map_err(|_| {
        error(
            "serviceNotConfigured",
            "supervisor configuration is invalid",
            false,
        )
    })?;
    if !configuration.engine_executable.is_absolute()
        || !configuration.engine_config.is_absolute()
        || !matches!(configuration.capture_mode.as_str(), "regular" | "local")
    {
        return Err(error(
            "serviceNotConfigured",
            "supervisor paths or capture mode are invalid",
            false,
        ));
    }
    Ok(configuration)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4().simple()));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

fn error(code: &str, message: &str, retryable: bool) -> StructuredError {
    StructuredError {
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

fn peer_process_id(stream: &Stream) -> u32 {
    stream
        .peer_creds()
        .ok()
        .and_then(|credentials| credentials.pid())
        .unwrap_or(0)
}

fn handle_connection(mut stream: Stream, runtime: &Arc<Mutex<Runtime>>) -> io::Result<()> {
    let peer_process_id = peer_process_id(&stream);
    let envelope: Envelope = read_frame(&mut stream).map_err(io::Error::other)?;
    let now = now_epoch_seconds();
    let response = {
        let mut runtime = runtime
            .lock()
            .map_err(|_| io::Error::other("runtime lock poisoned"))?;
        let validation = if envelope.protocol_version != 1 {
            Err(error(
                "unsupportedProtocol",
                "unsupported IPC protocol version",
                false,
            ))
        } else if (envelope.timestamp - now).abs() > MAX_CLOCK_SKEW_SECONDS {
            Err(error(
                "clockSkew",
                "IPC timestamp is outside the accepted window",
                false,
            ))
        } else if !runtime.seen_nonces.insert(envelope.nonce.clone()) {
            Err(error("replay", "IPC nonce has already been used", false))
        } else {
            runtime.handle(envelope.request, peer_process_id, now)
        };
        if runtime.seen_nonces.len() > 4096 {
            runtime.seen_nonces.clear();
        }
        match validation {
            Ok(payload) => Response {
                protocol_version: 1,
                request_id: envelope.request_id,
                timestamp: now,
                status: ResponseStatus::Ok,
                payload: Some(payload),
                error: None,
            },
            Err(error) => Response {
                protocol_version: 1,
                request_id: envelope.request_id,
                timestamp: now,
                status: ResponseStatus::Error,
                payload: None,
                error: Some(error),
            },
        }
    };
    write_frame(&mut stream, &response).map_err(io::Error::other)
}

#[cfg(windows)]
fn create_listener(name: Name<'static>) -> io::Result<Listener> {
    // SYSTEM and Built-in Administrators receive full access. Authenticated users
    // can connect, but privileged methods still require a caller-bound token.
    let sddl = U16CString::from_str("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)")
        .map_err(io::Error::other)?;
    let descriptor = SecurityDescriptor::deserialize(&sddl)?;
    ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .security_descriptor(descriptor)
        .create_sync()
}

#[cfg(unix)]
fn create_listener(name: Name<'static>) -> io::Result<Listener> {
    ListenerOptions::new()
        .name(name)
        .mode(0o660)
        .nonblocking(ListenerNonblockingMode::Accept)
        .create_sync()
}

#[cfg(windows)]
fn endpoint() -> io::Result<Name<'static>> {
    SOCKET_NAME
        .to_ns_name::<GenericNamespaced>()
        .map_err(io::Error::other)
}

#[cfg(unix)]
fn endpoint() -> io::Result<Name<'static>> {
    let path = Path::new(UNIX_SOCKET_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        use std::os::unix::fs::FileTypeExt;

        if !metadata.file_type().is_socket() {
            return Err(io::Error::other(
                "refusing to replace a non-socket IPC path",
            ));
        }
        fs::remove_file(path)?;
    }
    path.to_fs_name::<GenericFilePath>()
        .map(Name::into_owned)
        .map_err(io::Error::other)
}

fn default_state_directory() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("LocalAIImageFilter")
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/LocalAIImageFilter")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        PathBuf::from("/var/lib/local-ai-image-filter")
    }
}

fn run_server(shutdown: Arc<AtomicBool>) -> io::Result<()> {
    let runtime = Arc::new(Mutex::new(Runtime::load(default_state_directory())?));
    let listener = create_listener(endpoint()?)?;
    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok(stream) => {
                let runtime = Arc::clone(&runtime);
                thread::spawn(move || {
                    let _ = handle_connection(stream, &runtime);
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error),
        }
    }
    if let Ok(mut runtime) = runtime.lock() {
        let _ = runtime.stop_engine();
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() -> io::Result<()> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&shutdown);
    ctrlc::set_handler(move || signal.store(true, Ordering::Release)).map_err(io::Error::other)?;
    run_server(shutdown)
}

#[cfg(windows)]
mod windows_service_host {
    use super::*;
    use std::ffi::OsString;
    use windows_service::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
    };

    const SERVICE_NAME: &str = "LocalAIImageFilterSupervisor";
    define_windows_service!(ffi_service_main, service_main);

    pub fn dispatch() -> windows_service::Result<()> {
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
    }

    fn service_main(_arguments: Vec<OsString>) {
        if let Err(error) = run() {
            eprintln!("supervisor service failed: {error}");
        }
    }

    fn run() -> windows_service::Result<()> {
        let shutdown = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&shutdown);
        let status_handle =
            service_control_handler::register(SERVICE_NAME, move |control| match control {
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                ServiceControl::Stop => {
                    signal.store(true, Ordering::Release);
                    ServiceControlHandlerResult::NoError
                }
                _ => ServiceControlHandlerResult::NotImplemented,
            })?;
        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;
        run_server(shutdown).map_err(windows_service::Error::Winapi)?;
        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })
    }
}

#[cfg(windows)]
fn main() -> windows_service::Result<()> {
    windows_service_host::dispatch()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_policy_rejects_short_values() {
        assert_eq!(
            validate_password("short").unwrap_err().code,
            "passwordPolicy"
        );
    }

    #[test]
    fn authorization_is_single_use_and_caller_bound() {
        let temporary = std::env::temp_dir().join(format!("supervisor-test-{}", Uuid::new_v4()));
        let mut runtime = Runtime::load(temporary.clone()).unwrap();
        runtime.authorizations.insert(
            "token".into(),
            Authorization {
                scope: "stop".into(),
                peer_process_id: 7,
                expires_at: 200,
            },
        );
        assert!(
            runtime
                .consume_authorization("token", "stop", 7, 100)
                .is_ok()
        );
        assert_eq!(
            runtime
                .consume_authorization("token", "stop", 7, 100)
                .unwrap_err()
                .code,
            "authorizationInvalid"
        );
        fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn service_config_rejects_relative_paths() {
        let temporary =
            std::env::temp_dir().join(format!("supervisor-config-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temporary).unwrap();
        fs::write(
            temporary.join("supervisor-config.json"),
            br#"{"engineExecutable":"relative","engineConfig":"relative","captureMode":"local"}"#,
        )
        .unwrap();
        assert_eq!(
            load_service_configuration(&temporary).unwrap_err().code,
            "serviceNotConfigured"
        );
        fs::remove_dir_all(temporary).unwrap();
    }
}
