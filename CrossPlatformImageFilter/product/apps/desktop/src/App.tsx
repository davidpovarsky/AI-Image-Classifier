import { type SyntheticEvent, useEffect, useState } from "react";
import {
  activateLicense,
  applyPolicyAssignment,
  checkApplicationUpdate,
  authenticateAdministrator,
  createAdministratorPassword,
  exportSupportBundle,
  importOfflineLicense,
  installOrRepairCertificate,
  installApplicationUpdate,
  protectedOperation,
  readStatus,
  refreshPolicy,
  registerSupervisorService,
  resetAdministratorPassword,
  startProtection,
  type ProtectedOperation,
  type ServiceStatus,
} from "./service";
import "./styles.css";

type Locale = "en" | "he";
type Screen = "status" | "activation" | "onboarding" | "protection" | "policy" | "diagnostics" | "updates" | "license" | "administrator";

const labels = {
  en: {
    title: "Local AI Image Filter", status: "Status", activation: "Activation", onboarding: "Onboarding",
    protection: "Protection", policy: "Policy", diagnostics: "Diagnostics", updates: "Updates",
    license: "Account & license", administrator: "Administrator", refresh: "Refresh actual service state",
    unavailable: "The supervisor service could not be reached. No local setting was changed.", checkPolicy: "Check for policy updates",
  },
  he: {
    title: "מסנן התמונות המקומי", status: "מצב", activation: "הפעלה", onboarding: "הגדרה ראשונית",
    protection: "הגנה", policy: "מדיניות", diagnostics: "אבחון", updates: "עדכונים",
    license: "חשבון ורישיון", administrator: "מנהל מערכת", refresh: "רענון מצב השירות בפועל",
    unavailable: "לא ניתן ליצור קשר עם שירות המפקח. לא שונתה הגדרה מקומית.", checkPolicy: "בדיקת עדכוני מדיניות",
  },
} as const;

const initialStatus: ServiceStatus = {
  state: "supervisorUnreachable", engineState: "unknown", captureBackend: "unknown", policyName: "unknown",
  policyRevision: 0, modelStatus: "unknown", certificateStatus: "unknown", licenseStatus: "unknown",
  lastPolicyUpdate: null, lastApplicationUpdateCheck: null, degradedReason: null,
};

export function App() {
  const [locale, setLocale] = useState<Locale>("en");
  const [screen, setScreen] = useState<Screen>("status");
  const [status, setStatus] = useState<ServiceStatus>(initialStatus);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const text = labels[locale];

  const execute = async (operation: () => Promise<ServiceStatus>) => {
    setBusy(true); setMessage("");
    try { setStatus(await operation()); }
    catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  };

  useEffect(() => { void execute(readStatus); }, []);
  useEffect(() => { document.documentElement.lang = locale; document.documentElement.dir = locale === "he" ? "rtl" : "ltr"; }, [locale]);
  const navigation: Screen[] = ["status", "activation", "onboarding", "protection", "policy", "diagnostics", "updates", "license", "administrator"];
  return <div className="app-shell">
    <header><div><p className="eyebrow">Commercial desktop</p><h1>{text.title}</h1></div><button className="locale" onClick={() => { setLocale(locale === "en" ? "he" : "en"); }}>{locale === "en" ? "עברית" : "English"}</button></header>
    <div className="layout"><nav aria-label="Primary">{navigation.map((item) => <button key={item} aria-current={screen === item ? "page" : undefined} onClick={() => { setScreen(item); }}>{text[item]}</button>)}</nav>
      <main id="main-content" tabIndex={-1}>
        <section className={`hero state-${status.state}`} aria-live="polite"><span className="status-dot" aria-hidden="true" /><div><p>Supervisor</p><h2>{status.state}</h2></div><button disabled={busy} onClick={() => { void execute(readStatus); }}>{text.refresh}</button></section>
        {status.state === "supervisorUnreachable" && <p role="alert" className="warning">{text.unavailable}</p>}{message && <p role="alert" className="warning">{message}</p>}
        {screen === "status" && <StatusGrid status={status} />}
        {screen === "activation" && <ActivationPanel busy={busy} execute={execute} />}
        {screen === "onboarding" && <OnboardingPanel busy={busy} setBusy={setBusy} setMessage={setMessage} setStatus={setStatus} />}
        {screen === "protection" && <ProtectionPanel busy={busy} execute={execute} />}
        {screen === "policy" && <PolicyPanel status={status} busy={busy} execute={execute} checkLabel={text.checkPolicy} />}
        {screen === "diagnostics" && <DiagnosticsPanel busy={busy} setBusy={setBusy} setMessage={setMessage} />}
        {screen === "updates" && <UpdatePanel busy={busy} setBusy={setBusy} setMessage={setMessage} />}
        {screen === "license" && <section className="panel"><h2>{text.license}</h2><p>Entitlement state: {status.licenseStatus}</p><p>Device deactivation requires administrator authorization and contacts the license provider.</p></section>}
        {screen === "administrator" && <RecoveryPanel busy={busy} setBusy={setBusy} setMessage={setMessage} setStatus={setStatus} />}
      </main>
    </div>
  </div>;
}

