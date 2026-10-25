import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import { RecoveryControls } from "./RecoveryControls";
it("retries only the displayed attempt and refreshes after the bound action", async () => {
  vi.spyOn(api, "recovery").mockResolvedValue({
    eligible: true,
    attempt_id: "a1",
    step: "triage",
    message: "Failed read-only step",
  });
  const retry = vi
    .spyOn(api, "retryEngineering")
    .mockResolvedValue({ queued: true });
  const refresh = vi.fn();
  render(<RecoveryControls item="i" onRecovered={refresh} />);
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Retry failed step with passkey",
    }),
  );
  await waitFor(() => expect(retry).toHaveBeenCalledWith("i", "a1"));
  expect(refresh).toHaveBeenCalled();
});
it("does not offer retry for unsafe coding recovery", async () => {
  vi.spyOn(api, "recovery").mockResolvedValue({
    eligible: false,
    attempt_id: "a1",
    step: "backend",
    message: "Reconcile uncertain effects manually",
  });
  render(<RecoveryControls item="i" onRecovered={() => {}} />);
  await screen.findByText("Reconcile uncertain effects manually");
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});
it("shows current route exclusions and avoids a retry when no route is eligible",async()=>{
 vi.spyOn(api,"recovery").mockResolvedValue({eligible:true,attempt_id:"a1",step:"triage",message:"Read-only recovery",routing_error:"codex-sol: provider openai is disallowed by project policy"});
 render(<RecoveryControls item="i" onRecovered={()=>{}}/>);
 await screen.findByText(/No eligible model route now/);
 expect(screen.getByRole("button",{name:"Retry failed step with passkey"})).toBeDisabled();
});
