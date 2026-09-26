import { projectRun } from "./RunPanel";

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
});
