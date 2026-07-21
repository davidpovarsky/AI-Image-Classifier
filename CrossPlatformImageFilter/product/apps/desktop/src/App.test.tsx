import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";

vi.mock("./service", () => ({
  readStatus: vi.fn(() => Promise.resolve({
    state: "running", engineState: "healthy", captureBackend: "regularProxy", policyName: "stable",
    policyRevision: 7, modelStatus: "verified", certificateStatus: "trusted", licenseStatus: "active",
    lastPolicyUpdate: null, lastApplicationUpdateCheck: null, degradedReason: null,
  })),
  startProtection: vi.fn(), protectedOperation: vi.fn(), refreshPolicy: vi.fn(),
}));

describe("desktop UI", () => {
  beforeEach(() => { document.documentElement.dir = "ltr"; });

  it("renders actual supervisor status", async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByRole("heading", { name: "running" })).toBeInTheDocument());
    expect(screen.getByText("stable r7")).toBeInTheDocument();
  });

  it("switches between English LTR and Hebrew RTL", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "עברית" }));
    expect(document.documentElement.dir).toBe("rtl");
    expect(screen.getByRole("heading", { name: "מסנן התמונות המקומי" })).toBeInTheDocument();
  });

  it("supports keyboard-addressable primary navigation", () => {
    render(<App />);
    const policy = screen.getByRole("button", { name: "Policy" });
    policy.focus();
    expect(policy).toHaveFocus();
    fireEvent.click(policy);
    expect(policy).toHaveAttribute("aria-current", "page");
  });
});