function ActivationPanel({ busy, execute }: { busy: boolean; execute: (action: () => Promise<ServiceStatus>) => Promise<void> }) {
  const [licenseKey, setLicenseKey] = useState("");
  const submit = (event: SyntheticEvent<HTMLFormElement>) => { event.preventDefault(); void execute(() => activateLicense(licenseKey)).then(() => { setLicenseKey(""); }); };
  const importFile = (file: File | undefined) => { if (file) void file.arrayBuffer().then((bytes) => execute(() => importOfflineLicense(new Uint8Array(bytes)))); };
  return <section className="panel"><h2>Activation</h2><form onSubmit={submit}><label>License key<input type="password" autoComplete="off" value={licenseKey} onChange={(event) => { setLicenseKey(event.target.value); }} /></label><button disabled={busy || !licenseKey}>Activate this device</button></form><label className="file-input">Signed offline machine license<input type="file" accept=".lic,application/octet-stream" disabled={busy} onChange={(event) => { importFile(event.target.files?.[0]); event.currentTarget.value = ""; }} /></label></section>;
}

function OnboardingPanel({ busy, setBusy, setMessage, setStatus }: { busy: boolean; setBusy: (value: boolean) => void; setMessage: (value: string) => void; setStatus: (value: ServiceStatus) => void }) {
  const [password, setPassword] = useState(""); const [confirmation, setConfirmation] = useState(""); const [recoveryCode, setRecoveryCode] = useState("");
  const submit = async (event: SyntheticEvent<HTMLFormElement>) => { event.preventDefault(); if (password !== confirmation) { setMessage("Password confirmation does not match."); return; } setBusy(true); setMessage(""); try { const result = await createAdministratorPassword(password); setStatus(result.status); setRecoveryCode(result.recoveryCode); setPassword(""); setConfirmation(""); } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); } finally { setBusy(false); } };
  return <section className="panel"><h2>First-run onboarding</h2>{!recoveryCode && <form onSubmit={(event) => { void submit(event); }}><label>Administrator password<input type="password" autoComplete="new-password" minLength={12} value={password} onChange={(event) => { setPassword(event.target.value); }} /></label><label>Confirm password<input type="password" autoComplete="new-password" minLength={12} value={confirmation} onChange={(event) => { setConfirmation(event.target.value); }} /></label><button disabled={busy || password.length < 12}>Create administrator credentials</button></form>}{recoveryCode && <div className="recovery"><h3>Save this one-time recovery code now</h3><output aria-label="Recovery code">{recoveryCode}</output><p>It is stored only as an Argon2id hash and cannot be displayed again.</p></div>}<div className="actions"><button disabled={busy} onClick={() => { void registerSupervisorService().then(setMessage).catch((error: unknown) => { setMessage(error instanceof Error ? error.message : String(error)); }); }}>Request service approval</button></div></section>;
}

function ProtectionPanel({ busy, execute }: { busy: boolean; execute: (action: () => Promise<ServiceStatus>) => Promise<void> }) {
  const [password, setPassword] = useState("");
  const authorize = async (operation: ProtectedOperation, pauseSeconds?: number) => { await execute(async () => { const authorization = await authenticateAdministrator(password, operation); const result = await protectedOperation(operation, authorization, pauseSeconds); setPassword(""); return result; }); };
  const repairCertificate = async () => { await execute(async () => { const authorization = await authenticateAdministrator(password, "repair"); const result = await installOrRepairCertificate(authorization); setPassword(""); return result; }); };
  return <section className="panel"><h2>Protection controls</h2><label>Administrator password for protected actions<input type="password" autoComplete="current-password" value={password} onChange={(event) => { setPassword(event.target.value); }} /></label><div className="actions"><button disabled={busy} onClick={() => { void execute(startProtection); }}>Start / resume</button><button disabled={busy || !password} onClick={() => { void authorize("stop"); }}>Stop</button><button disabled={busy || !password} onClick={() => { void authorize("pause", 900); }}>Pause 15 minutes</button><button disabled={busy || !password} onClick={() => { void authorize("repair"); }}>Repair network</button><button disabled={busy || !password} onClick={() => { void repairCertificate(); }}>Install or repair trusted CA</button></div></section>;
}

function PolicyPanel({ status, busy, execute, checkLabel }: { status: ServiceStatus; busy: boolean; execute: (action: () => Promise<ServiceStatus>) => Promise<void>; checkLabel: string }) {
  const [password, setPassword] = useState(""); const [assignment, setAssignment] = useState("");
  const loadAssignment = (file: File | undefined) => { if (file) void file.text().then((text) => { const parsed = JSON.parse(text) as { signedAssignment?: unknown }; setAssignment(JSON.stringify(parsed.signedAssignment ?? parsed)); }); };
  const apply = async () => { await execute(async () => { const authorization = await authenticateAdministrator(password, "policy"); const result = await applyPolicyAssignment(assignment, authorization); setPassword(""); setAssignment(""); return result; }); };
  return <section className="panel"><h2>Policy</h2><p>{status.policyName} · revision {String(status.policyRevision)}</p><p>Company-managed. Threshold details are read-only.</p><div className="actions"><button disabled={busy} onClick={() => { void execute(refreshPolicy); }}>{checkLabel}</button></div><label className="file-input">Signed device assignment<input type="file" accept=".json,application/json" disabled={busy} onChange={(event) => { loadAssignment(event.target.files?.[0]); event.currentTarget.value = ""; }} /></label><label>Administrator password<input type="password" autoComplete="current-password" value={password} onChange={(event) => { setPassword(event.target.value); }} /></label><button disabled={busy || !password || !assignment} onClick={() => { void apply(); }}>Verify assignment and refresh device policy</button></section>;
}

