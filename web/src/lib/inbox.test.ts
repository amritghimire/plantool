import type { Comment, DocKind, Run, SessionView, Stage } from "../types";
import { age, groupQueue, sessionAttention } from "./inbox";

const view = (stage: Stage, over: Partial<SessionView> = {}): SessionView => ({
  key: `repo/${stage}`,
  url_path: `/s/repo/${stage}`,
  dir: "/sessions",
  session: {
    repo_slug: "repo", slug: stage, title: stage, repo: { root: "/repo", common_dir: "/repo/.git", branch: "main" },
    worktree: null, base: "main", mirror: false, created_at: "2026-01-01T00:00:00Z",
  },
  state: { stage, comments: [], docs: {}, seq: 0, review: null, updated_at: "2026-01-02T00:00:00Z" },
  docs: [], runs: [], open_comments: 0,
  ...over,
});

const run = (status: Run["status"], over: Partial<Run> = {}): Run => ({
  id: `r-${status}`, provider: "claude", provider_session_id: null, stage: "implementing", cwd: "/repo",
  status, model: null, started_at: "2026-01-01T00:00:00Z", ended_at: null, error: null, seq: 1, ...over,
});

const doc = (kind: DocKind) => ({ kind, path: `${kind}.md`, exists: true, sha: "abc1234", lines: 10, title: null, captured_at: null, progress: [] });

const blocker: Comment = {
  id: "c1", doc: "plan", anchor: { line: 1, text: "", outdated: false }, body: "no", kind: "human", author: "me",
  parent: null, resolved: false, created_at: "", updated_at: null, seq: 1, type: "blocker",
};

const milestones = [
  { key: "m1", status: "approved", completed_tasks: [], run_id: null },
  { key: "m2", status: "approved", completed_tasks: [], run_id: null },
  { key: "m3", status: "pending", completed_tasks: [], run_id: null },
];

const pick = (v: SessionView) => {
  const a = sessionAttention(v);
  return [a.queue, a.status, a.reason];
};

it.each<[string, SessionView, string[]]>([
  ["new", view("new"), ["idle", "Not started", "Not started"]],
  ["researching, nothing written", view("researching"), ["idle", "Not started", "No research yet"]],
  ["researching, research written", view("researching", { docs: [doc("research")], open_comments: 2 }), ["waiting", "Waiting on you", "Research written · plan it · 2 open"]],
  ["researching, live run", view("researching", { runs: [run("running", { stage: "researching" })] }), ["running", "Agent running", "Researching"]],
  ["research-review", view("research-review", { open_comments: 3 }), ["waiting", "Waiting on you", "Review research · 3 open"]],
  ["planning, live run", view("planning", { runs: [run("running", { stage: "planning" })] }), ["running", "Agent running", "Planning"]],
  ["planning, no plan", view("planning"), ["idle", "Not started", "No plan yet"]],
  ["plan-review", view("plan-review", { docs: [doc("plan")], open_comments: 5 }), ["waiting", "Waiting on you", "Approve plan · 5 open"]],
  ["plan-review with a blocker", view("plan-review", { docs: [doc("plan")], open_comments: 1, state: { ...view("new").state, stage: "plan-review", comments: [blocker] } }), ["waiting", "Waiting on you", "Approve plan · 1 open · 1 blocker"]],
  ["plan-review, critique running", view("plan-review", { docs: [doc("plan")], runs: [run("running", { stage: "plan-review" })] }), ["waiting", "Waiting on you", "Approve plan"]],
  ["approved", view("approved"), ["waiting", "Waiting on you", "Plan approved · start build"]],
  ["implementing, live build", view("implementing", { runs: [run("running")], state: { ...view("new").state, stage: "implementing", milestones } }), ["running", "Agent running", "Build 3 of 3 running"]],
  ["implementing, agent idle", view("implementing", { runs: [run("idle")] }), ["waiting", "Waiting on you", "Agent finished its turn · reply"]],
  ["implementing, no run", view("implementing", { open_comments: 1 }), ["waiting", "Waiting on you", "Build paused · continue · 1 open"]],
  ["implementing, run stopped with an error", view("implementing", { runs: [run("stopped", { error: "Rate limited" })] }), ["waiting", "Recovery needed", "Agent stopped · resume"]],
  ["implementing, run stopped after sitting idle", view("implementing", { runs: [run("stopped", { idle_stopped: true })] }), ["waiting", "Waiting on you", "Build paused · continue"]],
  ["researching, run failed", view("researching", { runs: [run("failed", { stage: "researching" })] }), ["waiting", "Recovery needed", "Agent failed · resume"]],
  ["plan-review, last run stopped", view("plan-review", { docs: [doc("plan")], runs: [run("stopped", { stage: "planning" })] }), ["waiting", "Waiting on you", "Approve plan"]],
  ["agent question", view("implementing", { runs: [run("waiting")] }), ["waiting", "Agent needs your answer", "Agent needs your answer"]],
  ["agent question in a review stage", view("plan-review", { docs: [doc("plan")], runs: [run("waiting")] }), ["waiting", "Agent needs your answer", "Agent needs your answer"]],
  ["milestone ready", view("implementing", { runs: [run("idle", { task: "implement", implementation_mode: "step-by-step", milestone_pending: true, milestones_approved: 1 })], state: { ...view("new").state, stage: "implementing", milestones: milestones.slice(0, 2).map((m, i) => (i ? { ...m, status: "pending" } : m)) } }), ["waiting", "Waiting on you", "Milestone 2 of 2 ready to commit"]],
  ["milestone committing", view("implementing", { runs: [run("idle", { task: "implement", implementation_mode: "step-by-step", milestone_pending: true })], commit: { id: "c", scope: { kind: "milestone", run_id: "r-idle" }, phase: "hooks", started_at: "", lines: [] } }), ["running", "Agent running", "Committing milestone 1"]],
  ["implementation-review", view("implementation-review", { open_comments: 4 }), ["waiting", "Waiting on you", "Accept implementation · 4 open"]],
  ["done", view("done", { runs: [run("stopped", { stage: "done" })] }), ["done", "Done", "Done"]],
])("%s", (_name, v, expected) => {
  expect(pick(v)).toEqual(expected);
});

it("orders waiting oldest first and every other queue newest first", () => {
  const at = (stage: Stage, key: string, updated_at: string): SessionView => {
    const v = view(stage);
    return { ...v, key, state: { ...v.state, updated_at } };
  };
  const groups = groupQueue([
    at("implementation-review", "new-wait", "2026-03-01T00:00:00Z"),
    at("done", "old-done", "2026-01-01T00:00:00Z"),
    at("research-review", "old-wait", "2026-01-01T00:00:00Z"),
    at("done", "new-done", "2026-03-01T00:00:00Z"),
    at("new", "idle", "2026-02-01T00:00:00Z"),
  ]);
  expect(groups.waiting.map((v) => v.key)).toEqual(["old-wait", "new-wait"]);
  expect(groups.done.map((v) => v.key)).toEqual(["new-done", "old-done"]);
  expect(groups.idle.map((v) => v.key)).toEqual(["idle"]);
  expect(groups.running).toEqual([]);
});

it("prints compact ages", () => {
  const now = Date.parse("2026-01-10T00:00:00Z");
  expect(age("2026-01-10T00:00:00Z", now)).toBe("now");
  expect(age("2026-01-09T23:45:00Z", now)).toBe("15m");
  expect(age("2026-01-09T09:00:00Z", now)).toBe("15h");
  expect(age("2026-01-01T00:00:00Z", now)).toBe("9d");
});
