#![forbid(unsafe_code)]

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
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
use licensing::{
    KeygenClientConfiguration, KeygenHttpClient, LicenseError, LicenseState, RecoveryReplayStore,
    SignedRecoveryToken, VerifiedKeygenLicense, verify_keygen_machine_file,
};
use rand_core::{OsRng, RngCore};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
#[cfg(windows)]
use secure_storage::DpapiMachineStore;
#[cfg(unix)]
use secure_storage::MachineFileStore;
use secure_storage::{RateLimiter, SecureStore, hash_admin_password, verify_admin_password};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
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
use time::OffsetDateTime;
use uuid::Uuid;
#[cfg(windows)]
use widestring::U16CString;

#[cfg(windows)]
const SOCKET_NAME: &str = "local-ai-image-filter.supervisor.v1";
#[cfg(unix)]
const UNIX_SOCKET_PATH: &str = "/var/run/local-ai-image-filter/supervisor.sock";
const MAX_CLOCK_SKEW_SECONDS: i64 = 120;
const AUTHORIZATION_TTL_SECONDS: i64 = 120;
const ENGINE_HEALTH_TIMEOUT: Duration = Duration::from_secs(20);
const ENGINE_STOP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_WATCHDOG_RESTARTS: u8 = 5;

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
    engine_sha256: String,
    capture_mode: String,
    #[serde(default = "default_proxy_host")]
    proxy_host: IpAddr,
    #[serde(default = "default_proxy_port")]
    proxy_port: u16,
    #[serde(default)]
    keygen: Option<KeygenConfiguration>,
    #[serde(default)]
    recovery: Option<RecoveryConfiguration>,
    #[serde(default)]
    policy: Option<PolicyConfiguration>,
}

fn default_proxy_host() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

