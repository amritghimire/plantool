import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
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

it("keeps an unsent message when sending fails", async () => {
  const send = vi.spyOn(api, "runInput").mockRejectedValue(new Error("Connection lost"));
  render(<RunPanel sessionKey="repo/x" run={{ ...run, status: "running" }} lines={[]} onClose={() => {}} />);
  const input = screen.getByPlaceholderText(/Tell the agent something/);
  fireEvent.change(input, { target: { value: "Please check the tests" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(screen.getByText("Connection lost")).toBeVisible());
  expect(input).toHaveValue("Please check the tests");
  expect(send).toHaveBeenCalledWith("repo/x", "r", { text: "Please check the tests" });
  vi.restoreAllMocks();
});

it("exposes an activity disclosure to keyboard users", () => {
  const activityLines: RunLine[] = [
    { seq: 1, event: { type: "activity-start", id: "a", kind: "tool", title: "Inspect a long path", detail: "/repository/long/path/file.ts" } },
  ];
  render(<RunPanel sessionKey="repo/activity" run={{ ...run, status: "running" }} lines={activityLines} onClose={() => {}} />);
  const disclosure = screen.getByRole("button", { name: /Inspect a long path/ });
  disclosure.focus();
  expect(disclosure).toHaveFocus();
  expect(disclosure).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(disclosure);
  expect(disclosure).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("/repository/long/path/file.ts")).toBeVisible();
});

it("retains permission options and sends the chosen decision", async () => {
  const send = vi.spyOn(api, "runInput").mockResolvedValue({ ok: true });
  render(<RunPanel sessionKey="repo/permission" run={{ ...run, status: "waiting" }} lines={[{ seq: 1, event: { type: "permission", request_id: "p2", kind: "permission", title: "Allow this command?", options: [{ id: "allow-once", label: "Allow once" }, { id: "deny", label: "Deny" }] } }]} onClose={() => {}} />);
  expect(screen.getByLabelText("Run permissions")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Allow once" }));
  await waitFor(() => expect(send).toHaveBeenCalledWith("repo/permission", "r", { permission: { request_id: "p2", decision: "allow-once" } }));
});

it("captures the run receiving an attachment while preserving a new run draft", async () => {
  let resolve!: (value: { ok: boolean }) => void;
  const pending = new Promise<{ ok: boolean }>((yes) => { resolve = yes; });
  const send = vi.spyOn(api, "runInput").mockReturnValue(pending);
  vi.spyOn(api, "uploadAttachment").mockResolvedValue({ name: "notes.txt", path: "/session/attachments/notes.txt" });
  const { rerender } = render(<RunPanel sessionKey="repo/race" run={{ ...run, status: "running" }} lines={[]} onClose={() => {}} />);
  fireEvent.change(screen.getByLabelText("Attach files"), { target: { files: [new File(["notes"], "notes.txt")] } });
  await screen.findByText("Uploaded");
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  rerender(<RunPanel sessionKey="repo/race" run={{ ...run, id: "other", status: "running" }} lines={[]} onClose={() => {}} />);
  fireEvent.change(screen.getByLabelText("Message to agent"), { target: { value: "Other run draft" } });
  await act(async () => resolve({ ok: true }));
  expect(send).toHaveBeenCalledWith("repo/race", "r", { text: "Attached file: /session/attachments/notes.txt" });
  expect(screen.getByLabelText("Message to agent")).toHaveValue("Other run draft");
  expect(screen.queryByLabelText("Attachments")).toBeNull();
  vi.restoreAllMocks();
});