function RecoveryPanel({ busy, setBusy, setMessage, setStatus }: { busy: boolean; setBusy: (value: boolean) => void; setMessage: (value: string) => void; setStatus: (value: ServiceStatus) => void }) {
  const [code, setCode] = useState(""); const [token, setToken] = useState<Uint8Array>(); const [password, setPassword] = useState(""); const [newCode, setNewCode] = useState("");
  const submit = async (event: SyntheticEvent<HTMLFormElement>) => { event.preventDefault(); setBusy(true); setMessage(""); try { const result = await resetAdministratorPassword(password, code || undefined, token); setStatus(result.status); setNewCode(result.recoveryCode); setCode(""); setToken(undefined); setPassword(""); } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); } finally { setBusy(false); } };
  const loadToken = (file: File | undefined) => { if (file) void file.arrayBuffer().then((bytes) => { setCode(""); setToken(new Uint8Array(bytes)); }); };
  return <section className="panel"><h2>Administrator recovery</h2><form onSubmit={(event) => { void submit(event); }}><label>One-time recovery code<input autoComplete="off" value={code} onChange={(event) => { setCode(event.target.value); setToken(undefined); }} /></label><label className="file-input">Or signed recovery token<input type="file" accept=".json,application/json" disabled={busy} onChange={(event) => { loadToken(event.target.files?.[0]); }} /></label><label>New administrator password<input type="password" autoComplete="new-password" minLength={12} value={password} onChange={(event) => { setPassword(event.target.value); }} /></label><button disabled={busy || (!code && !token) || password.length < 12}>Reset and rotate recovery code</button></form>{newCode && <div className="recovery"><h3>New one-time recovery code</h3><output>{newCode}</output></div>}</section>;
}

function DiagnosticsPanel({ busy, setBusy, setMessage }: { busy: boolean; setBusy: (value: boolean) => void; setMessage: (value: string) => void }) {
  const [password, setPassword] = useState(""); const [bundle, setBundle] = useState("");
  const exportBundle = async () => { setBusy(true); setMessage(""); try { const authorization = await authenticateAdministrator(password, "support"); const result = await exportSupportBundle(authorization); setBundle(`${result.path} · SHA-256 ${result.sha256} · ${String(result.bytes)} bytes`); setPassword(""); } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); } finally { setBusy(false); } };
  return <section className="panel"><h2>Diagnostics</h2><p>Support bundles exclude images, crops, URLs, CA private keys, password hashes, license keys, and device private keys.</p><label>Administrator password<input type="password" autoComplete="current-password" value={password} onChange={(event) => { setPassword(event.target.value); }} /></label><button disabled={busy || !password} onClick={() => { void exportBundle(); }}>Export redacted support bundle</button>{bundle && <output className="bundle-result">{bundle}</output>}</section>;
}

function UpdatePanel({ busy, setBusy, setMessage }: { busy: boolean; setBusy: (value: boolean) => void; setMessage: (value: string) => void }) {
  const [availableVersion, setAvailableVersion] = useState(""); const [notes, setNotes] = useState("");
  const check = async () => { setBusy(true); setMessage(""); try { const update = await checkApplicationUpdate(); setAvailableVersion(update.version ?? ""); setNotes(update.available ? update.notes ?? "Signed update is available." : "This installation is current."); } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); } finally { setBusy(false); } };
  const install = async () => { setBusy(true); setMessage(""); try { await installApplicationUpdate(); setMessage("The signed update was verified and handed to the platform installer."); } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); } finally { setBusy(false); } };
  return <section className="panel"><h2>Application updates</h2><p>Application, policy, and model updates use separate channels and trust roots.</p><div className="actions"><button disabled={busy} onClick={() => { void check(); }}>Check signed application update</button><button disabled={busy || !availableVersion} onClick={() => { void install(); }}>Install version {availableVersion || "—"}</button></div>{notes && <p>{notes}</p>}</section>;
}

function StatusGrid({ status }: { status: ServiceStatus }) { const rows = [["Engine", status.engineState], ["Capture", status.captureBackend], ["Policy", `${status.policyName} r${String(status.policyRevision)}`], ["Models", status.modelStatus], ["Certificate", status.certificateStatus], ["License", status.licenseStatus]]; return <section className="cards" aria-label="Health details">{rows.map(([name, value]) => <article key={name}><p>{name}</p><strong>{value}</strong></article>)}</section>; }
