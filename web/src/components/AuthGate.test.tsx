import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi, beforeEach, test, expect } from "vitest";
import { AuthGate } from "./AuthGate";
import { authenticate, authRequest, registerPasskey } from "../auth";
vi.mock("../auth", () => ({ authenticate: vi.fn(), authRequest: vi.fn(), registerPasskey: vi.fn(), SESSION_CHANGED_EVENT: "plantool:session-changed" }));
beforeEach(() => vi.resetAllMocks());
test("first enrollment asks for a terminal setup code before rendering the application", async () => {
  vi.mocked(authRequest).mockResolvedValueOnce({ registered: false, authenticated: false }).mockResolvedValueOnce({ registered: true, authenticated: true });
  render(<AuthGate><p>Private controls</p></AuthGate>);
  expect(screen.queryByText("Private controls")).not.toBeInTheDocument();
  const input = await screen.findByLabelText("Setup code");
  fireEvent.change(input, { target: { value: "one-time-code" } });
  fireEvent.click(screen.getByRole("button", { name: "Create passkey" }));
  await waitFor(() => expect(registerPasskey).toHaveBeenCalledWith("one-time-code"));
  expect(await screen.findByText("Private controls")).toBeInTheDocument();
});
test("existing credentials require passkey login and errors keep controls hidden", async () => {
  vi.mocked(authRequest).mockResolvedValue({ registered: true, authenticated: false });
  vi.mocked(authenticate).mockRejectedValue(new Error("Passkey confirmation was cancelled"));
  render(<AuthGate><p>Private controls</p></AuthGate>);
  fireEvent.click(await screen.findByRole("button", { name: "Sign in" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("cancelled");
  expect(screen.queryByText("Private controls")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Setup code")).not.toBeInTheDocument();
});

test.each(["focus", "online", "plantool:session-changed"])("returns to sign-in when the session is lost on %s", async (event) => {
  vi.mocked(authRequest).mockResolvedValueOnce({ registered: true, authenticated: true }).mockResolvedValue({ registered: true, authenticated: false });
  render(<AuthGate><p>Private controls</p></AuthGate>);
  expect(await screen.findByText("Private controls")).toBeInTheDocument();
  fireEvent(window, new Event(event));
  expect(await screen.findByRole("button", { name: "Sign in" })).toBeInTheDocument();
  expect(screen.queryByText("Private controls")).not.toBeInTheDocument();
});

test("an authorization failure with a live session keeps the user signed in", async () => {
  vi.mocked(authRequest).mockResolvedValue({ registered: true, authenticated: true });
  render(<AuthGate><p>Private controls</p></AuthGate>);
  expect(await screen.findByText("Private controls")).toBeInTheDocument();
  fireEvent(window, new Event("plantool:session-changed"));
  await waitFor(() => expect(authRequest).toHaveBeenCalledTimes(2));
  expect(screen.getByText("Private controls")).toBeInTheDocument();
});
