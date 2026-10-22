import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi, beforeEach, test, expect } from "vitest";
import { AuthGate } from "./AuthGate";
import { authenticate, authRequest, registerPasskey } from "../auth";
vi.mock("../auth", () => ({ authenticate: vi.fn(), authRequest: vi.fn(), registerPasskey: vi.fn() }));
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
