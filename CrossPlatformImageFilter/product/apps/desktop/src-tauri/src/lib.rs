#![forbid(unsafe_code)]

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{Name, Stream, prelude::*};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use supervisor_ipc::{Envelope, Request, Response, ResponseStatus, read_frame, write_frame};
use uuid::Uuid;

#[cfg(windows)]
const SOCKET_NAME: &str = "local-ai-image-filter.supervisor.v1";
#[cfg(unix)]
const UNIX_SOCKET_PATH: &str = "/var/run/local-ai-image-filter/supervisor.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingResult {
    status: ServiceStatus,
    recovery_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportBundle {
    path: String,
    sha256: String,
    bytes: usize,
}

impl ServiceStatus {
    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            state: "supervisorUnreachable".into(),
            engine_state: "unknown".into(),
            capture_backend: "unknown".into(),
            policy_name: "unknown".into(),
            policy_revision: 0,
            model_status: "unknown".into(),
            certificate_status: "unknown".into(),
            license_status: "unknown".into(),
            last_policy_update: None,
            last_application_update_check: None,
            degraded_reason: Some(reason.into()),
        }
    }
}

fn send_request<T>(request: Request) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let name = supervisor_endpoint()?;
    let mut stream =
        Stream::connect(name).map_err(|error| format!("supervisor unavailable: {error}"))?;
    let envelope = Envelope {
        protocol_version: 1,
        request_id: Uuid::new_v4(),
        nonce: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system clock is invalid: {error}"))?
            .as_secs() as i64,
        request,
    };
    write_frame(&mut stream, &envelope).map_err(|error| error.to_string())?;
    let response: Response<T> = read_frame(&mut stream).map_err(|error| error.to_string())?;
    if response.request_id != envelope.request_id || response.protocol_version != 1 {
        return Err("supervisor response correlation failed".into());
    }
    match (response.status, response.payload, response.error) {
        (ResponseStatus::Ok, Some(status), None) => Ok(status),
        (ResponseStatus::Error, None, Some(error)) => {
            Err(format!("{}: {}", error.code, error.message))
        }
        _ => Err("supervisor returned an invalid response envelope".into()),
    }
}

#[cfg(windows)]
fn supervisor_endpoint() -> Result<Name<'static>, String> {
    SOCKET_NAME
        .to_ns_name::<GenericNamespaced>()
        .map_err(|error| format!("invalid supervisor endpoint: {error}"))
}

#[cfg(unix)]
fn supervisor_endpoint() -> Result<Name<'static>, String> {
    std::path::Path::new(UNIX_SOCKET_PATH)
        .to_fs_name::<GenericFilePath>()
        .map(Name::into_owned)
        .map_err(|error| format!("invalid supervisor endpoint: {error}"))
}

#[tauri::command]
fn service_status() -> ServiceStatus {
    send_request(Request::GetStatus).unwrap_or_else(ServiceStatus::unavailable)
}

#[tauri::command]
fn start_protection() -> Result<ServiceStatus, String> {
    send_request(Request::StartFiltering)
}

#[tauri::command]
fn refresh_policy() -> Result<ServiceStatus, String> {
    send_request(Request::CheckPolicyUpdate)
}

#[tauri::command]
fn create_administrator_password(password: String) -> Result<OnboardingResult, String> {
    if password.len() < 12 || password.len() > 4096 {
        return Err("administrator password must contain between 12 and 4096 characters".into());
    }
    send_request(Request::SetAdminPassword { password })
}

#[tauri::command]
fn reset_administrator_password(
    new_password: String,
    recovery_code: Option<String>,
    recovery_token: Option<Vec<u8>>,
) -> Result<OnboardingResult, String> {
    if new_password.len() < 12 || new_password.len() > 4096 {
        return Err("administrator password must contain between 12 and 4096 characters".into());
    }
    send_request(Request::ResetAdminPassword {
        new_password,
        recovery_code,
        recovery_token,
    })
}

#[tauri::command]
fn activate_license(license_key: String) -> Result<ServiceStatus, String> {
    if license_key.is_empty() || license_key.len() > 4096 {
        return Err("license key is missing or oversized".into());
    }
    send_request(Request::ActivateLicense { license_key })
}

#[tauri::command]
fn import_offline_license(license: Vec<u8>) -> Result<ServiceStatus, String> {
    if license.is_empty() || license.len() > 1_048_576 {
        return Err("offline license is missing or oversized".into());
    }
    send_request(Request::ImportOfflineLicense { license })
}

#[tauri::command]
fn authenticate_administrator(password: String, scope: String) -> Result<String, String> {
    if password.is_empty() || password.len() > 4096 {
        return Err("administrator password is missing or oversized".into());
    }
    if !matches!(
        scope.as_str(),
        "stop" | "pause" | "repair" | "uninstall" | "deactivate" | "support"
    ) {
        return Err("unsupported authorization scope".into());
    }
    send_request(Request::VerifyAdminPassword { password, scope })
}

#[tauri::command]
fn install_or_repair_certificate(authorization: String) -> Result<ServiceStatus, String> {
    send_request(Request::InstallOrRepairCertificate { authorization })
}

#[tauri::command]
fn export_support_bundle(authorization: String) -> Result<SupportBundle, String> {
    send_request(Request::ExportSupportBundle { authorization })
}

#[tauri::command]
fn register_supervisor_service() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        use smappservice_rs::{AppService, ServiceStatus, ServiceType};

        let service = AppService::new(ServiceType::Daemon {
            plist_name: "com.localimagefilter.supervisor.plist",
        });
        service.register().map_err(|error| error.to_string())?;
        let status = service.status();
        if status == ServiceStatus::RequiresApproval {
            AppService::open_system_settings_login_items();
        }
        Ok(status.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("SMAppService registration is available only in the signed macOS application".into())
    }
}

#[tauri::command]
fn protected_operation(
    operation: &str,
    authorization: String,
    pause_seconds: Option<u32>,
) -> Result<ServiceStatus, String> {
    if authorization.is_empty() || authorization.len() > 4096 {
        return Err("administrator authorization is missing or oversized".into());
    }
    let request = match operation {
        "stop" => Request::StopFiltering { authorization },
        "pause" => Request::PauseFiltering {
            authorization,
            seconds: pause_seconds
                .filter(|seconds| *seconds <= 86_400)
                .ok_or("invalid pause duration")?,
        },
        "repair" => Request::RepairNetworkConfiguration { authorization },
        "uninstall" => Request::RequestUninstallAuthorization { authorization },
        "deactivate" => Request::DeactivateDevice { authorization },
        _ => return Err("unsupported protected operation".into()),
    };
    send_request(request)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            service_status,
            start_protection,
            refresh_policy,
            create_administrator_password,
            reset_administrator_password,
            activate_license,
            import_offline_license,
            authenticate_administrator,
            register_supervisor_service,
            install_or_repair_certificate,
            export_support_bundle,
            protected_operation
        ])
        .run(tauri::generate_context!())
        .expect("failed to run desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_operations_cannot_reach_ipc() {
        let result = protected_operation("executeShell", "token".into(), None);
        assert!(matches!(result, Err(message) if message == "unsupported protected operation"));
    }

    #[test]
    fn unavailable_status_is_explicit_not_optimistic() {
        let status = ServiceStatus::unavailable("not running");
        assert_eq!(status.state, "supervisorUnreachable");
        assert_ne!(status.state, "running");
    }
}
