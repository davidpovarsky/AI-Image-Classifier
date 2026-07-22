import { invoke } from "@tauri-apps/api/core";

export type SupervisorState =
  | "uninstalled"
  | "needsActivation"
  | "needsOnboarding"
  | "stopped"
  | "starting"
  | "running"
  | "degraded"
  | "updating"
  | "stopping"
  | "recovering"
  | "error"
  | "supervisorUnreachable";

export interface ServiceStatus {
  state: SupervisorState;
  engineState: string;
  captureBackend: string;
  policyName: string;
  policyRevision: number;
  modelStatus: string;
  certificateStatus: string;
  licenseStatus: string;
  lastPolicyUpdate: string | null;
  lastApplicationUpdateCheck: string | null;
  degradedReason: string | null;
}

export type ProtectedOperation = "stop" | "pause" | "repair" | "uninstall" | "deactivate";
export type AuthorizationScope = ProtectedOperation | "support";

export interface OnboardingResult {
  status: ServiceStatus;
  recoveryCode: string;
}

export interface SupportBundle {
  path: string;
  sha256: string;
  bytes: number;
}

export async function readStatus(): Promise<ServiceStatus> {
  return invoke<ServiceStatus>("service_status");
}

export async function startProtection(): Promise<ServiceStatus> {
  return invoke<ServiceStatus>("start_protection");
}

export async function protectedOperation(
  operation: ProtectedOperation,
  authorization: string,
  pauseSeconds?: number,
): Promise<ServiceStatus> {
  return invoke<ServiceStatus>("protected_operation", { operation, authorization, pauseSeconds });
}

export async function authenticateAdministrator(
  password: string,
  scope: AuthorizationScope,
): Promise<string> {
  return invoke<string>("authenticate_administrator", { password, scope });
}

export async function installOrRepairCertificate(authorization: string): Promise<ServiceStatus> {
  return invoke<ServiceStatus>("install_or_repair_certificate", { authorization });
}

export async function exportSupportBundle(authorization: string): Promise<SupportBundle> {
  return invoke<SupportBundle>("export_support_bundle", { authorization });
}

export async function registerSupervisorService(): Promise<string> {
  return invoke<string>("register_supervisor_service");
}

export async function refreshPolicy(): Promise<ServiceStatus> {
  return invoke<ServiceStatus>("refresh_policy");
}

export async function createAdministratorPassword(password: string): Promise<OnboardingResult> {
  return invoke<OnboardingResult>("create_administrator_password", { password });
}

export async function resetAdministratorPassword(
  newPassword: string,
  recoveryCode?: string,
  recoveryToken?: Uint8Array,
): Promise<OnboardingResult> {
  return invoke<OnboardingResult>("reset_administrator_password", {
    newPassword,
    recoveryCode: recoveryCode ?? null,
    recoveryToken: recoveryToken ? Array.from(recoveryToken) : null,
  });
}

export async function activateLicense(licenseKey: string): Promise<ServiceStatus> {
  return invoke<ServiceStatus>("activate_license", { licenseKey });
}

export async function importOfflineLicense(license: Uint8Array): Promise<ServiceStatus> {
  return invoke<ServiceStatus>("import_offline_license", { license: Array.from(license) });
}
