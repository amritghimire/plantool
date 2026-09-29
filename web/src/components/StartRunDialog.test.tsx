import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import type { Run, SessionView } from "../types";
import { StartRunDialog } from "./StartRunDialog";

const idle: Run = { id: "old", provider: "claude", provider_session_id: "ps", stage: "researching", task: "research", cwd: "/repo", status: "idle", model: null, started_at: "2026-01-01T00:00:00Z", ended_at: null, error: null, seq: 1 };
const view = { runs: [idle] } as SessionView;

beforeEach(() => {
  vi.spyOn(api, "providers").mockResolvedValue({ providers: [{ id: "claude", available: true, models: [] }] });
  vi.spyOn(api, "prompt").mockResolvedValue({ stage: "plan", prompt: "Plan this", session_stage: "research-review" });
});

afterEach(() => vi.restoreAllMocks());

it("waits for an idle run to end before starting the next run", async () => {
  const session = vi.spyOn(api, "session").mockResolvedValueOnce(view).mockResolvedValue({ ...view, runs: [{ ...idle, status: "stopped" }] });
  const stop = vi.spyOn(api, "stopRun").mockResolvedValue({ ok: true });
  const start = vi.spyOn(api, "startRun").mockResolvedValue({ run: { ...idle, id: "next", status: "starting" }, prompt: "Plan this" });
  const onStarted = vi.fn();
  render(<StartRunDialog stage="plan" sessionKey="repo/x" runs={[idle]} onClose={() => {}} onStarted={onStarted} />);
  expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
  fireEvent.click(screen.getByLabelText("Stop and start"));
  fireEvent.click(screen.getByRole("button", { name: "Start" }));
  await waitFor(() => expect(onStarted).toHaveBeenCalledWith("next"));
  expect(stop).toHaveBeenCalledWith("repo/x", "old");
  expect(session).toHaveBeenCalledTimes(3);
  expect(start.mock.invocationCallOrder[0]).toBeGreaterThan(stop.mock.invocationCallOrder[0]);
});

it("keeps the dialog open when stopping the old run fails", async () => {
  vi.spyOn(api, "session").mockResolvedValue(view);
  vi.spyOn(api, "stopRun").mockRejectedValue(new Error("stop failed"));
  const start = vi.spyOn(api, "startRun");
  render(<StartRunDialog stage="plan" sessionKey="repo/x" runs={[idle]} onClose={() => {}} onStarted={() => {}} />);
  fireEvent.click(screen.getByLabelText("Stop and start"));
  fireEvent.click(screen.getByRole("button", { name: "Start" }));
  expect(await screen.findByText("stop failed")).toBeInTheDocument();
  expect(start).not.toHaveBeenCalled();
});

it("can keep an idle run and start a separate run", async () => {
  vi.spyOn(api, "session").mockResolvedValue(view);
  const stop = vi.spyOn(api, "stopRun");
  const start = vi.spyOn(api, "startRun").mockResolvedValue({ run: { ...idle, id: "next" }, prompt: "Plan this" });
  const close = vi.fn();
  render(<StartRunDialog stage="plan" sessionKey="repo/x" runs={[idle]} onClose={close} onStarted={() => {}} />);
  fireEvent.click(screen.getByLabelText("Keep running and start"));
  fireEvent.click(screen.getByRole("button", { name: "Start" }));
  await waitFor(() => expect(start).toHaveBeenCalledOnce());
  expect(stop).not.toHaveBeenCalled();
  expect(close).toHaveBeenCalled();
});

it("refreshes stale run status before starting", async () => {
  vi.spyOn(api, "session").mockResolvedValue(view);
  const start = vi.spyOn(api, "startRun");
  render(<StartRunDialog stage="plan" sessionKey="repo/x" runs={[]} onClose={() => {}} onStarted={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "Start" }));
  expect(await screen.findByText("Choose what to do with the live run before starting another.")).toBeInTheDocument();
  expect(start).not.toHaveBeenCalled();
});
