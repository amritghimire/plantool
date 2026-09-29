import { phaseOf, projectRun } from "./RunPanel";
import type { Run } from "../types";

const base: Run = { id: "r", provider: "claude", provider_session_id: "s", stage: "researching", cwd: "/r", status: "running", model: null, started_at: "", ended_at: null, error: null, seq: 0 };

describe("projectRun", () => {
  it("folds deltas, activities and permissions", () => {
    const { items, pending } = projectRun([
      { seq: 1, event: { type: "turn-started", turn_id: "t1" } },
      { seq: 2, event: { type: "text-delta", delta: "Hel" } },
      { seq: 3, event: { type: "text-delta", delta: "lo" } },
      { seq: 4, event: { type: "activity-start", id: "a1", kind: "tool", title: "Read plan.md" } },
      { seq: 5, event: { type: "activity-output", id: "a1", delta: "ok" } },
      { seq: 6, event: { type: "activity-complete", id: "a1", status: "completed" } },
      { seq: 7, event: { type: "permission", request_id: "p1", kind: "command", title: "Run cargo test" } },
      { seq: 8, event: { type: "message", id: "m1", role: "assistant", content: "Hello" } },
    ]);
    expect(items[0]).toMatchObject({ t: "msg", m: { role: "assistant", content: "Hello" } });
    expect(items[1]).toMatchObject({ t: "act", a: { id: "a1", output: "ok", status: "completed" } });
    expect(pending).toHaveLength(1);
    expect(pending[0].title).toBe("Run cargo test");
    expect(items.filter((i) => i.t === "msg")).toHaveLength(1);
    const after = projectRun([{ seq: 7, event: { type: "permission", request_id: "p1", kind: "command", title: "x" } }, { seq: 9, event: { type: "request-resolved", request_id: "p1" } }]);
    expect(after.pending).toHaveLength(0);
  });

  it("tells watching apart from working and idle from waiting", () => {
    const watching = projectRun([{ seq: 1, event: { type: "activity-start", id: "a", kind: "tool", title: "Bash Wait for the human", detail: '{"command":"plantool session watch --session x --since 3"}' } }]);
    expect(phaseOf(base, watching.items, watching.pending).kind).toBe("watching");
    const working = projectRun([{ seq: 1, event: { type: "activity-start", id: "a", kind: "tool", title: "Read plan.md" } }]);
    expect(phaseOf(base, working.items, working.pending)).toMatchObject({ kind: "working", title: "Working · Read plan.md" });
    expect(phaseOf({ ...base, status: "idle" }, [], []).kind).toBe("idle");
    expect(phaseOf({ ...base, status: "waiting" }, [], [{ request_id: "p", kind: "permission", title: "x" }]).kind).toBe("waiting");
    expect(phaseOf({ ...base, status: "stopped" }, [], []).detail).toMatch(/Resume/);
  });

  it("does not repeat streamed text when the final message arrives", () => {
    const { items } = projectRun([
      { seq: 1, event: { type: "text-delta", delta: "I'll start " } },
      { seq: 2, event: { type: "text-delta", delta: "by reading." } },
      { seq: 3, event: { type: "message", id: "m1:0", role: "assistant", content: "I'll start by reading." } },
      { seq: 4, event: { type: "activity-start", id: "a1", kind: "tool", title: "Bash" } },
      { seq: 5, event: { type: "text-delta", delta: "Next." } },
      { seq: 6, event: { type: "message", id: "m2:0", role: "assistant", content: "Next." } },
      { seq: 7, event: { type: "message", id: "u1", role: "user", content: "hi" } },
    ]);
    expect(items.map((i) => (i.t === "msg" ? `${i.m.role}:${i.m.content}` : i.t))).toEqual(["assistant:I'll start by reading.", "act", "assistant:Next.", "user:hi"]);
  });

  it("marks completed turns and leaves pending requests separate", () => {
    const result = projectRun([
      { seq: 1, at: "2026-01-01T00:00:00Z", event: { type: "turn-started", turn_id: "t" } },
      { seq: 2, event: { type: "activity-start", id: "a", kind: "tool", title: "Read files" } },
      { seq: 3, event: { type: "activity-complete", id: "a", status: "completed" } },
      { seq: 4, event: { type: "message", id: "answer", role: "assistant", content: "Finished" } },
      { seq: 5, at: "2026-01-01T00:00:04Z", event: { type: "turn-completed", turn_id: "t", status: "completed" } },
      { seq: 6, event: { type: "permission", request_id: "p", kind: "permission", title: "Approve" } },
    ]);
    expect(result.turns).toMatchObject([{ completed: true, duration: "4s", start: 0, end: 2 }]);
    expect(result.pending).toHaveLength(1);
  });
});
