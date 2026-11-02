import type { Comment } from "../types";
import { documentOrder, filterThreads, referenceNumerals, stepThread } from "./comments";

const base: Comment = {
  id: "a",
  doc: "plan",
  anchor: { line: 1, text: "", outdated: false },
  body: "",
  kind: "human",
  author: "me",
  parent: null,
  resolved: false,
  created_at: "",
  updated_at: null,
  seq: 1,
};
const at = (id: string, line: number, seq: number, over: Partial<Comment> = {}): Comment => ({
  ...base,
  id,
  seq,
  anchor: { ...base.anchor, line },
  ...over,
});

describe("comment order", () => {
  const list = [
    at("late", 30, 1),
    at("code", 2, 2, { anchor: { line: 2, text: "", outdated: false, code: { path: "a.rs", side: "new", line: 9, context: "" } } }),
    at("same-line-second", 10, 5),
    at("research", 50, 9, { doc: "research" }),
    at("same-line-first", 10, 3),
  ];

  it("sorts by document, then line, then creation", () => {
    expect(documentOrder(list).map((c) => c.id)).toEqual(["research", "same-line-first", "same-line-second", "late", "code"]);
  });

  it("n and p step through in document order and wrap", () => {
    const order = documentOrder(list.filter((c) => c.doc === "plan" && !c.anchor.code));
    expect(stepThread(order, null, 1)?.id).toBe("same-line-first");
    expect(stepThread(order, null, -1)?.id).toBe("late");
    expect(stepThread(order, "same-line-first", 1)?.id).toBe("same-line-second");
    expect(stepThread(order, "late", 1)?.id).toBe("same-line-first");
    expect(stepThread(order, "same-line-first", -1)?.id).toBe("late");
    expect(stepThread(order, "gone", 1)?.id).toBe("same-line-first");
    expect(stepThread([], null, 1)).toBeNull();
  });

  it("filters open, blockers, outdated and mine, leaving replies and resolved out", () => {
    const comments = [
      at("open", 1, 1, { kind: "agent", author: "council" }),
      at("blocker", 2, 2, { type: "blocker" }),
      at("agent-blocker", 3, 3, { type: "blocker", kind: "agent", author: "me" }),
      at("outdated", 4, 4, { anchor: { line: 4, text: "", outdated: true }, author: "you" }),
      at("orphan", 99, 5, { author: "you" }),
      at("resolved", 6, 6, { resolved: true }),
      at("reply", 7, 7, { parent: "open" }),
    ];
    const ids = (f: Parameters<typeof filterThreads>[1]) =>
      filterThreads(comments, f, "me", (c) => c.anchor.line > 50).map((c) => c.id);
    expect(ids("open")).toEqual(["open", "blocker", "agent-blocker", "outdated", "orphan"]);
    expect(ids("blockers")).toEqual(["blocker"]);
    expect(ids("outdated")).toEqual(["outdated", "orphan"]);
    expect(ids("mine")).toEqual(["blocker"]);
  });
});

describe("reference numerals", () => {
  const threads = [
    at("late", 30, 1),
    at("early", 3, 4),
    at("research", 50, 9, { doc: "research" }),
    at("done", 10, 5, { resolved: true }),
    at("reply", 3, 6, { parent: "early" }),
  ];

  it("numbers threads in document order with even numerals from 10", () => {
    expect([...referenceNumerals(threads)]).toEqual([["research", 10], ["early", 12], ["done", 14], ["late", 16]]);
  });

  it("keeps a thread's numeral when another thread resolves or a reply lands", () => {
    const before = referenceNumerals(threads);
    const after = referenceNumerals([
      ...threads.map((c) => (c.id === "early" ? { ...c, resolved: true } : c)),
      at("reply-2", 30, 10, { parent: "late" }),
    ]);
    expect(after).toEqual(before);
  });
});
