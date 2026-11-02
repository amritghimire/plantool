import type { Comment, DocSummary, Run, SessionView, Stage } from "../types";
import { noticeFor, resumeStageFor, stepOf, type Notice } from "./notice";

const now = "2026-11-02T10:00:00Z";

function doc(kind: "research" | "plan", exists: boolean, sha = "4f2c19a8e1", progress: DocSummary["progress"] = []): DocSummary {
  return { kind, path: `/d/${kind}.md`, exists, sha: exists ? sha : null, lines: 10, title: null, captured_at: null, progress };
}

function run(over: Partial<Run> = {}): Run {
  return { id: "r1", provider: "claude", provider_session_id: "p1", stage: "implementing", task: "implement", cwd: "/r", status: "running", model: null, started_at: now, ended_at: null, error: null, seq: 1, ...over };
}

function view(stage: Stage, over: Partial<SessionView> = {}): SessionView {
  return {
    key: "repo/x",
    url_path: "/s/repo/x",
    dir: "/d",
    session: { repo_slug: "repo", slug: "x", title: "x", repo: { root: "/r", common_dir: "/r/.git", branch: "main" }, worktree: null, base: "main", mirror: false, created_at: now, brief: "Do it" },
    state: { stage, comments: [], docs: {}, seq: 1, review: null, updated_at: now },
    docs: [doc("research", true), doc("plan", true, "4f2c19a8e1", [{ name: "Phase 1", done: 2, total: 5 }])],
    runs: [],
    open_comments: 0,
    ...over,
  };
}

function comment(over: Partial<Comment> = {}): Comment {
  return { id: "c1", doc: "plan", anchor: { line: 3, text: "", outdated: false }, body: "b", kind: "human", author: "me", parent: null, resolved: false, created_at: now, updated_at: null, seq: 1, ...over };
}

const ctx = { comments: [] as Comment[], activeTab: "plan" };
const ids = (n: Notice, key: "alternates" | "secondary") => n[key].map((a) => a.id);

