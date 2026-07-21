import { useEffect, useState } from "react";
import { authenticateAdministrator, protectedOperation, readStatus, refreshPolicy, registerSupervisorService, startProtection, type ServiceStatus } from "./service";
import "./styles.css";

type Locale = "en" | "he";
type Screen = "status" | "activation" | "onboarding" | "protection" | "policy" | "diagnostics" | "updates" | "license" | "administrator";

const labels = {
  en: {
    title: "Local AI Image Filter", status: "Status", activation: "Activation", onboarding: "Onboarding",
    protection: "Protection", policy: "Policy", diagnostics: "Diagnostics", updates: "Updates",
    license: "Account & license", administrator: "Administrator", refresh: "Refresh actual service state",
    unavailable: "The supervisor service could not be reached. No local setting was changed.", start: "Start protection",
    stop: "Stop (authorization required)", checkPolicy: "Check for policy updates",
  },
  he: {
    title: "מסנן התמונות המקומי", status: "מצב", activation: "הפעלה", onboarding: "הגדרה ראשונית",
    protection: "הגנה", policy: "מדיניות", diagnostics: "אבחון", updates: "עדכונים",
    license: "חשבון ורישיון", administrator: "מנהל מערכת", refresh: "רענון מצב השירות בפועל",
    unavailable: "לא ניתן ליצור קשר עם שירות המפקח. לא שונתה הגדרה מקומית.", start: "הפעלת הגנה",
    stop: "עצירה (נדרש אישור)", checkPolicy: "בדיקת עדכוני מדיניות",
  },
} as const;

const initialStatus: ServiceStatus = {
  state: "serviceUnavailable", engineState: "unknown", captureBackend: "unknown", policyName: "unknown",
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
    setBusy(true);
    setMessage("");
    try { setStatus(await operation()); }
    catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  };

  useEffect(() => { void execute(readStatus); }, []);
  useEffect(() => {
    document.documentElement.lang = locale;
    document.documentElement.dir = locale === "he" ? "rtl" : "ltr";
  }, [locale]);

  const navigation: Screen[] = ["status", "activation", "onboarding", "protection", "policy", "diagnostics", "updates", "license", "administrator"];
  return <div className="app-shell">
    <header>
      <div><p className="eyebrow">Commercial desktop</p><h1>{text.title}</h1></div>
      <button className="locale" onClick={() => { setLocale(locale === "en" ? "he" : "en"); }}>{locale === "en" ? "עברית" : "English"}</button>
    </header>
    <div className="layout">
      <nav aria-label="Primary">
        {navigation.map((item) => <button key={item} aria-current={screen === item ? "page" : undefined} onClick={() => { setScreen(item); }}>{text[item]}</button>)}
      </nav>
      <main id="main-content" tabIndex={-1}>
        <section className={`hero state-${status.state}`} aria-live="polite">
          <span className="status-dot" aria-hidden="true" /><div><p>Supervisor</p><h2>{status.state}</h2></div>
          <button disabled={busy} onClick={() => { void execute(readStatus); }}>{text.refresh}</button>
        </section>
        {status.state === "serviceUnavailable" && <p role="alert" className="warning">{text.unavailable}</p>}
        {message && <p role="alert" className="warning">{message}</p>}
        {screen === "status" && <StatusGrid status={status} />}
        {screen === "activation" && <Panel title={text.activation} body="Enter a license key or import a signed offline license. Device-limit errors are reported by the service." />}
        {screen === "onboarding" && <section className="panel"><h2>{text.onboarding}</h2><p>Administrator password, recovery code, service approval, CA consent, network permission, and health checks are completed transactionally.</p><button disabled={busy} onClick={() => { void registerSupervisorService().then(setMessage).catch((error: unknown) => { setMessage(error instanceof Error ? error.message : String(error)); }); }}>Register signed macOS service</button></section>}
        {screen === "protection" && <section className="panel"><h2>{text.protection}</h2><div className="actions"><button disabled={busy} onClick={() => { void execute(startProtection); }}>{text.start}</button><button disabled={busy} onClick={() => { void requestProtected("stop", execute); }}>{text.stop}</button></div></section>}
        {screen === "policy" && <section className="panel"><h2>{text.policy}</h2><p>{status.policyName} · revision {String(status.policyRevision)}</p><p>Company-managed. Threshold details are read-only.</p><button disabled={busy} onClick={() => { void execute(refreshPolicy); }}>{text.checkPolicy}</button></section>}
        {screen === "diagnostics" && <Panel title={text.diagnostics} body="Support bundles exclude images, crops, URLs, CA private keys, password hashes, license keys, and device private keys." />}
        {screen === "updates" && <Panel title={text.updates} body={`Signed application updater status: ${status.lastApplicationUpdateCheck ?? "not checked"}. Policy and model updates use separate trust roots.`} />}
        {screen === "license" && <Panel title={text.license} body={`Entitlement state: ${status.licenseStatus}. Device deactivation requires administrator authorization.`} />}
        {screen === "administrator" && <Panel title={text.administrator} body="Change password, review recovery guidance, authorize safe uninstall, and inspect the redacted audit summary." />}
      </main>
    </div>
  </div>;
}

async function requestProtected(operation: "stop", execute: (action: () => Promise<ServiceStatus>) => Promise<void>) {
  const password = window.prompt("Administrator password");
  if (password) {
    await execute(async () => {
      const authorization = await authenticateAdministrator(password, operation);
      return protectedOperation(operation, authorization);
    });
  }
}

function StatusGrid({ status }: { status: ServiceStatus }) {
  const rows = [
    ["Engine", status.engineState], ["Capture", status.captureBackend], ["Policy", `${status.policyName} r${String(status.policyRevision)}`],
    ["Models", status.modelStatus], ["Certificate", status.certificateStatus], ["License", status.licenseStatus],
  ];
  return <section className="cards" aria-label="Health details">{rows.map(([name, value]) => <article key={name}><p>{name}</p><strong>{value}</strong></article>)}</section>;
}

function Panel({ title, body }: { title: string; body: string }) {
  return <section className="panel"><h2>{title}</h2><p>{body}</p></section>;
}