const fn default_proxy_port() -> u16 {
    8080
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CertificateMetadata {
    sha256_fingerprint: String,
    trust_store: String,
    installed_at: String,
    installer_version: String,
    engine_instance_id: String,
    #[serde(default)]
    platform_identifier: Option<String>,
    #[serde(default)]
    linux_anchor_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NetworkTransaction {
    schema_version: u32,
    state: String,
    proxy_address: SocketAddr,
    snapshot: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeygenConfiguration {
    account_id: String,
    product_id: String,
    account_public_key_base64: String,
    offline_ttl_seconds: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryConfiguration {
    key_id: String,
    public_key_base64: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyConfiguration {
    metadata_directory: PathBuf,
    target_directory: PathBuf,
    metadata_base_url: String,
    target_base_url: String,
    bootstrap_root: PathBuf,
    trusted_keys: PathBuf,
    assignment_key_id: String,
    assignment_public_key_base64: String,
    allowed_origins: Vec<String>,
    targets: Vec<PolicyTargetConfiguration>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyTargetConfiguration {
    target_path: String,
    store_directory: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyAssignmentClaims {
    tenant_id: String,
    device_id: String,
    channel: String,
    target_path: String,
    minimum_revision: u64,
    issued_at: i64,
    expires_at: i64,
    nonce: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignedPolicyAssignment {
    claims: PolicyAssignmentClaims,
    key_id: String,
    algorithm: String,
    signature: String,
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
    secure_store: Box<dyn SecureStore>,
    verified_license: Option<VerifiedKeygenLicense>,
    recovery_replay: RecoveryReplayStore,
    watchdog_failures: u8,
    restart_at: Option<i64>,
    resume_at: Option<i64>,
}

impl Runtime {
    fn load(state_directory: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&state_directory)?;
        let secure_store = create_secure_store(&state_directory)?;
        let password_path = state_directory.join("administrator-password.phc");
        let password_hash = fs::read_to_string(password_path).ok();
        let rate_limiter = fs::read(state_directory.join("authentication-state.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let recovery_replay = fs::read(state_directory.join("recovery-replay.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let verified_license = load_persisted_license(&state_directory, secure_store.as_ref());
        let state = if password_hash.is_none() {
            "needsOnboarding"
        } else if verified_license.is_none() {
            "needsActivation"
        } else {
            "stopped"
        };
        let license_status = verified_license
            .as_ref()
            .map(|license| license_state_name(license.state(OffsetDateTime::now_utc())))
            .unwrap_or("unactivated");
        let mut runtime = Self {
            state_directory,
            status: ServiceStatus {
                state: state.into(),
                engine_state: "stopped".into(),
                capture_backend: "disabled".into(),
                policy_name: "last-known-good".into(),
                policy_revision: 0,
                model_status: "notVerified".into(),
                certificate_status: "notVerified".into(),
                license_status: license_status.into(),
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
            secure_store,
            verified_license,
            recovery_replay,
            watchdog_failures: 0,
            restart_at: None,
            resume_at: None,
        };
        if runtime
            .state_directory
            .join("network-transaction.json")
            .exists()
            && let Err(failure) = runtime.restore_network_configuration()
        {
            runtime.status.state = "error".into();
            runtime.status.degraded_reason = Some(failure.message);
        }
        Ok(runtime)
    }

    fn refresh_engine_state(&mut self) {
        let exited = self
            .engine
            .as_mut()
            .and_then(|child| child.try_wait().ok().flatten());
        if let Some(status) = exited {
            self.engine = None;
            let network_failure = self.restore_network_configuration().err();
            self.status.state = "recovering".into();
            self.status.engine_state = "exited".into();
            self.status.capture_backend = "disabled".into();
            self.status.degraded_reason = Some(match network_failure {
                Some(failure) => format!("engine exited with {status}; {}", failure.message),
                None => format!("engine exited with {status}"),
            });
            self.restart_at = Some(now_epoch_seconds() + 1);
        }
    }

    fn watchdog_tick(&mut self, now: i64) {
        self.refresh_engine_state();
        if self.resume_at.is_some_and(|deadline| deadline <= now) {
            self.resume_at = None;
            self.restart_at = Some(now);
        }
        if self.restart_at.is_none_or(|deadline| deadline > now) {
            return;
        }
        self.restart_at = None;
        match self.start_engine() {
            Ok(_) => {
                self.watchdog_failures = 0;
                self.status.degraded_reason = None;
            }
            Err(failure) => {
                self.watchdog_failures = self.watchdog_failures.saturating_add(1);
                self.status.degraded_reason = Some(format!(
                    "watchdog restart {}/{} failed: {}",
                    self.watchdog_failures, MAX_WATCHDOG_RESTARTS, failure.message
                ));
                if self.watchdog_failures >= MAX_WATCHDOG_RESTARTS {
                    self.status.state = "degraded".into();
                } else {
                    self.status.state = "recovering".into();
                    let delay = 1_i64 << self.watchdog_failures.min(5);
                    self.restart_at = Some(now + delay);
                }
            }
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
                let (recovery_code, recovery_hash) = generate_recovery_code()?;
                atomic_write(
                    &self.state_directory.join("recovery-code.phc"),
                    recovery_hash.as_bytes(),
                )
                .map_err(|_| {
                    error(
                        "stateWriteFailed",
                        "recovery-code hash could not be written",
                        true,
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
                Ok(serde_json::json!({
                    "status": self.status_value(),
                    "recoveryCode": recovery_code
                }))
            }
            Request::ResetAdminPassword {
                new_password,
                recovery_code,
                recovery_token,
            } => {
                validate_password(&new_password)?;
                if recovery_code.is_some() == recovery_token.is_some() {
                    return Err(error(
                        "recoveryCredentialInvalid",
                        "provide exactly one recovery code or signed recovery token",
                        false,
                    ));
                }
                if let Some(code) = recovery_code {
                    let recovery_hash =
                        fs::read_to_string(self.state_directory.join("recovery-code.phc"))
                            .map_err(|_| {
                                error(
                                    "recoveryCredentialInvalid",
                                    "the one-time recovery code is unavailable",
                                    false,
                                )
                            })?;
                    if !verify_admin_password(code.into(), &recovery_hash)
                        .map_err(|_| secure_storage_error())?
                    {
                        return Err(error(
                            "recoveryCredentialInvalid",
                            "the one-time recovery code is invalid",
                            false,
                        ));
                    }
                } else if let Some(bytes) = recovery_token {
                    let token: SignedRecoveryToken =
                        serde_json::from_slice(&bytes).map_err(|_| {
                            error(
                                "recoveryCredentialInvalid",
                                "the signed recovery token is malformed",
                                false,
                            )
                        })?;
                    let configuration = load_service_configuration(&self.state_directory)?;
                    let (recovery_key_id, recovery_public_key) =
                        recovery_configuration(&configuration)?;
                    let (device_id, _, _) = self.device_identity()?;
                    self.recovery_replay
                        .verify_and_consume(
                            &token,
                            &device_id,
                            "password-reset",
                            &recovery_key_id,
                            &recovery_public_key,
                            OffsetDateTime::now_utc(),
                        )
                        .map_err(license_provider_error)?;
                    self.persist_recovery_replay()?;
                }
                let password_hash =
                    hash_admin_password(new_password.into()).map_err(|_| secure_storage_error())?;
                let (new_recovery_code, recovery_hash) = generate_recovery_code()?;
                atomic_write(
                    &self.state_directory.join("recovery-code.phc"),
                    recovery_hash.as_bytes(),
                )
                .and_then(|()| {
                    atomic_write(
                        &self.state_directory.join("administrator-password.phc"),
                        password_hash.as_bytes(),
                    )
                })
                .map_err(|_| {
                    error(
                        "stateWriteFailed",
                        "administrator credentials could not be rotated",
                        true,
                    )
                })?;
                self.password_hash = Some(password_hash);
                Ok(serde_json::json!({
                    "status": self.status_value(),
                    "recoveryCode": new_recovery_code
                }))
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
                self.resume_at = None;
                self.restart_at = None;
                self.stop_engine()
            }
            Request::PauseFiltering {
                authorization,
                seconds,
            } => {
                self.consume_authorization(&authorization, "pause", peer_process_id, now)?;
                if seconds == 0 || seconds > 86_400 {
                    return Err(error(
                        "pauseDurationInvalid",
                        "pause duration must be between 1 second and 24 hours",
                        false,
                    ));
                }
                let status = self.stop_engine()?;
                self.resume_at = Some(now + i64::from(seconds));
                Ok(status)
            }
            Request::RepairNetworkConfiguration { authorization } => {
                self.consume_authorization(&authorization, "repair", peer_process_id, now)?;
                self.restore_network_configuration()?;
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
                self.restore_network_configuration()?;
                self.remove_product_certificate()?;
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
                let current = self
                    .verified_license
                    .as_ref()
                    .ok_or_else(|| error("notActivated", "this device is not activated", false))?;
                let license_key = self
                    .secure_store
                    .get("license-key")
                    .map_err(|_| secure_storage_error())?
                    .ok_or_else(|| {
                        error(
                            "onlineDeactivationUnavailable",
                            "an imported offline license has no online deactivation credential",
                            false,
                        )
                    })?;
                let license_key =
                    String::from_utf8(license_key).map_err(|_| secure_storage_error())?;
                let configuration = load_service_configuration(&self.state_directory)?;
                let client = keygen_client(&configuration)?;
                client
                    .deactivate(&license_key, &current.machine_id)
                    .map_err(license_provider_error)?;
                self.secure_store
                    .delete("license-key")
                    .and_then(|()| self.secure_store.delete("license-file"))
                    .map_err(|_| secure_storage_error())?;
                self.verified_license = None;
                self.status.license_status = "unactivated".into();
                self.status.state = "needsActivation".into();
                Ok(self.status_value())
            }
            Request::CheckPolicyUpdate => self.refresh_policies(),
            Request::ResumeFiltering => {
                self.resume_at = None;
                self.restart_at = None;
                self.start_engine()
            }
            Request::ActivateLicense { license_key } => {
                let configuration = load_service_configuration(&self.state_directory)?;
                let client = keygen_client(&configuration)?;
                let (fingerprint, _, public_key) = self.device_identity()?;
                let verified = client
                    .activate(
                        &license_key,
                        &fingerprint,
                        &public_key,
                        OffsetDateTime::now_utc(),
                    )
                    .map_err(license_provider_error)?;
                self.secure_store
                    .put("license-key", license_key.as_bytes())
                    .and_then(|()| self.secure_store.put("license-file", &verified.certificate))
                    .map_err(|_| secure_storage_error())?;
                self.apply_verified_license(verified);
                Ok(self.status_value())
            }
            Request::ImportOfflineLicense { license } => {
                let configuration = load_service_configuration(&self.state_directory)?;
                let keygen = keygen_configuration(&configuration)?;
                let (fingerprint, public_key_base64, _) = self.device_identity()?;
                let verified = verify_keygen_machine_file(
                    &license,
                    &fingerprint,
                    &public_key_base64,
                    &keygen.product_id,
                    &keygen.account_public_key,
                    OffsetDateTime::now_utc(),
                )
                .map_err(license_provider_error)?;
                self.secure_store
                    .put("license-file", &verified.certificate)
                    .map_err(|_| secure_storage_error())?;
                self.apply_verified_license(verified);
                Ok(self.status_value())
            }
            Request::ExportSupportBundle { authorization } => {
                self.consume_authorization(&authorization, "support", peer_process_id, now)?;
                self.export_support_bundle()
            }
            Request::InstallOrRepairCertificate { authorization } => {
                self.consume_authorization(&authorization, "repair", peer_process_id, now)?;
                self.install_or_repair_certificate()?;
                Ok(self.status_value())
            }
            Request::ApplyPolicyAssignment {
                authorization,
                assignment,
            } => {
                self.consume_authorization(&authorization, "policy", peer_process_id, now)?;
                let configuration = load_service_configuration(&self.state_directory)?;
                let policy = policy_configuration(&configuration)?;
                let verified = verify_policy_assignment(policy, &assignment, now)?;
                let policy_nonce = format!("policy:{}", verified.nonce);
                if self.seen_nonces.contains(&policy_nonce) {
                    return Err(error(
                        "policyAssignmentReplay",
                        "the signed policy assignment was already consumed",
                        false,
                    ));
                }
                let status = self.refresh_policies()?;
                if self.status.policy_revision < verified.minimum_revision {
                    return Err(error(
                        "policyAssignmentUnfulfilled",
                        "the signed assignment requires a policy revision that is not available",
                        true,
                    ));
                }
                self.seen_nonces.insert(policy_nonce);
                Ok(status)
            }
        }
    }

    fn export_support_bundle(&mut self) -> Result<serde_json::Value, StructuredError> {
        let support_directory = self.state_directory.join("support");
        fs::create_dir_all(&support_directory).map_err(|_| {
            error(
                "supportBundleWriteFailed",
                "the protected support directory could not be created",
                true,
            )
        })?;
        let created_at = OffsetDateTime::now_utc();
        let file_name = format!("support-{}.json", created_at.unix_timestamp());
        let path = support_directory.join(file_name);
        let configuration = load_service_configuration(&self.state_directory).ok();
        let certificate = load_certificate_metadata(&self.state_directory).ok();
        let payload = serde_json::json!({
            "schemaVersion": 1,
            "createdAt": created_at,
            "productVersion": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
            "status": self.status,
            "configuration": configuration.map(|value| serde_json::json!({
                "captureMode": value.capture_mode,
                "proxyHost": value.proxy_host,
                "proxyPort": value.proxy_port,
                "keygenConfigured": value.keygen.is_some(),
                "recoveryConfigured": value.recovery.is_some(),
                "policyConfigured": value.policy.is_some()
            })),
            "certificate": certificate.map(|value| serde_json::json!({
                "sha256Fingerprint": value.sha256_fingerprint,
                "trustStore": value.trust_store,
                "installedAt": value.installed_at,
                "installerVersion": value.installer_version,
                "engineInstanceId": value.engine_instance_id
            }))
        });
        let bytes = serde_json::to_vec_pretty(&payload).map_err(|_| {
            error(
                "supportBundleWriteFailed",
                "support diagnostics could not be serialized",
                false,
            )
        })?;
        atomic_write(&path, &bytes).map_err(|_| {
            error(
                "supportBundleWriteFailed",
                "support diagnostics could not be written",
                true,
            )
        })?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        Ok(serde_json::json!({
            "path": path,
            "sha256": sha256,
            "bytes": bytes.len()
        }))
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

    fn persist_recovery_replay(&self) -> Result<(), StructuredError> {
        let bytes = serde_json::to_vec(&self.recovery_replay).map_err(|_| {
            error(
                "stateWriteFailed",
                "recovery replay state could not be serialized",
                false,
            )
        })?;
        atomic_write(&self.state_directory.join("recovery-replay.json"), &bytes).map_err(|_| {
            error(
                "stateWriteFailed",
                "recovery replay state could not be persisted",
                true,
            )
        })
    }

    fn refresh_policies(&mut self) -> Result<serde_json::Value, StructuredError> {
        let configuration = load_service_configuration(&self.state_directory)?;
        let policy = policy_configuration(&configuration)?;
        let (device_id, _, _) = self.device_identity()?;
        let mut maximum_revision = 0_u64;
        for target in &policy.targets {
            let minimum_revision = active_policy_revision(&target.store_directory).unwrap_or(0);
            let report =
                refresh_policy_target(&configuration, policy, target, &device_id, minimum_revision);
            match report {
                Ok(revision) => maximum_revision = maximum_revision.max(revision),
                Err(failure) => {
                    if verify_active_policies(&configuration, policy, &device_id).is_ok() {
                        self.status.degraded_reason = Some(format!(
                            "policy refresh failed; verified last-known-good policies remain active: {}",
                            failure.message
                        ));
                    }
                    return Err(failure);
                }
            }
        }
        self.status.policy_name = policy_scope_name(policy);
        self.status.policy_revision = maximum_revision;
        self.status.last_policy_update = Some(OffsetDateTime::now_utc().to_string());
        self.status.degraded_reason = None;
        Ok(self.status_value())
    }

    fn install_or_repair_certificate(&mut self) -> Result<(), StructuredError> {
        let mut metadata = ensure_ca_material(&self.state_directory)?;
        let certificate_path = self
            .state_directory
            .join("ca")
            .join("mitmproxy-ca-cert.cer");
        let platform = install_platform_certificate(&certificate_path, &metadata)?;
        metadata.platform_identifier = platform.platform_identifier;
        metadata.linux_anchor_path = platform.linux_anchor_path;
        persist_certificate_metadata(&self.state_directory, &metadata)?;
        verify_platform_certificate(&certificate_path, &metadata)?;
        self.status.certificate_status = "trusted".into();
        Ok(())
    }

    fn remove_product_certificate(&mut self) -> Result<(), StructuredError> {
        let metadata = match load_certificate_metadata(&self.state_directory) {
            Ok(value) => value,
            Err(error) if error.code == "certificateMetadataMissing" => {
                self.status.certificate_status = "notInstalled".into();
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let certificate_path = self
            .state_directory
            .join("ca")
            .join("mitmproxy-ca-cert.cer");
        verify_certificate_file(&certificate_path, &metadata.sha256_fingerprint)?;
        remove_platform_certificate(&certificate_path, &metadata)?;
        let ca_directory = self.state_directory.join("ca");
        for name in [
            "mitmproxy-ca.pem",
            "mitmproxy-ca-cert.pem",
            "mitmproxy-ca-cert.cer",
            "certificate-metadata.json",
        ] {
            remove_file_if_present(&ca_directory.join(name)).map_err(|_| {
                error(
                    "certificateRemovalFailed",
                    "product CA material could not be removed",
                    true,
                )
            })?;
        }
        self.status.certificate_status = "notInstalled".into();
        Ok(())
    }

    fn restore_network_configuration(&mut self) -> Result<(), StructuredError> {
        let transaction = self.state_directory.join("network-transaction.json");
        if !transaction.exists() {
            self.status.capture_backend = "disabled".into();
            return Ok(());
        }
        let bytes = fs::read(&transaction).map_err(|_| network_recovery_error())?;
        let saved: NetworkTransaction =
            serde_json::from_slice(&bytes).map_err(|_| network_recovery_error())?;
        if saved.schema_version != 1 {
            return Err(network_recovery_error());
        }
        restore_platform_proxy(&saved.snapshot)?;
        verify_platform_proxy_restored(&saved.snapshot)?;
        remove_file_if_present(&transaction).map_err(|_| network_recovery_error())?;
        self.status.capture_backend = "disabled".into();
        Ok(())
    }

    fn enable_network_capture(
        &mut self,
        configuration: &ServiceConfiguration,
    ) -> Result<(), StructuredError> {
        if configuration.capture_mode == "local" {
            self.status.capture_backend = "mitmLocalCapture".into();
            return Ok(());
        }
        let transaction_path = self.state_directory.join("network-transaction.json");
        if transaction_path.exists() {
            self.restore_network_configuration()?;
        }
        let proxy_address = SocketAddr::new(configuration.proxy_host, configuration.proxy_port);
        let snapshot = snapshot_platform_proxy()?;
        let mut transaction = NetworkTransaction {
            schema_version: 1,
            state: "prepared".into(),
            proxy_address,
            snapshot,
        };
        persist_network_transaction(&transaction_path, &transaction)?;
        if let Err(failure) = enable_platform_proxy(proxy_address) {
            let _ = restore_platform_proxy(&transaction.snapshot);
            let _ = verify_platform_proxy_restored(&transaction.snapshot);
            let _ = remove_file_if_present(&transaction_path);
            return Err(failure);
        }
        transaction.state = "enabled".into();
        persist_network_transaction(&transaction_path, &transaction)?;
        self.status.capture_backend = "regularSystemProxy".into();
        Ok(())
    }

    fn device_identity(&self) -> Result<(String, String, [u8; 32]), StructuredError> {
        let private_key = match self
            .secure_store
            .get("device-private-key")
            .map_err(|_| secure_storage_error())?
        {
            Some(bytes) => bytes.try_into().map_err(|_| secure_storage_error())?,
            None => {
                let bytes = SigningKey::generate(&mut OsRng).to_bytes();
                self.secure_store
                    .put("device-private-key", &bytes)
                    .map_err(|_| secure_storage_error())?;
                bytes
            }
        };
        let signing_key = SigningKey::from_bytes(&private_key);
        let public_key = signing_key.verifying_key().to_bytes();
        let public_key_base64 = STANDARD.encode(public_key);
        let machine_identifier = stable_machine_identifier(self.secure_store.as_ref())?;
        let mut digest = Sha256::new();
        digest.update(b"com.localimagefilter.device.v1\0");
        digest.update(machine_identifier.as_bytes());
        let fingerprint = format!("{:x}", digest.finalize());
        Ok((fingerprint, public_key_base64, public_key))
    }

    fn apply_verified_license(&mut self, verified: VerifiedKeygenLicense) {
        self.status.license_status =
            license_state_name(verified.state(OffsetDateTime::now_utc())).into();
        self.status.state = if self.password_hash.is_some() {
            "stopped".into()
        } else {
            "needsOnboarding".into()
        };
        self.verified_license = Some(verified);
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
        let license_state = self
            .verified_license
            .as_ref()
            .map(|license| license.state(OffsetDateTime::now_utc()))
            .unwrap_or(LicenseState::Unactivated);
        self.status.license_status = license_state_name(license_state).into();
        if !matches!(
            license_state,
            LicenseState::ActiveOnline | LicenseState::ActiveOffline | LicenseState::Grace
        ) {
            return Err(error(
                "notActivated",
                "an active, cryptographically verified license is required before filtering",
                false,
            ));
        }
        let configuration = load_service_configuration(&self.state_directory)?;
        let (device_id, _, _) = self.device_identity()?;
        let policy = policy_configuration(&configuration)?;
        let policy_revision = verify_active_policies(&configuration, policy, &device_id)?;
        self.status.policy_name = policy_scope_name(policy);
        self.status.policy_revision = policy_revision;
        verify_engine_and_models(&configuration, policy, &device_id)?;
        self.status.model_status = "verified".into();
        let certificate = load_certificate_metadata(&self.state_directory)?;
        let certificate_path = self
            .state_directory
            .join("ca")
            .join("mitmproxy-ca-cert.cer");
        verify_platform_certificate(&certificate_path, &certificate)?;
        self.status.certificate_status = "trusted".into();
        let mut command = Command::new(&configuration.engine_executable);
        command
            .arg("run")
            .arg("--config")
            .arg(&configuration.engine_config)
            .arg("--mode")
            .arg(&configuration.capture_mode)
            .arg("--listen-host")
            .arg(configuration.proxy_host.to_string())
            .arg("--listen-port")
            .arg(configuration.proxy_port.to_string())
            .arg("--ca-directory")
            .arg(self.state_directory.join("ca"));
        append_active_policy_arguments(&mut command, policy, &device_id);
        command
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
        wait_for_engine_health(
            &mut child,
            SocketAddr::new(configuration.proxy_host, configuration.proxy_port),
        )?;
        if let Err(failure) = self.enable_network_capture(&configuration) {
            let _ = graceful_stop(&mut child);
            return Err(failure);
        }
        self.engine = Some(child);
        self.status.state = "running".into();
        self.status.engine_state = "running".into();
        self.status.degraded_reason = None;
        self.watchdog_failures = 0;
        Ok(self.status_value())
    }

    fn stop_engine(&mut self) -> Result<serde_json::Value, StructuredError> {
        // Capture is declared disabled before process termination so a regular-proxy
        // backend can restore its exact snapshot before this point.
        self.restore_network_configuration()?;
        if let Some(mut child) = self.engine.take() {
            graceful_stop(&mut child)?;
        }
        self.status.state = "stopped".into();
        self.status.engine_state = "stopped".into();
        Ok(self.status_value())
    }
}

#[derive(Debug)]
struct PlatformCertificateInstallation {
    platform_identifier: Option<String>,
    linux_anchor_path: Option<PathBuf>,
}

fn ensure_ca_material(state_directory: &Path) -> Result<CertificateMetadata, StructuredError> {
    let ca_directory = state_directory.join("ca");
    fs::create_dir_all(&ca_directory).map_err(|_| certificate_generation_error())?;
    let certificate_der_path = ca_directory.join("mitmproxy-ca-cert.cer");
    if let Ok(metadata) = load_certificate_metadata(state_directory) {
        verify_certificate_file(&certificate_der_path, &metadata.sha256_fingerprint)?;
        return Ok(metadata);
    }

    let key = KeyPair::generate().map_err(|_| certificate_generation_error())?;
    let mut params = CertificateParams::default();
    params.not_before = OffsetDateTime::now_utc() - time::Duration::days(1);
    params.not_after = OffsetDateTime::now_utc() + time::Duration::days(3650);
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::OrganizationName, "Local AI Image Filter");
    distinguished_name.push(DnType::CommonName, "Local AI Image Filter Installation CA");
    params.distinguished_name = distinguished_name;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::CrlSign,
    ];
    let certificate = params
        .self_signed(&key)
        .map_err(|_| certificate_generation_error())?;
    let certificate_der = certificate.der().as_ref();
    let certificate_pem = certificate.pem();
    let private_key_pem = key.serialize_pem();
    let combined_pem = format!("{private_key_pem}{certificate_pem}");
    atomic_write(
        &ca_directory.join("mitmproxy-ca.pem"),
        combined_pem.as_bytes(),
    )
    .and_then(|()| {
        atomic_write(
            &ca_directory.join("mitmproxy-ca-cert.pem"),
            certificate_pem.as_bytes(),
        )
    })
    .and_then(|()| atomic_write(&certificate_der_path, certificate_der))
    .map_err(|_| certificate_generation_error())?;
    set_private_file_permissions(&ca_directory.join("mitmproxy-ca.pem"))?;

    let instance_path = state_directory.join("engine-instance-id");
    let engine_instance_id = fs::read_to_string(&instance_path).unwrap_or_else(|_| {
        let value = Uuid::new_v4().to_string();
        let _ = atomic_write(&instance_path, value.as_bytes());
        value
    });
    let metadata = CertificateMetadata {
        sha256_fingerprint: format!("{:x}", Sha256::digest(certificate_der)),
        trust_store: platform_trust_store().into(),
        installed_at: OffsetDateTime::now_utc().to_string(),
        installer_version: env!("CARGO_PKG_VERSION").into(),
        engine_instance_id: engine_instance_id.trim().into(),
        platform_identifier: None,
        linux_anchor_path: None,
    };
    persist_certificate_metadata(state_directory, &metadata)?;
    Ok(metadata)
}

fn certificate_generation_error() -> StructuredError {
    error(
        "certificateGenerationFailed",
        "the unique installation CA could not be generated or persisted",
        true,
    )
}

fn persist_certificate_metadata(
    state_directory: &Path,
    metadata: &CertificateMetadata,
) -> Result<(), StructuredError> {
    let bytes = serde_json::to_vec_pretty(metadata).map_err(|_| certificate_generation_error())?;
    atomic_write(
        &state_directory.join("ca").join("certificate-metadata.json"),
        &bytes,
    )
    .map_err(|_| certificate_generation_error())
}

fn load_certificate_metadata(
    state_directory: &Path,
) -> Result<CertificateMetadata, StructuredError> {
    let bytes =
        fs::read(state_directory.join("ca").join("certificate-metadata.json")).map_err(|_| {
            error(
                "certificateMetadataMissing",
                "the product CA has not been generated and approved",
                false,
            )
        })?;
    serde_json::from_slice(&bytes).map_err(|_| {
        error(
            "certificateMetadataInvalid",
            "the product CA metadata is invalid",
            false,
        )
    })
}

fn verify_certificate_file(path: &Path, expected_sha256: &str) -> Result<(), StructuredError> {
    let certificate = fs::read(path).map_err(|_| {
        error(
            "certificateFileMissing",
            "the product CA certificate file is unavailable",
            false,
        )
    })?;
    let actual = format!("{:x}", Sha256::digest(certificate));
    if actual != expected_sha256 {
        return Err(error(
            "certificateFingerprintMismatch",
            "the product CA certificate does not match the recorded fingerprint",
            false,
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), StructuredError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| certificate_generation_error())
}

#[cfg(windows)]
fn set_private_file_permissions(_path: &Path) -> Result<(), StructuredError> {
    // The installer gives only SYSTEM and Administrators access to ProgramData's
    // product directory. DPAPI remains in use for license and device secrets.
    Ok(())
}

#[cfg(windows)]
fn platform_trust_store() -> &'static str {
    "Windows LocalMachine Root"
}

#[cfg(target_os = "macos")]
fn platform_trust_store() -> &'static str {
    "macOS System.keychain"
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_trust_store() -> &'static str {
    "Linux system CA trust"
}

fn command_output(command: &mut Command, operation: &str) -> Result<String, StructuredError> {
    let output = command.output().map_err(|_| {
        error(
            "platformCommandFailed",
            &format!("{operation} could not be started"),
            true,
        )
    })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(error(
            "platformCommandFailed",
            &format!("{operation} failed: {}", message.trim()),
            true,
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| {
        error(
            "platformCommandFailed",
            &format!("{operation} returned invalid text"),
            false,
        )
    })
}

#[cfg(windows)]
fn powershell_literal(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

#[cfg(windows)]
fn install_platform_certificate(
    certificate_path: &Path,
    metadata: &CertificateMetadata,
) -> Result<PlatformCertificateInstallation, StructuredError> {
    verify_certificate_file(certificate_path, &metadata.sha256_fingerprint)?;
    let script = format!(
        "$ErrorActionPreference='Stop'; $c=Import-Certificate -FilePath '{}' -CertStoreLocation 'Cert:\\LocalMachine\\Root'; $c.Thumbprint",
        powershell_literal(certificate_path)
    );
    let thumbprint = command_output(
        Command::new("powershell.exe").args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ]),
        "Windows CA installation",
    )?
    .trim()
    .to_ascii_uppercase();
    if thumbprint.len() != 40 || !thumbprint.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err(error(
            "certificateInstallationFailed",
            "Windows returned an invalid installed certificate thumbprint",
            false,
        ));
    }
    Ok(PlatformCertificateInstallation {
        platform_identifier: Some(thumbprint),
        linux_anchor_path: None,
    })
}

#[cfg(windows)]
fn verify_platform_certificate(
    certificate_path: &Path,
    metadata: &CertificateMetadata,
) -> Result<(), StructuredError> {
    verify_certificate_file(certificate_path, &metadata.sha256_fingerprint)?;
    let thumbprint = metadata.platform_identifier.as_deref().ok_or_else(|| {
        error(
            "certificateTrustMissing",
            "the Windows certificate thumbprint was not recorded",
            false,
        )
    })?;
    let script = format!(
        "$c=Get-Item 'Cert:\\LocalMachine\\Root\\{thumbprint}' -ErrorAction Stop; $s=[Security.Cryptography.SHA256]::Create(); (($s.ComputeHash($c.RawData)|ForEach-Object {{$_.ToString('x2')}})-join '')"
    );
    let fingerprint = command_output(
        Command::new("powershell.exe").args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ]),
        "Windows CA trust verification",
    )?;
    if fingerprint.trim() != metadata.sha256_fingerprint {
        return Err(error(
            "certificateFingerprintMismatch",
            "the trusted Windows certificate does not match this installation",
            false,
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn remove_platform_certificate(
    certificate_path: &Path,
    metadata: &CertificateMetadata,
) -> Result<(), StructuredError> {
    verify_platform_certificate(certificate_path, metadata)?;
    let thumbprint = metadata.platform_identifier.as_deref().ok_or_else(|| {
        error(
            "certificateTrustMissing",
            "the Windows certificate thumbprint was not recorded",
            false,
        )
    })?;
    let script = format!("Remove-Item 'Cert:\\LocalMachine\\Root\\{thumbprint}' -ErrorAction Stop");
    command_output(
        Command::new("powershell.exe").args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ]),
        "Windows CA removal",
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_platform_certificate(
    certificate_path: &Path,
    metadata: &CertificateMetadata,
) -> Result<PlatformCertificateInstallation, StructuredError> {
    verify_certificate_file(certificate_path, &metadata.sha256_fingerprint)?;
    command_output(
        Command::new("/usr/bin/security").args([
            "add-trusted-cert",
            "-d",
            "-r",
            "trustRoot",
            "-k",
            "/Library/Keychains/System.keychain",
            certificate_path.to_string_lossy().as_ref(),
        ]),
        "macOS CA installation",
    )?;
    Ok(PlatformCertificateInstallation {
        platform_identifier: Some(metadata.sha256_fingerprint.to_ascii_uppercase()),
        linux_anchor_path: None,
    })
}

#[cfg(target_os = "macos")]
fn verify_platform_certificate(
    certificate_path: &Path,
    metadata: &CertificateMetadata,
) -> Result<(), StructuredError> {
    verify_certificate_file(certificate_path, &metadata.sha256_fingerprint)?;
    let output = command_output(
        Command::new("/usr/bin/security").args([
            "find-certificate",
            "-a",
            "-Z",
            "/Library/Keychains/System.keychain",
        ]),
        "macOS CA trust verification",
    )?;
    if !output
        .to_ascii_lowercase()
        .contains(&metadata.sha256_fingerprint)
    {
        return Err(error(
            "certificateTrustMissing",
            "the exact product CA is not trusted by the macOS system keychain",
            false,
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_platform_certificate(
    certificate_path: &Path,
    metadata: &CertificateMetadata,
) -> Result<(), StructuredError> {
    verify_platform_certificate(certificate_path, metadata)?;
    command_output(
        Command::new("/usr/bin/security").args([
            "delete-certificate",
            "-Z",
            &metadata.sha256_fingerprint.to_ascii_uppercase(),
            "/Library/Keychains/System.keychain",
        ]),
        "macOS CA removal",
    )?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_anchor_path(fingerprint: &str) -> Result<PathBuf, StructuredError> {
    let name = format!("local-ai-image-filter-{}.crt", &fingerprint[..16]);
    for root in [
        Path::new("/usr/local/share/ca-certificates"),
        Path::new("/etc/pki/ca-trust/source/anchors"),
    ] {
        if root.is_dir() {
            return Ok(root.join(&name));
        }
    }
    Err(error(
        "certificateTrustStoreUnavailable",
        "neither update-ca-certificates nor update-ca-trust has a system anchor directory",
        false,
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn update_linux_ca_trust(anchor: &Path) -> Result<(), StructuredError> {
    if anchor.starts_with("/usr/local/share/ca-certificates") {
        command_output(
            &mut Command::new("update-ca-certificates"),
            "Linux CA trust update",
        )?;
    } else {
        command_output(
            Command::new("update-ca-trust").arg("extract"),
            "Linux CA trust update",
        )?;
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn install_platform_certificate(
    certificate_path: &Path,
    metadata: &CertificateMetadata,
) -> Result<PlatformCertificateInstallation, StructuredError> {
    verify_certificate_file(certificate_path, &metadata.sha256_fingerprint)?;
    let anchor = linux_anchor_path(&metadata.sha256_fingerprint)?;
    fs::copy(certificate_path, &anchor).map_err(|_| {
        error(
            "certificateInstallationFailed",
            "the Linux system CA anchor could not be written",
            true,
        )
    })?;
    update_linux_ca_trust(&anchor)?;
    Ok(PlatformCertificateInstallation {
        platform_identifier: Some(metadata.sha256_fingerprint.clone()),
        linux_anchor_path: Some(anchor),
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn verify_platform_certificate(
    certificate_path: &Path,
    metadata: &CertificateMetadata,
) -> Result<(), StructuredError> {
    verify_certificate_file(certificate_path, &metadata.sha256_fingerprint)?;
    let anchor = metadata.linux_anchor_path.as_deref().ok_or_else(|| {
        error(
            "certificateTrustMissing",
            "the Linux CA anchor was not recorded",
            false,
        )
    })?;
    verify_certificate_file(anchor, &metadata.sha256_fingerprint)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn remove_platform_certificate(
    certificate_path: &Path,
    metadata: &CertificateMetadata,
) -> Result<(), StructuredError> {
    verify_platform_certificate(certificate_path, metadata)?;
    let anchor = metadata.linux_anchor_path.as_deref().ok_or_else(|| {
        error(
            "certificateTrustMissing",
            "the Linux CA anchor was not recorded",
            false,
        )
    })?;
    remove_file_if_present(anchor).map_err(|_| {
        error(
            "certificateRemovalFailed",
            "the exact Linux CA anchor could not be removed",
            true,
        )
    })?;
    update_linux_ca_trust(anchor)
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn persist_network_transaction(
    path: &Path,
    transaction: &NetworkTransaction,
) -> Result<(), StructuredError> {
    let bytes = serde_json::to_vec_pretty(transaction).map_err(|_| network_recovery_error())?;
    atomic_write(path, &bytes).map_err(|_| network_recovery_error())
}

fn network_recovery_error() -> StructuredError {
    error(
        "networkRecoveryFailed",
        "the exact original proxy configuration could not be restored and verified",
        true,
    )
}

fn sha256_file(path: &Path) -> Result<String, StructuredError> {
    let mut file = fs::File::open(path).map_err(|_| {
        error(
            "engineVerificationFailed",
            "the configured engine executable is unavailable",
            false,
        )
    })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let length = file.read(&mut buffer).map_err(|_| {
            error(
                "engineVerificationFailed",
                "the engine executable could not be hashed",
                true,
            )
        })?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn verify_engine_and_models(
    configuration: &ServiceConfiguration,
    policy: &PolicyConfiguration,
    device_id: &str,
) -> Result<(), StructuredError> {
    let actual_hash = sha256_file(&configuration.engine_executable)?;
    if actual_hash != configuration.engine_sha256.to_ascii_lowercase() {
        return Err(error(
            "engineVerificationFailed",
            "the engine executable does not match the installer-recorded SHA-256",
            false,
        ));
    }
    if !configuration.engine_config.is_file() {
        return Err(error(
            "engineConfigurationMissing",
            "the engine configuration file is unavailable",
            false,
        ));
    }
    let mut command = Command::new(&configuration.engine_executable);
    command
        .arg("doctor")
        .arg("--config")
        .arg(&configuration.engine_config)
        .arg("--load-models");
    append_active_policy_arguments(&mut command, policy, device_id);
    let report = command_output(&mut command, "engine and real-model preflight")?;
    let report: serde_json::Value = serde_json::from_str(&report).map_err(|_| {
        error(
            "modelVerificationFailed",
            "the engine preflight returned an invalid report",
            false,
        )
    })?;
    if report["modelLoad"]["success"] != true
        || report["models"].as_object().is_none_or(|models| {
            models.len() < 6 || models.values().any(|value| value["exists"] != true)
        })
    {
        return Err(error(
            "modelVerificationFailed",
            "real model hashes, artifacts, or ONNX sessions failed verification",
            false,
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn windows_proxy_snapshot_script() -> &'static str {
    r#"$ErrorActionPreference='Stop'
$rows=@()
Get-ChildItem 'Registry::HKEY_USERS' | Where-Object {$_.PSChildName -match '^S-1-5-21-' -and $_.PSChildName -notmatch '_Classes$'} | ForEach-Object {
  $sid=$_.PSChildName
  $path="Registry::HKEY_USERS\$sid\Software\Microsoft\Windows\CurrentVersion\Internet Settings"
  $exists=Test-Path $path
  $key=if($exists){Get-Item $path}else{$null}
  $row=[ordered]@{sid=$sid;keyExists=$exists}
  foreach($name in @('ProxyEnable','ProxyServer','ProxyOverride','AutoConfigURL')) {
    $present=$null -ne $key -and $key.GetValueNames() -contains $name
    $value=if($present){$key.GetValue($name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)}else{$null}
    $row[$name]=[ordered]@{present=$present;value=$value}
  }
  $rows += [pscustomobject]$row
}
[ordered]@{kind='windows';users=@($rows)} | ConvertTo-Json -Compress -Depth 6"#
}

#[cfg(windows)]
fn snapshot_platform_proxy() -> Result<serde_json::Value, StructuredError> {
    let output = command_output(
        Command::new("powershell.exe").args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            windows_proxy_snapshot_script(),
        ]),
        "Windows proxy snapshot",
    )?;
    let snapshot: serde_json::Value =
        serde_json::from_str(&output).map_err(|_| network_recovery_error())?;
    if snapshot["users"].as_array().is_none_or(Vec::is_empty) {
        return Err(error(
            "proxyTargetUnavailable",
            "no interactive Windows user registry hive is loaded",
            true,
        ));
    }
    Ok(snapshot)
}

#[cfg(windows)]
fn enable_platform_proxy(address: SocketAddr) -> Result<(), StructuredError> {
    if !address.ip().is_loopback() {
        return Err(network_recovery_error());
    }
    let script = r#"$ErrorActionPreference='Stop'
Get-ChildItem 'Registry::HKEY_USERS' | Where-Object {$_.PSChildName -match '^S-1-5-21-' -and $_.PSChildName -notmatch '_Classes$'} | ForEach-Object {
  $path="Registry::HKEY_USERS\$($_.PSChildName)\Software\Microsoft\Windows\CurrentVersion\Internet Settings"
  New-Item $path -Force | Out-Null
  New-ItemProperty $path -Name ProxyServer -PropertyType String -Value '__PROXY__' -Force | Out-Null
  New-ItemProperty $path -Name ProxyEnable -PropertyType DWord -Value 1 -Force | Out-Null
}"#
        .replace("__PROXY__", &address.to_string());
    command_output(
        Command::new("powershell.exe").args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ]),
        "Windows proxy enable",
    )?;
    Ok(())
}

#[cfg(windows)]
fn restore_platform_proxy(snapshot: &serde_json::Value) -> Result<(), StructuredError> {
    if snapshot["kind"] != "windows" {
        return Err(network_recovery_error());
    }
    let encoded =
        STANDARD.encode(serde_json::to_vec(snapshot).map_err(|_| network_recovery_error())?);
    let script = r#"$ErrorActionPreference='Stop'
$json=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__SNAPSHOT__')) | ConvertFrom-Json
foreach($u in $json.users) {
  $path="Registry::HKEY_USERS\$($u.sid)\Software\Microsoft\Windows\CurrentVersion\Internet Settings"
  if(-not $u.keyExists) { if(Test-Path $path){Remove-Item $path -Recurse -Force}; continue }
  New-Item $path -Force | Out-Null
  foreach($name in @('ProxyEnable','ProxyServer','ProxyOverride','AutoConfigURL')) {
    $saved=$u.$name
    if($saved.present) {
      $type=if($name -eq 'ProxyEnable'){'DWord'}else{'String'}
      New-ItemProperty $path -Name $name -PropertyType $type -Value $saved.value -Force | Out-Null
    } else { Remove-ItemProperty $path -Name $name -ErrorAction SilentlyContinue }
  }
}"#
        .replace("__SNAPSHOT__", &encoded);
    command_output(
        Command::new("powershell.exe").args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ]),
        "Windows proxy restoration",
    )?;
    Ok(())
}

#[cfg(windows)]
fn verify_platform_proxy_restored(snapshot: &serde_json::Value) -> Result<(), StructuredError> {
    if &snapshot_platform_proxy()? == snapshot {
        Ok(())
    } else {
        Err(network_recovery_error())
    }
}

#[cfg(target_os = "macos")]
fn mac_proxy_state(service: &str, secure: bool) -> Result<serde_json::Value, StructuredError> {
    let option = if secure {
        "-getsecurewebproxy"
    } else {
        "-getwebproxy"
    };
    let output = command_output(
        Command::new("/usr/sbin/networksetup").args([option, service]),
        "macOS proxy snapshot",
    )?;
    let mut enabled = false;
    let mut server = String::new();
    let mut port = 0_u16;
    for line in output.lines() {
        if let Some(value) = line.strip_prefix("Enabled: ") {
            enabled = value == "Yes";
        } else if let Some(value) = line.strip_prefix("Server: ") {
            server = value.into();
        } else if let Some(value) = line.strip_prefix("Port: ") {
            port = value.parse().unwrap_or(0);
        }
    }
    Ok(serde_json::json!({"enabled": enabled, "server": server, "port": port}))
}

#[cfg(target_os = "macos")]
fn snapshot_platform_proxy() -> Result<serde_json::Value, StructuredError> {
    let services = command_output(
        Command::new("/usr/sbin/networksetup").arg("-listallnetworkservices"),
        "macOS network service discovery",
    )?;
    let mut rows = Vec::new();
    for line in services.lines().skip(1) {
        let service = line.trim_start_matches('*').trim();
        if service.is_empty() {
            continue;
        }
        let bypass_output = command_output(
            Command::new("/usr/sbin/networksetup").args(["-getproxybypassdomains", service]),
            "macOS proxy bypass snapshot",
        )?;
        let bypass: Vec<&str> = if bypass_output.starts_with("There aren't any") {
            Vec::new()
        } else {
            bypass_output
                .lines()
                .filter(|value| !value.is_empty())
                .collect()
        };
        rows.push(serde_json::json!({
            "service": service,
            "web": mac_proxy_state(service, false)?,
            "secureWeb": mac_proxy_state(service, true)?,
            "bypass": bypass
        }));
    }
    if rows.is_empty() {
        return Err(error(
            "proxyTargetUnavailable",
            "macOS has no configurable network service",
            true,
        ));
    }
    Ok(serde_json::json!({"kind": "macos", "services": rows}))
}

#[cfg(target_os = "macos")]
fn enable_platform_proxy(address: SocketAddr) -> Result<(), StructuredError> {
    let snapshot = snapshot_platform_proxy()?;
    let host = address.ip().to_string();
    let port = address.port().to_string();
    for row in snapshot["services"]
        .as_array()
        .ok_or_else(network_recovery_error)?
    {
        let service = row["service"].as_str().ok_or_else(network_recovery_error)?;
        for option in ["-setwebproxy", "-setsecurewebproxy"] {
            command_output(
                Command::new("/usr/sbin/networksetup").args([option, service, &host, &port]),
                "macOS proxy enable",
            )?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn restore_platform_proxy(snapshot: &serde_json::Value) -> Result<(), StructuredError> {
    if snapshot["kind"] != "macos" {
        return Err(network_recovery_error());
    }
    for row in snapshot["services"]
        .as_array()
        .ok_or_else(network_recovery_error)?
    {
        let service = row["service"].as_str().ok_or_else(network_recovery_error)?;
        for (name, state_option, value_option) in [
            ("web", "-setwebproxystate", "-setwebproxy"),
            ("secureWeb", "-setsecurewebproxystate", "-setsecurewebproxy"),
        ] {
            let saved = &row[name];
            let server = saved["server"].as_str().unwrap_or("");
            let port = saved["port"].as_u64().unwrap_or(0).to_string();
            if !server.is_empty() && port != "0" {
                command_output(
                    Command::new("/usr/sbin/networksetup").args([
                        value_option,
                        service,
                        server,
                        &port,
                    ]),
                    "macOS proxy restoration",
                )?;
            }
            let state = if saved["enabled"].as_bool().unwrap_or(false) {
                "on"
            } else {
                "off"
            };
            command_output(
                Command::new("/usr/sbin/networksetup").args([state_option, service, state]),
                "macOS proxy restoration",
            )?;
        }
        let bypass = row["bypass"]
            .as_array()
            .ok_or_else(network_recovery_error)?;
        let mut command = Command::new("/usr/sbin/networksetup");
        command.args(["-setproxybypassdomains", service]);
        if bypass.is_empty() {
            command.arg("Empty");
        } else {
            for value in bypass {
                command.arg(value.as_str().ok_or_else(network_recovery_error)?);
            }
        }
        command_output(&mut command, "macOS proxy bypass restoration")?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_platform_proxy_restored(snapshot: &serde_json::Value) -> Result<(), StructuredError> {
    if &snapshot_platform_proxy()? == snapshot {
        Ok(())
    } else {
        Err(network_recovery_error())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_proxy_users() -> Result<Vec<(String, String, String)>, StructuredError> {
    let mut users = Vec::new();
    let entries = fs::read_dir("/run/user").map_err(|_| {
        error(
            "proxyTargetUnavailable",
            "Linux has no active graphical user runtime directory",
            true,
        )
    })?;
    for entry in entries.flatten() {
        let uid = entry.file_name().to_string_lossy().into_owned();
        if uid == "0"
            || !uid.bytes().all(|value| value.is_ascii_digit())
            || !entry.path().join("bus").exists()
        {
            continue;
        }
        let user = command_output(
            Command::new("id").args(["-nu", &uid]),
            "Linux graphical user discovery",
        )?
        .trim()
        .to_owned();
        if !user.is_empty()
            && user
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-'))
        {
            users.push((user, uid.clone(), format!("unix:path=/run/user/{uid}/bus")));
        }
    }
    if users.is_empty() {
        return Err(error(
            "proxyTargetUnavailable",
            "Linux regular proxy mode requires an active GNOME session; local capture remains available",
            true,
        ));
    }
    Ok(users)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_gsettings(
    user: &(String, String, String),
    arguments: &[&str],
) -> Result<String, StructuredError> {
    let runtime = format!("XDG_RUNTIME_DIR=/run/user/{}", user.1);
    let bus = format!("DBUS_SESSION_BUS_ADDRESS={}", user.2);
    let mut command = Command::new("runuser");
    command.args(["-u", &user.0, "--", "env", &runtime, &bus, "gsettings"]);
    command.args(arguments);
    command_output(&mut command, "GNOME proxy operation")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_proxy_keys() -> [(&'static str, &'static str); 7] {
    [
        ("org.gnome.system.proxy", "mode"),
        ("org.gnome.system.proxy", "use-same-proxy"),
        ("org.gnome.system.proxy", "ignore-hosts"),
        ("org.gnome.system.proxy.http", "host"),
        ("org.gnome.system.proxy.http", "port"),
        ("org.gnome.system.proxy.https", "host"),
        ("org.gnome.system.proxy.https", "port"),
    ]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn snapshot_platform_proxy() -> Result<serde_json::Value, StructuredError> {
    let mut rows = Vec::new();
    for user in linux_proxy_users()? {
        let mut values = serde_json::Map::new();
        for (schema, key) in linux_proxy_keys() {
            values.insert(
                format!("{schema}|{key}"),
                serde_json::Value::String(
                    linux_gsettings(&user, &["get", schema, key])?.trim().into(),
                ),
            );
        }
        rows.push(
            serde_json::json!({"user": user.0, "uid": user.1, "bus": user.2, "values": values}),
        );
    }
    Ok(serde_json::json!({"kind": "linuxGnome", "users": rows}))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_user_from_row(
    row: &serde_json::Value,
) -> Result<(String, String, String), StructuredError> {
    Ok((
        row["user"]
            .as_str()
            .ok_or_else(network_recovery_error)?
            .into(),
        row["uid"]
            .as_str()
            .ok_or_else(network_recovery_error)?
            .into(),
        row["bus"]
            .as_str()
            .ok_or_else(network_recovery_error)?
            .into(),
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn enable_platform_proxy(address: SocketAddr) -> Result<(), StructuredError> {
    for user in linux_proxy_users()? {
        let host = format!("'{}'", address.ip());
        let port = address.port().to_string();
        for (schema, key, value) in [
            ("org.gnome.system.proxy.http", "host", host.clone()),
            ("org.gnome.system.proxy.http", "port", port.clone()),
            ("org.gnome.system.proxy.https", "host", host.clone()),
            ("org.gnome.system.proxy.https", "port", port.clone()),
            ("org.gnome.system.proxy", "use-same-proxy", "true".into()),
            ("org.gnome.system.proxy", "mode", "'manual'".into()),
        ] {
            linux_gsettings(&user, &["set", schema, key, &value])?;
        }
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn restore_platform_proxy(snapshot: &serde_json::Value) -> Result<(), StructuredError> {
    if snapshot["kind"] != "linuxGnome" {
        return Err(network_recovery_error());
    }
    for row in snapshot["users"]
        .as_array()
        .ok_or_else(network_recovery_error)?
    {
        let user = linux_user_from_row(row)?;
        for (compound, value) in row["values"]
            .as_object()
            .ok_or_else(network_recovery_error)?
        {
            let (schema, key) = compound
                .split_once('|')
                .ok_or_else(network_recovery_error)?;
            linux_gsettings(
                &user,
                &[
                    "set",
                    schema,
                    key,
                    value.as_str().ok_or_else(network_recovery_error)?,
                ],
            )?;
        }
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn verify_platform_proxy_restored(snapshot: &serde_json::Value) -> Result<(), StructuredError> {
    if &snapshot_platform_proxy()? == snapshot {
        Ok(())
    } else {
        Err(network_recovery_error())
    }
}

fn wait_for_engine_health(child: &mut Child, address: SocketAddr) -> Result<(), StructuredError> {
    let deadline = std::time::Instant::now() + ENGINE_HEALTH_TIMEOUT;
    loop {
        if child
            .try_wait()
            .map_err(|_| engine_health_error())?
            .is_some()
        {
            return Err(engine_health_error());
        }
        if probe_http_proxy(address).is_ok() && probe_https_proxy(address).is_ok() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(engine_health_error());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn probe_http_proxy(address: SocketAddr) -> io::Result<()> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(
        b"GET http://local-filter.invalid/.well-known/local-image-filter/health HTTP/1.1\r\nHost: local-filter.invalid\r\nConnection: close\r\n\r\n",
    )?;
    let mut response = [0_u8; 256];
    let length = stream.read(&mut response)?;
    let response = std::str::from_utf8(&response[..length]).map_err(io::Error::other)?;
    if response.starts_with("HTTP/1.1 204") || response.starts_with("HTTP/1.0 204") {
        Ok(())
    } else {
        Err(io::Error::other("proxy health endpoint did not return 204"))
    }
}

fn probe_https_proxy(address: SocketAddr) -> Result<(), StructuredError> {
    let proxy = format!("http://{address}");
    command_output(
        Command::new(if cfg!(windows) { "curl.exe" } else { "curl" }).args([
            "--silent",
            "--show-error",
            "--fail",
            "--max-time",
            "5",
            "--proxy",
            &proxy,
            "https://local-filter.invalid/.well-known/local-image-filter/health",
        ]),
        "controlled HTTPS proxy health check",
    )?;
    Ok(())
}

fn engine_health_error() -> StructuredError {
    error(
        "engineHealthFailed",
        "the engine did not pass controlled HTTP and HTTPS health checks",
        true,
    )
}

fn graceful_stop(child: &mut Child) -> Result<(), StructuredError> {
    #[cfg(windows)]
    let _ = Command::new("taskkill.exe")
        .args(["/PID", &child.id().to_string(), "/T"])
        .output();
    #[cfg(unix)]
    let _ = Command::new("/bin/kill")
        .args(["-TERM", &child.id().to_string()])
        .output();

    let deadline = std::time::Instant::now() + ENGINE_STOP_TIMEOUT;
    loop {
        if child.try_wait().map_err(|_| engine_stop_error())?.is_some() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return child
                .kill()
                .and_then(|()| child.wait().map(|_| ()))
                .map_err(|_| engine_stop_error());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn engine_stop_error() -> StructuredError {
    error(
        "engineStopFailed",
        "the engine process could not be stopped after the bounded shutdown timeout",
        true,
    )
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

fn generate_recovery_code() -> Result<(String, String), StructuredError> {
    let mut entropy = [0_u8; 24];
    OsRng.fill_bytes(&mut entropy);
    let encoded = URL_SAFE_NO_PAD.encode(entropy);
    let recovery_code = encoded
        .as_bytes()
        .chunks(8)
        .map(|chunk| std::str::from_utf8(chunk).expect("base64url is valid UTF-8"))
        .collect::<Vec<_>>()
        .join("-");
    let hash = hash_admin_password(recovery_code.clone().into()).map_err(|_| {
        error(
            "passwordHashFailed",
            "the recovery code could not be hashed",
            false,
        )
    })?;
    Ok((recovery_code, hash))
}

fn load_service_configuration(
    state_directory: &Path,
) -> Result<ServiceConfiguration, StructuredError> {
    let path = state_directory.join("supervisor-config.json");
    let bytes = fs::read(path).map_err(|_| {
        error(
            "serviceConfigurationMissing",
            "supervisor configuration is unavailable",
            false,
        )
    })?;
    let configuration: ServiceConfiguration = serde_json::from_slice(&bytes).map_err(|_| {
        error(
            "serviceConfigurationInvalid",
            "supervisor configuration is invalid",
            false,
        )
    })?;
    if !configuration.engine_executable.is_absolute()
        || !configuration.engine_config.is_absolute()
        || configuration.engine_sha256.len() != 64
        || !configuration
            .engine_sha256
            .bytes()
            .all(|value| value.is_ascii_hexdigit())
        || !matches!(configuration.capture_mode.as_str(), "regular" | "local")
        || !configuration.proxy_host.is_loopback()
        || configuration.proxy_port == 0
    {
        return Err(error(
            "serviceConfigurationInvalid",
            "supervisor paths, engine SHA-256, capture mode, or loopback proxy endpoint are invalid",
            false,
        ));
    }
    Ok(configuration)
}

fn keygen_configuration(
    configuration: &ServiceConfiguration,
) -> Result<KeygenClientConfiguration, StructuredError> {
    let configured = configuration.keygen.as_ref().ok_or_else(|| {
        error(
            "licenseProviderCredentialRequired",
            "Keygen accountId, productId, accountPublicKeyBase64, and offlineTtlSeconds are required in supervisor-config.json",
            false,
        )
    })?;
    let public_key = STANDARD
        .decode(&configured.account_public_key_base64)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            error(
                "licenseProviderConfigurationInvalid",
                "Keygen accountPublicKeyBase64 must decode to exactly 32 bytes",
                false,
            )
        })?;
    Ok(KeygenClientConfiguration {
        account_id: configured.account_id.clone(),
        product_id: configured.product_id.clone(),
        account_public_key: public_key,
        offline_ttl_seconds: configured.offline_ttl_seconds,
    })
}

fn recovery_configuration(
    configuration: &ServiceConfiguration,
) -> Result<(String, [u8; 32]), StructuredError> {
    let configured = configuration.recovery.as_ref().ok_or_else(|| {
        error(
            "recoverySigningCredentialRequired",
            "recovery keyId and publicKeyBase64 are required in supervisor-config.json",
            false,
        )
    })?;
    let public_key = STANDARD
        .decode(&configured.public_key_base64)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            error(
                "recoverySigningConfigurationInvalid",
                "recovery publicKeyBase64 must decode to exactly 32 bytes",
                false,
            )
        })?;
    if configured.key_id.is_empty() || configured.key_id.len() > 128 {
        return Err(error(
            "recoverySigningConfigurationInvalid",
            "recovery keyId is invalid",
            false,
        ));
    }
    Ok((configured.key_id.clone(), public_key))
}

fn policy_configuration(
    configuration: &ServiceConfiguration,
) -> Result<&PolicyConfiguration, StructuredError> {
    let policy = configuration.policy.as_ref().ok_or_else(|| {
        error(
            "policyTrustConfigurationRequired",
            "TUF URLs, bootstrap root, bundle verification keys, allowed origins, and targets are required in supervisor-config.json",
            false,
        )
    })?;
    let paths = [
        &policy.metadata_directory,
        &policy.target_directory,
        &policy.bootstrap_root,
        &policy.trusted_keys,
    ];
    if paths.iter().any(|path| !path.is_absolute())
        || policy.assignment_key_id.is_empty()
        || policy.assignment_key_id.len() > 128
        || STANDARD
            .decode(&policy.assignment_public_key_base64)
            .ok()
            .is_none_or(|bytes| bytes.len() != 32)
        || policy.targets.is_empty()
        || policy
            .targets
            .iter()
            .any(|target| !target.store_directory.is_absolute())
        || policy.allowed_origins.is_empty()
        || !valid_policy_url(&policy.metadata_base_url, &policy.allowed_origins)
        || !valid_policy_url(&policy.target_base_url, &policy.allowed_origins)
    {
        return Err(policy_configuration_error());
    }
    let mut previous_rank = None;
    let mut seen = HashSet::new();
    for target in &policy.targets {
        let rank =
            policy_target_rank(&target.target_path).ok_or_else(policy_configuration_error)?;
        if previous_rank.is_some_and(|previous| rank <= previous)
            || !seen.insert(target.target_path.clone())
        {
            return Err(policy_configuration_error());
        }
        previous_rank = Some(rank);
    }
    Ok(policy)
}

fn policy_configuration_error() -> StructuredError {
    error(
        "policyTrustConfigurationInvalid",
        "policy configuration must use allowlisted HTTPS origins, absolute paths, and ordered unique vendor, tenant, device targets",
        false,
    )
}

fn valid_policy_url(url: &str, allowed_origins: &[String]) -> bool {
    if !url.starts_with("https://") || url.contains('@') || url.contains('?') || url.contains('#') {
        return false;
    }
    allowed_origins.iter().any(|origin| {
        origin.starts_with("https://")
            && !origin.contains('@')
            && (url == origin || url.starts_with(&format!("{}/", origin.trim_end_matches('/'))))
    })
}

fn policy_target_rank(path: &str) -> Option<u8> {
    let components: Vec<&str> = path.split('/').collect();
    if components.len() != 4
        || components[0] != "policies"
        || components[3] != "policy-bundle.json"
        || components[2].is_empty()
        || components[2].len() > 128
        || !components[2]
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'.' | b'_' | b'-'))
    {
        return None;
    }
    match components[1] {
        "channels" if matches!(components[2], "stable" | "beta") => Some(0),
        "tenants" => Some(1),
        "devices" => Some(2),
        _ => None,
    }
}

fn policy_scope_name(policy: &PolicyConfiguration) -> String {
    policy
        .targets
        .iter()
        .filter_map(|target| match policy_target_rank(&target.target_path) {
            Some(0) => Some("vendor"),
            Some(1) => Some("tenant"),
            Some(2) => Some("device"),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn policy_target_identifier(policy: &PolicyConfiguration, rank: u8) -> Option<&str> {
    policy.targets.iter().find_map(|target| {
        (policy_target_rank(&target.target_path) == Some(rank))
            .then(|| target.target_path.split('/').nth(2))
            .flatten()
    })
}

fn verify_policy_assignment(
    policy: &PolicyConfiguration,
    encoded: &str,
    now: i64,
) -> Result<PolicyAssignmentClaims, StructuredError> {
    let assignment: SignedPolicyAssignment = serde_json::from_str(encoded).map_err(|_| {
        error(
            "policyAssignmentInvalid",
            "the signed policy assignment is malformed",
            false,
        )
    })?;
    let claims = &assignment.claims;
    let current_revision = policy
        .targets
        .iter()
        .filter_map(|target| active_policy_revision(&target.store_directory))
        .max()
        .unwrap_or(0);
    let expected_device_target = policy
        .targets
        .iter()
        .find(|target| policy_target_rank(&target.target_path) == Some(2))
        .map(|target| target.target_path.as_str());
    if assignment.algorithm != "Ed25519"
        || assignment.key_id != policy.assignment_key_id
        || Some(claims.tenant_id.as_str()) != policy_target_identifier(policy, 1)
        || Some(claims.device_id.as_str()) != policy_target_identifier(policy, 2)
        || Some(claims.channel.as_str()) != policy_target_identifier(policy, 0)
        || Some(claims.target_path.as_str()) != expected_device_target
        || claims.minimum_revision <= current_revision
        || claims.nonce.len() < 32
        || claims.issued_at > now
        || claims.expires_at < now
        || claims.expires_at.saturating_sub(claims.issued_at) > 900
    {
        return Err(error(
            "policyAssignmentInvalid",
            "the assignment does not match this tenant, device, channel, target, revision, or validity window",
            false,
        ));
    }
    let public_key: [u8; 32] = STANDARD
        .decode(&policy.assignment_public_key_base64)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(policy_configuration_error)?;
    let signature = STANDARD
        .decode(&assignment.signature)
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or_else(|| {
            error(
                "policyAssignmentInvalid",
                "the assignment signature encoding is invalid",
                false,
            )
        })?;
    let canonical = serde_json_canonicalizer::to_vec(claims).map_err(|_| {
        error(
            "policyAssignmentInvalid",
            "the assignment claims could not be canonicalized",
            false,
        )
    })?;
    VerifyingKey::from_bytes(&public_key)
        .and_then(|key| key.verify(&canonical, &signature))
        .map_err(|_| {
            error(
                "policyAssignmentInvalid",
                "the assignment signature is invalid",
                false,
            )
        })?;
    Ok(assignment.claims)
}

fn active_policy_path(target: &PolicyTargetConfiguration) -> PathBuf {
    target.store_directory.join("active-policy.json")
}

fn active_policy_revision(store_directory: &Path) -> Option<u64> {
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(store_directory.join("active-policy.json")).ok()?).ok()?;
    value["revision"].as_u64()
}

fn append_active_policy_arguments(
    command: &mut Command,
    policy: &PolicyConfiguration,
    device_id: &str,
) {
    for target in &policy.targets {
        command
            .arg("--active-policy-bundle")
            .arg(active_policy_path(target));
    }
    command
        .arg("--active-policy-trusted-keys")
        .arg(&policy.trusted_keys);
    if let Some(tenant_id) = policy_tenant_id(policy) {
        command.arg("--active-policy-tenant-id").arg(tenant_id);
    }
    command.arg("--active-policy-device-id").arg(device_id);
}

fn policy_tenant_id(policy: &PolicyConfiguration) -> Option<&str> {
    policy_target_identifier(policy, 1)
}

fn verify_active_policies(
    configuration: &ServiceConfiguration,
    policy: &PolicyConfiguration,
    device_id: &str,
) -> Result<u64, StructuredError> {
    if policy
        .targets
        .iter()
        .any(|target| !active_policy_path(target).is_file())
    {
        return Err(error(
            "signedPolicyMissing",
            "all configured vendor, tenant, and device signed policies must be refreshed before filtering",
            false,
        ));
    }
    let mut command = Command::new(&configuration.engine_executable);
    command
        .arg("print-config")
        .arg("--config")
        .arg(&configuration.engine_config);
    append_active_policy_arguments(&mut command, policy, device_id);
    command_output(&mut command, "signed policy verification")?;
    policy
        .targets
        .iter()
        .filter_map(|target| active_policy_revision(&target.store_directory))
        .max()
        .ok_or_else(|| {
            error(
                "signedPolicyInvalid",
                "verified policy revision metadata is unavailable",
                false,
            )
        })
}

fn refresh_policy_target(
    configuration: &ServiceConfiguration,
    policy: &PolicyConfiguration,
    target: &PolicyTargetConfiguration,
    device_id: &str,
    minimum_revision: u64,
) -> Result<u64, StructuredError> {
    let mut command = Command::new(&configuration.engine_executable);
    command
        .arg("policy")
        .arg("refresh")
        .arg("--config")
        .arg(&configuration.engine_config)
        .arg("--metadata-directory")
        .arg(&policy.metadata_directory)
        .arg("--target-directory")
        .arg(&policy.target_directory)
        .arg("--metadata-base-url")
        .arg(&policy.metadata_base_url)
        .arg("--target-base-url")
        .arg(&policy.target_base_url)
        .arg("--bootstrap-root")
        .arg(&policy.bootstrap_root)
        .arg("--target-path")
        .arg(&target.target_path)
        .arg("--trusted-keys")
        .arg(&policy.trusted_keys);
    if let Some(tenant_id) = policy_tenant_id(policy) {
        command.arg("--tenant-id").arg(tenant_id);
    }
    command
        .arg("--device-id")
        .arg(device_id)
        .arg("--minimum-revision")
        .arg(minimum_revision.to_string())
        .arg("--policy-store")
        .arg(&target.store_directory);
    for origin in &policy.allowed_origins {
        command.arg("--allowed-origin").arg(origin);
    }
    let output = command_output(&mut command, "TUF policy refresh")?;
    let report: serde_json::Value =
        serde_json::from_str(&output).map_err(|_| policy_configuration_error())?;
    report["revision"].as_u64().ok_or_else(|| {
        error(
            "signedPolicyInvalid",
            "the verified TUF policy report did not contain a revision",
            false,
        )
    })
}

fn keygen_client(
    configuration: &ServiceConfiguration,
) -> Result<KeygenHttpClient, StructuredError> {
    KeygenHttpClient::new(keygen_configuration(configuration)?).map_err(license_provider_error)
}

fn create_secure_store(_state_directory: &Path) -> io::Result<Box<dyn SecureStore>> {
    #[cfg(windows)]
    {
        DpapiMachineStore::new(
            _state_directory.join("protected-secrets"),
            "com.localimagefilter.supervisor.v1",
        )
        .map(|store| Box::new(store) as Box<dyn SecureStore>)
        .map_err(io::Error::other)
    }
    #[cfg(unix)]
    {
        let key_path = std::env::var_os("LOCAL_FILTER_MACHINE_SECRET_KEY_FILE")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| io::Error::other("machine secret key path is unavailable"))?;
        #[cfg(target_os = "macos")]
        let store = MachineFileStore::new_or_create_key(
            _state_directory.join("protected-secrets"),
            &key_path,
            "com.localimagefilter.supervisor.v1",
        );
        #[cfg(not(target_os = "macos"))]
        let store = MachineFileStore::new(
            _state_directory.join("protected-secrets"),
            &key_path,
            "com.localimagefilter.supervisor.v1",
        );
        store
            .map(|store| Box::new(store) as Box<dyn SecureStore>)
            .map_err(io::Error::other)
    }
}

fn stable_machine_identifier(secure_store: &dyn SecureStore) -> Result<String, StructuredError> {
    #[cfg(windows)]
    let native = Command::new("reg.exe")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| output.split_whitespace().last().map(str::to_owned));
    #[cfg(target_os = "macos")]
    let native = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| {
            output.lines().find_map(|line| {
                line.split_once("IOPlatformUUID")
                    .and_then(|(_, value)| value.split('"').nth(1))
                    .map(str::to_owned)
            })
        });
    #[cfg(all(unix, not(target_os = "macos")))]
    let native = ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .map(|value| value.trim().to_owned());

    if let Some(identifier) = native.filter(|value| !value.is_empty() && value.len() <= 256) {
        return Ok(identifier);
    }
    if let Some(bytes) = secure_store
        .get("installation-identifier")
        .map_err(|_| secure_storage_error())?
    {
        return String::from_utf8(bytes).map_err(|_| secure_storage_error());
    }
    let identifier = Uuid::new_v4().to_string();
    secure_store
        .put("installation-identifier", identifier.as_bytes())
        .map_err(|_| secure_storage_error())?;
    Ok(identifier)
}

fn load_persisted_license(
    state_directory: &Path,
    secure_store: &dyn SecureStore,
) -> Option<VerifiedKeygenLicense> {
    let configuration = load_service_configuration(state_directory).ok()?;
    let keygen = keygen_configuration(&configuration).ok()?;
    let certificate = secure_store.get("license-file").ok()??;
    let private_key: [u8; 32] = secure_store
        .get("device-private-key")
        .ok()??
        .try_into()
        .ok()?;
    let public_key = SigningKey::from_bytes(&private_key)
        .verifying_key()
        .to_bytes();
    let public_key_base64 = STANDARD.encode(public_key);
    let machine_identifier = stable_machine_identifier(secure_store).ok()?;
    let mut digest = Sha256::new();
    digest.update(b"com.localimagefilter.device.v1\0");
    digest.update(machine_identifier.as_bytes());
    let fingerprint = format!("{:x}", digest.finalize());
    verify_keygen_machine_file(
        &certificate,
        &fingerprint,
        &public_key_base64,
        &keygen.product_id,
        &keygen.account_public_key,
        OffsetDateTime::now_utc(),
    )
    .ok()
}

fn license_state_name(state: LicenseState) -> &'static str {
    match state {
        LicenseState::Unactivated => "unactivated",
        LicenseState::ActiveOnline => "activeOnline",
        LicenseState::ActiveOffline => "activeOffline",
        LicenseState::Grace => "grace",
        LicenseState::Expired => "expired",
        LicenseState::Suspended => "suspended",
        LicenseState::Revoked => "revoked",
        LicenseState::ValidationError => "validationError",
    }
}

fn license_provider_error(value: LicenseError) -> StructuredError {
    match value {
        LicenseError::ActivationLimit => error(
            "activationLimitReached",
            "the Keygen machine activation limit was reached",
            false,
        ),
        LicenseError::Transport => error(
            "licenseProviderUnavailable",
            "Keygen could not be reached over its pinned HTTPS endpoint",
            true,
        ),
        LicenseError::Device => error(
            "licenseDeviceMismatch",
            "the signed machine license belongs to a different device",
            false,
        ),
        _ => error(
            "licenseVerificationFailed",
            "the license could not be cryptographically verified",
            false,
        ),
    }
}

fn secure_storage_error() -> StructuredError {
    error(
        "secureStorageFailed",
        "the operating-system protected secret store failed",
        true,
    )
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

#[cfg(windows)]
fn peer_process_id(stream: &Stream) -> u32 {
    stream
        .peer_creds()
        .ok()
        .and_then(|credentials| credentials.pid())
        .unwrap_or(0)
}

#[cfg(unix)]
fn peer_process_id(stream: &Stream) -> u32 {
    stream
        .peer_creds()
        .ok()
        .and_then(|credentials| credentials.pid())
        .and_then(|process_id| u32::try_from(process_id).ok())
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
                if let Ok(mut runtime) = runtime.lock() {
                    runtime.watchdog_tick(now_epoch_seconds());
                }
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
    use ed25519_dalek::Signer as _;

    #[test]
    fn password_policy_rejects_short_values() {
        assert_eq!(
            validate_password("short").unwrap_err().code,
            "passwordPolicy"
        );
    }

    #[test]
    fn signed_policy_assignment_is_tenant_device_and_revision_bound() {
        let temporary = std::env::temp_dir().join(format!("assignment-test-{}", Uuid::new_v4()));
        let tenant_id = Uuid::new_v4().to_string();
        let device_id = Uuid::new_v4().to_string();
        let key = SigningKey::from_bytes(&[8; 32]);
        let policy = PolicyConfiguration {
            metadata_directory: temporary.join("metadata"),
            target_directory: temporary.join("targets"),
            metadata_base_url: "https://policy.example/metadata".into(),
            target_base_url: "https://policy.example/targets".into(),
            bootstrap_root: temporary.join("root.json"),
            trusted_keys: temporary.join("keys.json"),
            assignment_key_id: "assignment-key".into(),
            assignment_public_key_base64: STANDARD.encode(key.verifying_key().to_bytes()),
            allowed_origins: vec!["https://policy.example".into()],
            targets: vec![
                PolicyTargetConfiguration {
                    target_path: "policies/channels/stable/policy-bundle.json".into(),
                    store_directory: temporary.join("vendor"),
                },
                PolicyTargetConfiguration {
                    target_path: format!("policies/tenants/{tenant_id}/policy-bundle.json"),
                    store_directory: temporary.join("tenant"),
                },
                PolicyTargetConfiguration {
                    target_path: format!("policies/devices/{device_id}/policy-bundle.json"),
                    store_directory: temporary.join("device"),
                },
            ],
        };
        let claims = PolicyAssignmentClaims {
            tenant_id,
            device_id: device_id.clone(),
            channel: "stable".into(),
            target_path: format!("policies/devices/{device_id}/policy-bundle.json"),
            minimum_revision: 1,
            issued_at: 100,
            expires_at: 1_000,
            nonce: "n".repeat(32),
        };
        let signature = STANDARD.encode(
            key.sign(&serde_json_canonicalizer::to_vec(&claims).unwrap())
                .to_bytes(),
        );
        let assignment = serde_json::json!({
            "claims": claims,
            "keyId": "assignment-key",
            "algorithm": "Ed25519",
            "signature": signature,
        });
        assert!(verify_policy_assignment(&policy, &assignment.to_string(), 500).is_ok());
        let mut wrong = assignment;
        wrong["claims"]["deviceId"] = serde_json::Value::String(Uuid::new_v4().to_string());
        assert_eq!(
            verify_policy_assignment(&policy, &wrong.to_string(), 500)
                .unwrap_err()
                .code,
            "policyAssignmentInvalid"
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
            "serviceConfigurationInvalid"
        );
        fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn generated_ca_is_unique_and_fingerprint_bound() {
        let first = std::env::temp_dir().join(format!("supervisor-ca-test-{}", Uuid::new_v4()));
        let second = std::env::temp_dir().join(format!("supervisor-ca-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let first_metadata = ensure_ca_material(&first).unwrap();
        let second_metadata = ensure_ca_material(&second).unwrap();
        assert_ne!(
            first_metadata.sha256_fingerprint,
            second_metadata.sha256_fingerprint
        );
        assert!(
            verify_certificate_file(
                &first.join("ca").join("mitmproxy-ca-cert.cer"),
                &first_metadata.sha256_fingerprint
            )
            .is_ok()
        );
        let combined = fs::read_to_string(first.join("ca").join("mitmproxy-ca.pem")).unwrap();
        assert!(combined.contains("BEGIN PRIVATE KEY"));
        assert!(combined.contains("BEGIN CERTIFICATE"));
        assert!(
            !serde_json::to_string(&first_metadata)
                .unwrap()
                .contains("PRIVATE KEY")
        );
        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }
}
