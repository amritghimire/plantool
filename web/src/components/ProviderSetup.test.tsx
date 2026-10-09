import { fireEvent, render, screen } from "@testing-library/react";
import { ProviderSetup } from "./ProviderSetup";
it("distinguishes missing, unsigned and unverified providers and offers re-check", () => {
  const recheck = vi.fn();
  render(<ProviderSetup onRecheck={recheck} providers={[{ id: "claude", available: false, version: "1", error: "Not signed in" }, { id: "codex", available: false }, { id: "copilot", available: true, version: "1", error: "Check login manually" }]} />);
  expect(screen.getByText("Not signed in or not ready")).toBeInTheDocument();
  expect(screen.getByText("Not installed or unavailable")).toBeInTheDocument();
  expect(screen.getByText("Sign-in not verified")).toBeInTheDocument();
  expect(screen.queryByText("Ready")).toBeNull();
  fireEvent.click(screen.getByText("Re-check"));
  expect(recheck).toHaveBeenCalledOnce();
});
