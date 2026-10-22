import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import { ExecutionControls } from "./ExecutionControls";
afterEach(() => vi.restoreAllMocks());
it("stops immediately and requires a separate resume action", async () => {
  vi.spyOn(api, "executionStatus").mockResolvedValue({ execution: { stopped: false, generation: 0 }, sandbox_supported: true });
  const stop = vi.spyOn(api, "stopAll").mockResolvedValue({ execution: { stopped: true } });
  const resume = vi.spyOn(api, "resumeExecution").mockRejectedValue(new Error("Passkey cancelled"));
  render(<ExecutionControls />);
  await waitFor(() => expect(api.executionStatus).toHaveBeenCalled());
  fireEvent.click(screen.getByRole("button", { name: "Stop All" }));
  const button = await screen.findByRole("button", { name: "Resume execution with passkey" });
  expect(stop).toHaveBeenCalledOnce();
  expect(resume).not.toHaveBeenCalled();
  fireEvent.click(button);
  expect(await screen.findByRole("alert")).toHaveTextContent("Passkey cancelled");
  expect(screen.getByRole("status")).toHaveTextContent("Execution stopped");
});
