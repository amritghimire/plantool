import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import type { EngineeringAttempt } from "../engineering";
import { AttemptTranscript } from "./AttemptTranscript";

const attempt: EngineeringAttempt = {
  id: "a1", role: "backend", generation: 2, state: "failed", route: "codex-sol",
  started_at: "2026-10-28T00:00:00Z", ended_at: "2026-10-28T00:01:00Z",
  summary: "The provider could not finish", provider: "openai", model: "gpt-6.1-sol",
  tokens_in: null, tokens_out: null, duration_ms: 60_000,
};

it("loads and renders the persisted AI transcript on demand", async () => {
  const transcript = vi.spyOn(api, "engineeringTranscript").mockResolvedValue({
    attempt: "a1",
    events: [
      { seq: 1, event: { type: "message", id: "m1", role: "assistant", content: "I found the failed check." } },
      { seq: 2, event: { type: "activity-start", id: "t1", kind: "tool", title: "Inspect test output" } },
      { seq: 3, event: { type: "activity-complete", id: "t1", status: "failed", output: "check.sh exited 1" } },
    ],
  });
  render(<AttemptTranscript item="p:1" attempt={attempt} />);
  fireEvent.click(screen.getByText("View AI transcript"));
  expect(await screen.findByText("I found the failed check.")).toBeInTheDocument();
  expect(await screen.findByText("Inspect test output · failed")).toBeInTheDocument();
  expect(screen.getByText("check.sh exited 1")).toBeInTheDocument();
  expect(transcript).toHaveBeenCalledWith("p:1", "a1");
});

it("refreshes an open transcript while its attempt is running", async () => {
  const liveAttempt = { ...attempt, state: "running" };
  const transcript = vi.spyOn(api, "engineeringTranscript")
    .mockResolvedValueOnce({ attempt: "a1", events: [] })
    .mockResolvedValueOnce({ attempt: "a1", events: [
      { seq: 1, event: { type: "message", id: "m2", role: "assistant", content: "The plan fields were incomplete." } },
    ] })
    .mockResolvedValue({ attempt: "a1", events: [
      { seq: 1, event: { type: "message", id: "m2", role: "assistant", content: "The plan fields were incomplete." } },
      { seq: 2, event: { type: "message", id: "m3", role: "assistant", content: "I am correcting the plan now." } },
    ] });
  render(<AttemptTranscript item="p:1" attempt={liveAttempt} />);
  fireEvent.click(screen.getByText("View AI transcript"));
  expect(await screen.findByText(/No provider events have arrived yet/)).toBeInTheDocument();
  await waitFor(() => expect(screen.getByText("The plan fields were incomplete.")).toBeInTheDocument(), { timeout: 4000 });
  await waitFor(() => expect(screen.getByText("I am correcting the plan now.")).toBeInTheDocument(), { timeout: 4000 });
  expect(transcript.mock.calls.length).toBeGreaterThanOrEqual(3);
});

it("explains when a completed historical attempt has no retained transcript", async () => {
  vi.spyOn(api, "engineeringTranscript").mockResolvedValue({ attempt: "a1", events: [] });
  render(<AttemptTranscript item="p:1" attempt={attempt} />);
  fireEvent.click(screen.getByText("View AI transcript"));
  expect(await screen.findByText(/No provider events were retained for this attempt/)).toBeInTheDocument();
});
