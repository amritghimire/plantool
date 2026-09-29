import { fireEvent, render, screen } from "@testing-library/react";
import { RunPanel, type RunLine } from "./RunPanel";
import type { Run } from "../types";

const run: Run = { id: "r", provider: "claude", provider_session_id: "ps", stage: "implementing", task: "implement", cwd: "/r", status: "stopped", model: null, started_at: "2026-01-01T00:00:00Z", ended_at: "2026-01-01T00:00:10Z", error: null, seq: 8 };
const lines: RunLine[] = [
  { seq: 1, event: { type: "turn-started", turn_id: "t" } },
  { seq: 2, event: { type: "activity-start", id: "a", kind: "tool", title: "Read plan" } },
  { seq: 3, event: { type: "activity-complete", id: "a", status: "completed" } },
  { seq: 4, event: { type: "activity-start", id: "b", kind: "tool", title: "Failed check" } },
  { seq: 5, event: { type: "activity-complete", id: "b", status: "failed" } },
  { seq: 6, event: { type: "message", id: "answer", role: "assistant", content: "The answer is ready." } },
  { seq: 7, event: { type: "turn-completed", turn_id: "t", status: "completed" } },
  { seq: 8, event: { type: "permission", request_id: "p", kind: "permission", title: "Needs approval" } },
];

it("folds completed work while keeping the answer, failure and request visible", () => {
  render(<RunPanel sessionKey="repo/x" run={run} lines={lines} onClose={() => {}} />);
  const summary = screen.getByText(/Finished turn/);
  expect(summary.closest("details")).not.toHaveAttribute("open");
  expect(screen.getByText("The answer is ready.")).toBeVisible();
  expect(screen.getByText("Failed check")).toBeVisible();
  expect(screen.getByText("Needs approval")).toBeVisible();
  fireEvent.click(summary);
  expect(summary.closest("details")).toHaveAttribute("open");
  expect(screen.getByText("Read plan")).toBeVisible();
});

it("keeps live turn work in the timeline", () => {
  render(<RunPanel sessionKey="repo/x" run={{ ...run, status: "running" }} lines={lines.slice(0, 4)} onClose={() => {}} />);
  expect(screen.queryByText(/Finished turn/)).toBeNull();
  expect(screen.getByText("Read plan")).toBeVisible();
});
