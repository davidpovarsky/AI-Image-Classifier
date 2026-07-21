import { describe, expect, it } from "vitest";
import { assertPrivacySafePayload, validateControlPlaneOrigin } from "./api";

describe("administrative API boundary", () => {
  it("requires HTTPS outside explicit loopback development", () => {
    expect(() => validateControlPlaneOrigin("http://control.example/")).toThrow(/HTTPS/);
    expect(validateControlPlaneOrigin("https://control.example/").origin).toBe(
      "https://control.example",
    );
    expect(validateControlPlaneOrigin("http://127.0.0.1:8787/", true).origin).toBe(
      "http://127.0.0.1:8787",
    );
  });

  it("rejects privacy-sensitive fields at any nesting level", () => {
    expect(() => {
      assertPrivacySafePayload({ deviceId: "device", nested: { url: "secret" } });
    }).toThrow(/Forbidden privacy field: url/);
    expect(() => {
      assertPrivacySafePayload({ deviceId: "device", policyRevision: 9 });
    }).not.toThrow();
  });
});