describe("noticeFor", () => {
  it("new: starts research and offers both skips", () => {
    const n = noticeFor(view("new", { docs: [doc("research", false), doc("plan", false)] }), { ...ctx, activeTab: "research" });
    expect(n.form).toBe("Research");
    expect(n.primary?.id).toBe("start-research");
    expect(ids(n, "alternates")).toEqual([]);
    expect(ids(n, "secondary")).toEqual(["skip-to-plan", "skip-to-build", "ask"]);
  });

  it("new without a brief asks for one", () => {
    const v = view("new");
    const n = noticeFor({ ...v, session: { ...v.session, brief: null } }, ctx);
    expect(n.note).toMatch(/Add a brief/);
  });

  it("researching with a live run has no primary and links the run", () => {
    const live = run({ stage: "researching", task: "research" });
    const n = noticeFor(view("researching", { runs: [live] }), ctx);
    expect(n.heading).toBe("The agent is researching");
    expect(n.primary).toBeNull();
    expect(n.alternates[0]).toMatchObject({ id: "open-run", run: live });
  });

  it("a waiting run makes answering the primary", () => {
    const n = noticeFor(view("planning", { runs: [run({ status: "waiting", stage: "planning", task: "plan" })] }), ctx);
    expect(n.primary).toMatchObject({ id: "open-run", label: "Answer the agent" });
  });

  it("researching idle without research starts research", () => {
    const n = noticeFor(view("researching", { docs: [doc("research", false), doc("plan", false)] }), ctx);
    expect(n.primary?.id).toBe("start-research");
  });

  it("research-review plans it and shows the evidence", () => {
    const n = noticeFor(view("research-review"), { comments: [comment({ doc: "research" })], activeTab: "research" });
    expect(n.heading).toBe("Your turn: review the research");
    expect(n.primary).toMatchObject({ id: "start-plan", label: "Plan it" });
    expect(n.evidence).toContainEqual({ label: "Status", value: "Open: 1 · Blockers: 0" });
    expect(n.evidence).toContainEqual({ label: "Document", value: "research.md rev 4f2c19a" });
    expect(ids(n, "secondary")).toEqual(["critique", "ask"]);
    expect(ids(n, "alternates")).toContain("skip-to-build");
  });

  it("planning without a plan starts planning", () => {
    const n = noticeFor(view("planning", { docs: [doc("research", true), doc("plan", false)] }), ctx);
    expect(n.primary).toMatchObject({ id: "start-plan", label: "Start planning" });
  });

  it("plan-review issues the notice of allowance", () => {
    const n = noticeFor(view("plan-review"), { ...ctx, planSha: "abcdef12345" });
    expect(n.form).toBe("Notice of allowance");
    expect(n.heading).toBe("Your turn: approve the plan");
    expect(n.primary).toMatchObject({ id: "approve-plan", label: "Approve plan" });
    expect(n.blockedBy).toBeUndefined();
    expect(n.evidence).toContainEqual({ label: "Application", value: "plan.md rev abcdef1" });
    expect(ids(n, "secondary")).toEqual(["revise-plan", "critique", "critique", "critique", "critique", "ask"]);
    expect(n.secondary.filter((a) => a.prompt).map((a) => a.label)).toEqual(["What could go wrong?", "What was assumed?", "What is missing?"]);
  });

  it("plan-review with open blockers needs a recorded override and points at the blocker", () => {
    const blocker = comment({ id: "b1", type: "blocker" });
    const n = noticeFor(view("plan-review"), { comments: [blocker, comment({ id: "a1", kind: "agent", type: "blocker" })], activeTab: "plan" });
    expect(n.primary).toMatchObject({ id: "approve-override" });
    expect(n.blockedBy).toEqual({ text: "1 blocker stops approval", comment: blocker });
    expect(n.evidence).toContainEqual({ label: "Status", value: "Open: 2 · Blockers: 1" });
  });

  it("plan-review keeps approval while a critique runs", () => {
    const n = noticeFor(view("plan-review", { runs: [run({ stage: "plan-review", task: "critique" })] }), ctx);
    expect(n.primary?.id).toBe("approve-plan");
    expect(ids(n, "alternates")).toContain("open-run");
  });

  it("approved starts the build either way", () => {
    const v = view("approved");
    const n = noticeFor({ ...v, state: { ...v.state, approved: { sha: "9999999aaa", at: now, override_reason: null, open_blockers: [] } } }, ctx);
    expect(n.primary).toMatchObject({ id: "start-build" });
    expect(n.alternates[0]).toMatchObject({ id: "start-build-steps" });
    expect(n.evidence).toContainEqual({ label: "Allowed", value: "plan.md rev 9999999" });
    expect(n.evidence).toContainEqual({ label: "Tasks", value: "2 / 5 done" });
  });

  it("approved without a plan explains the task list", () => {
    const n = noticeFor(view("approved", { docs: [doc("research", false), doc("plan", false)] }), ctx);
    expect(n.note).toMatch(/No plan/);
  });

  it("implementing live: agent working, open run", () => {
    const v = view("implementing", { runs: [run()] });
    const n = noticeFor({ ...v, state: { ...v.state, milestones: [{ key: "a", status: "approved", completed_tasks: [], run_id: null }, { key: "b", status: "pending", completed_tasks: [], run_id: null }, { key: "c", status: "pending", completed_tasks: [], run_id: null }] } }, { ...ctx, activeTab: "changes" });
    expect(n.form).toBe("Build 2 of 3");
    expect(n.heading).toBe("The agent is building");
    expect(n.primary).toBeNull();
    expect(n.alternates[0].id).toBe("open-run");
  });

  it("implementing with a stopped run needs recovery", () => {
    const stopped = run({ status: "stopped", error: "Rate limited" });
    const n = noticeFor(view("implementing", { runs: [stopped] }), ctx);
    expect(n.heading).toBe("Recovery needed: the run stopped");
    expect(n.primary).toMatchObject({ id: "resume-run", run: stopped });
    expect(n.evidence).toContainEqual({ label: "Reason", value: "Rate limited" });
    expect(n.alternates.map((a) => a.id)).toEqual(expect.arrayContaining(["review-changes", "finish-build"]));
  });

  it("an idle-stopped run with every task ticked offers finishing the build", () => {
    const v = view("implementing", { runs: [run({ status: "stopped", idle_stopped: true })], docs: [doc("plan", true, "4f2c19a8e1", [{ name: "Phase 1", done: 3, total: 3 }])] });
    const n = noticeFor(v, ctx);
    expect(n.heading).toBe("Every task is ticked: review the build");
    expect(n.primary?.id).toBe("finish-build");
    expect(n.alternates.map((a) => a.id)).toEqual(expect.arrayContaining(["resume-run", "continue-build", "review-changes"]));
  });

  it("a paused build can be finished without the agent", () => {
    const n = noticeFor(view("implementing"), ctx);
    expect(n.alternates.map((a) => a.id)).toContain("finish-build");
  });

  it("a failed planning run offers no finish-build", () => {
    const n = noticeFor(view("planning", { runs: [run({ status: "failed", stage: "planning", task: "plan" })] }), ctx);
    expect(n.alternates.map((a) => a.id)).not.toContain("finish-build");
  });

  it("a failed run needs recovery outside done", () => {
    const n = noticeFor(view("planning", { runs: [run({ status: "failed", stage: "planning", task: "plan" })] }), ctx);
    expect(n.primary?.id).toBe("resume-run");
    expect(noticeFor(view("done", { runs: [run({ status: "failed" })] }), ctx).primary).toBeNull();
  });

  it("implementing idle without milestones replies to the agent", () => {
    const n = noticeFor(view("implementing", { runs: [run({ status: "idle" })] }), ctx);
    expect(n.primary).toMatchObject({ id: "open-run", label: "Reply to the agent" });
    expect(ids(n, "alternates")).not.toContain("open-run");
  });

  it("implementing with nothing live continues the build", () => {
    const n = noticeFor(view("implementing"), ctx);
    expect(n.primary?.id).toBe("continue-build");
    expect(ids(n, "alternates")).toContain("review-changes");
  });

  it("milestone review approves the milestone", () => {
    const m = run({ status: "idle", implementation_mode: "step-by-step", milestone_pending: true, milestones_approved: 1 });
    const n = noticeFor(view("implementing", { runs: [m] }), ctx);
    expect(n.form).toBe("Milestone review");
    expect(n.heading).toBe("Milestone 2 is ready");
    expect(n.primary).toMatchObject({ id: "approve-milestone", run: m });
    expect(n.milestone).toEqual({ run: m, committing: false });
  });

  it("a pending plan revision takes over the milestone primary", () => {
    const m = run({ status: "idle", implementation_mode: "step-by-step", milestone_pending: true });
    const v = view("implementing", { runs: [m] });
    const n = noticeFor({ ...v, state: { ...v.state, plan_revision_pending: true } }, ctx);
    expect(n.primary).toEqual({ id: "accept-revision", label: "Accept plan revision" });
    expect(n.heading).toBe("Your turn: accept the plan revision");
    expect(n.note).toBe("Plan scope changed since approval. Accept the revision, then commit milestone 1 and continue.");
    expect(n.evidence).toContainEqual({ label: "Revision", value: "plan.md rev 4f2c19a" });
  });

  it("a pending plan revision in review makes accepting it the approval", () => {
    const v = view("plan-review");
    const n = noticeFor({ ...v, state: { ...v.state, plan_revision_pending: true } }, ctx);
    expect(n.primary).toEqual({ id: "accept-revision", label: "Accept plan revision" });
    expect(n.note).toBe("Plan scope changed since approval. Accepting the revision approves this plan.");
    expect(n.evidence.filter((f) => f.label === "Revision")).toEqual([]);
  });

  it("a pending plan revision with blockers keeps the override", () => {
    const v = view("plan-review");
    const n = noticeFor({ ...v, state: { ...v.state, plan_revision_pending: true } }, { ...ctx, comments: [comment({ type: "blocker" })] });
    expect(n.primary?.id).toBe("approve-override");
    expect(n.note).toMatch(/recorded override/);
  });

  it("a pending plan revision leaves a working agent alone", () => {
    const v = view("implementing", { runs: [run()] });
    const n = noticeFor({ ...v, state: { ...v.state, plan_revision_pending: true } }, ctx);
    expect(n.primary).toBeNull();
  });

  it("milestone commit in progress has no primary", () => {
    const m = run({ status: "idle", implementation_mode: "step-by-step", milestone_pending: true });
    const n = noticeFor(view("implementing", { runs: [m], commit: { id: "c", scope: { kind: "milestone", run_id: m.id }, phase: "hooks", started_at: now, lines: [] } }), ctx);
    expect(n.primary).toBeNull();
    expect(n.milestone?.committing).toBe(true);
  });

  it("a stopped unscoped milestone resumes the review", () => {
    const m = run({ status: "stopped", implementation_mode: "step-by-step", milestone_pending: true });
    expect(noticeFor(view("implementing", { runs: [m] }), ctx).primary).toMatchObject({ id: "resume-milestone", run: m });
  });

  it("a stopped scoped milestone can still be approved", () => {
    const m = run({ status: "stopped", implementation_mode: "all-at-once", milestone_key: "Phase 1", milestone_pending: true });
    expect(noticeFor(view("implementing", { runs: [m] }), ctx).primary?.id).toBe("approve-milestone");
  });

  it("implementation-review accepts and offers a PR", () => {
    const n = noticeFor(view("implementation-review"), { ...ctx, activeTab: "changes" });
    expect(n.form).toBe("Final review");
    expect(n.primary).toMatchObject({ id: "accept", label: "Accept and finish" });
    expect(n.alternates[0].id).toBe("review-changes");
    expect(ids(n, "secondary")).toEqual(["ask", "create-pr"]);
  });

  it("done shows what shipped and the PR", () => {
    const v = view("done");
    const n = noticeFor({ ...v, session: { ...v.session, pull_request: { number: 23, url: "https://x/23", state: "MERGED", draft: false, updated_at: now } } }, ctx);
    expect(n.form).toBe("Complete");
    expect(n.primary).toBeNull();
    expect(n.evidence).toContainEqual({ label: "Pull request", value: "#23 · merged", href: "https://x/23" });
    expect(n.evidence).toContainEqual({ label: "Shipped", value: "2 / 5 tasks" });
    expect(ids(n, "secondary")).not.toContain("create-pr");
  });

  it("surfaces checkpoints and workspace proposals", () => {
    const v = view("implementing");
    const n = noticeFor({ ...v, state: { ...v.state, pause_reason: "Public API change", proposals: [{ id: "p", kind: "base", old: "main", new: "dev", reason: "r" }] } }, ctx);
    expect(n.evidence[0]).toEqual({ label: "Checkpoint", value: "Public API change" });
    expect(ids(n, "alternates")).toContain("open-details");
  });
});

