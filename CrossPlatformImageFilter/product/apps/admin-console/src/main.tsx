import { StrictMode, useState, type SyntheticEvent } from "react";
import { createRoot } from "react-dom/client";
import { adminRequest, type AdminOperation } from "./api";
import "./style.css";

function App() {
  const [origin, setOrigin] = useState(import.meta.env.DEV ? "http://127.0.0.1:8787/" : "");
  const [accessToken, setAccessToken] = useState("");
  const [tenantId, setTenantId] = useState("");
  const [deviceId, setDeviceId] = useState("");
  const [channel, setChannel] = useState("stable");
  const [result, setResult] = useState("No operation has run.");
  const [busy, setBusy] = useState(false);

  const execute = async (operation: AdminOperation, payload: Record<string, unknown>) => {
    setBusy(true);
    try {
      const response = await adminRequest<unknown>(
        origin,
        accessToken,
        operation,
        payload,
        import.meta.env.DEV ? tenantId : undefined,
      );
      setResult(JSON.stringify(response, null, 2));
    } catch (error) {
      setResult(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const submit = (event: SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();
    void execute("device/locate", { deviceId });
  };

  const ready = origin.length > 0 && accessToken.length > 0 && deviceId.length > 0
    && (!import.meta.env.DEV || tenantId.length > 0);

  return <main>
    <p className="eyebrow">OIDC-protected operations</p>
    <h1>Local AI Image Filter administration</h1>
    <p>This console never receives images, crops, browsing URLs, page content, or classification events.</p>
    <form className="connection" onSubmit={submit}>
      <label>Control-plane origin<input type="url" required value={origin} onChange={(event) => { setOrigin(event.target.value); }} /></label>
      <label>{import.meta.env.DEV ? "Development subject" : "OIDC access token"}<input type="password" required autoComplete="off" value={accessToken} onChange={(event) => { setAccessToken(event.target.value); }} /></label>
      {import.meta.env.DEV && <label>Development tenant ID<input required value={tenantId} onChange={(event) => { setTenantId(event.target.value); }} /></label>}
      <label>Device ID<input required value={deviceId} onChange={(event) => { setDeviceId(event.target.value); }} /></label>
      <button disabled={!ready || busy}>Locate device</button>
    </form>
    <section aria-label="Administrative operations">
      <div className="channel"><label>Policy channel<select value={channel} onChange={(event) => { setChannel(event.target.value); }}><option value="stable">Stable</option><option value="beta">Beta</option></select></label><button disabled={!ready || busy} onClick={() => { void execute("device/policy-channel", { deviceId, channel }); }}>Assign policy channel</button></div>
      <button disabled={!ready || busy} onClick={() => { void execute("device/policy-refresh", { deviceId }); }}>Request policy refresh</button>
      <button disabled={!ready || busy} onClick={() => { void execute("device/policy-acknowledgement", { deviceId }); }}>View policy acknowledgement</button>
      <button disabled={!ready || busy} onClick={() => { void execute("device/license-entitlement", { deviceId }); }}>View license entitlement</button>
      <button disabled={!ready || busy} onClick={() => { void execute("device/audit-history", { deviceId }); }}>View audit history</button>
    </section>
    <h2>Verified control-plane response</h2>
    <pre aria-live="polite">{result}</pre>
  </main>;
}

const root = document.getElementById("root");
if (!root) throw new Error("Missing application root");
createRoot(root).render(<StrictMode><App /></StrictMode>);
