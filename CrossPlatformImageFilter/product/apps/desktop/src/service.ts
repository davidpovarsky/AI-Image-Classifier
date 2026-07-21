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
  | "serviceUnavailable";

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
  scope: ProtectedOperation,
): Promise<string> {
  return invoke<string>("authenticate_administrator", { password, scope });
}

export async function registerSupervisorService(): Promise<string> {
  return invoke<string>("register_supervisor_service");
}

export async function refreshPolicy(): Promise<ServiceStatus> {
  return invoke<ServiceStatus>("refresh_policy");
}
