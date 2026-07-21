const forbiddenFields = new Set([
  "browsingHistory",
  "classificationEvents",
  "image",
  "imageBytes",
  "imageCrops",
  "pageContent",
  "url",
  "urls",
]);

export type AdminOperation =
  | "device/locate"
  | "device/policy-channel"
  | "device/policy-refresh"
  | "device/policy-acknowledgement"
  | "device/license-entitlement"
  | "device/audit-history";

export function assertPrivacySafePayload(value: unknown): void {
  if (Array.isArray(value)) {
    value.forEach(assertPrivacySafePayload);
    return;
  }
  if (value && typeof value === "object") {
    for (const [key, nested] of Object.entries(value)) {
      if (forbiddenFields.has(key)) {
        throw new Error(`Forbidden privacy field: ${key}`);
      }
      assertPrivacySafePayload(nested);
    }
  }
}

export function validateControlPlaneOrigin(origin: string, development = false): URL {
  const parsed = new URL(origin);
  const loopback = parsed.hostname === "127.0.0.1" || parsed.hostname === "localhost";
  if (parsed.protocol !== "https:" && !(development && loopback)) {
    throw new Error("The administrative control plane must use HTTPS");
  }
  if (parsed.username || parsed.password || parsed.pathname !== "/") {
    throw new Error("The control-plane origin must not contain credentials or a path");
  }
  return parsed;
}

export async function adminRequest<T>(
  origin: string,
  accessToken: string,
  operation: AdminOperation,
  payload: Record<string, unknown>,
): Promise<T> {
  if (!accessToken) throw new Error("An OIDC access token is required");
  assertPrivacySafePayload(payload);
  const base = validateControlPlaneOrigin(origin, import.meta.env.DEV);
  const response = await fetch(new URL(`/v1/admin/${operation}`, base), {
    method: "POST",
    headers: {
      authorization: `Bearer ${accessToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify(payload),
    credentials: "omit",
    redirect: "error",
  });
  if (!response.ok) throw new Error(`Administrative request failed (${String(response.status)})`);
  return response.json() as Promise<T>;
}