it("resumes a run with the stage it was doing", () => {
  expect(resumeStageFor(run({ task: "critique" }))).toBe("critique");
  expect(resumeStageFor(run({ task: undefined, stage: "researching" }))).toBe("research");
  expect(resumeStageFor(run({ task: undefined, stage: "implementation-review" }))).toBe("implement");
  expect(resumeStageFor(run({ task: undefined, stage: "plan-review" }))).toBe("plan");
});

it("falls back to the stage when the daemon sends no step", () => {
  expect(stepOf(view("approved"))).toBe("Build");
  expect(stepOf({ ...view("approved"), step: "Review" })).toBe("Review");
});

describe("first-run guide", () => {
  it("explains the hosted run and the copy-prompt path before research exists", () => {
    for (const stage of ["new", "researching"] as const) {
      const n = noticeFor(view(stage, { docs: [doc("research", false), doc("plan", false)] }), { ...ctx, activeTab: "research" });
      expect(n.primary?.id).toBe("start-research");
      expect(n.guide).toEqual(["Start research runs a hosted agent from this page.", "Or copy the prompt into any agent in your terminal."]);
      expect(n.copyPrompt).toBe(true);
      expect(ids(n, "secondary")).toContain("skip-to-build");
      expect(ids(n, "alternates")).not.toContain("skip-to-build");
    }
  });

  it("drops the guide once research is written", () => {
    const n = noticeFor(view("researching", { docs: [doc("research", true), doc("plan", false)] }), ctx);
    expect(n.guide).toBeUndefined();
    expect(n.copyPrompt).toBe(false);
  });
});
