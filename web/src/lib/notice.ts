import type { Comment, DocKind, Run, SessionView, Stage } from "../types";
import type { RunStage } from "../components/StartRunDialog";

export type NoticeActionId =
  | "start-research"
  | "skip-to-plan"
  | "skip-to-build"
  | "start-plan"
  | "approve-plan"
  | "approve-override"
  | "start-build"
  | "start-build-steps"
  | "continue-build"
  | "approve-milestone"
  | "resume-milestone"
  | "review-changes"
  | "accept"
  | "open-run"
  | "resume-run"
  | "open-details"
  | "revise-plan"
  | "critique"
  | "ask"
  | "create-pr"
  | "accept-revision"
  | "finish-build";

export interface NoticeAction {
  id: NoticeActionId;
  label: string;
  disabled?: string;
  run?: Run;
  prompt?: string;
}

const FINISH_BUILD: NoticeAction = { id: "finish-build", label: "Finish the build and review" };

export interface NoticeField {
  label: string;
  value: string;
  href?: string;
}

export interface Notice {
  form: string;
  heading: string;
  evidence: NoticeField[];
  note?: string;
  guide?: string[];
  copyPrompt?: boolean;
  primary: NoticeAction | null;
  blockedBy?: { text: string; comment: Comment };
  alternates: NoticeAction[];
  secondary: NoticeAction[];
  milestone?: { run: Run; committing: boolean };
}

export interface NoticeContext {
  comments: Comment[];
  activeTab: string;
  planSha?: string | null;
}

export const CRITIQUE_PRESETS = [
  "What could go wrong?",
  "What was assumed?",
  "What is missing?",
];

export function critiquePrompt(question: string): string {
  return `Review the plan with this question: ${question} Check claims against the code and leave focused comments with risks and open questions.`;
}

export const RUN_WORD: Record<Run["status"], string> = {
  starting: "starting",
  running: "working",
  waiting: "needs input",
  idle: "your move",
  stopped: "stopped",
  failed: "failed",
};

export function isLive(r: Run): boolean {
  return (
    r.status === "starting" ||
    r.status === "running" ||
    r.status === "waiting" ||
    r.status === "idle"
  );
}

export function resumeStageFor(r: Run): RunStage {
  switch (r.task) {
    case "assist":
    case "draft-pr":
    case "critique":
    case "research":
    case "plan":
    case "implement":
      return r.task;
  }
  return r.stage === "new" || r.stage === "researching"
    ? "research"
    : r.stage === "approved" ||
        r.stage === "implementing" ||
        r.stage === "implementation-review"
      ? "implement"
      : "plan";
}

export function runAnyStage(stage: Stage): RunStage {
  return stage === "new" ||
    stage === "researching" ||
    stage === "research-review"
    ? "research"
    : stage === "approved" ||
        stage === "implementing" ||
        stage === "implementation-review"
      ? "implement"
      : "plan";
}

export const STEPS = ["Research", "Plan", "Build", "Review", "Done"] as const;

export function stepOf(view: SessionView): (typeof STEPS)[number] {
  const named = STEPS.find((s) => s === view.step);
  if (named) return named;
  const stage = view.state.stage;
  return stage === "done"
    ? "Done"
    : stage === "implementation-review"
      ? "Review"
      : stage === "approved" || stage === "implementing"
        ? "Build"
        : stage === "planning" || stage === "plan-review"
          ? "Plan"
          : "Research";
}

export function planProgress(view: SessionView): { done: number; total: number } {
  const phases = view.docs.find((d) => d.kind === "plan")?.progress ?? [];
  return {
    done: phases.reduce((n, p) => n + p.done, 0),
    total: phases.reduce((n, p) => n + p.total, 0),
  };
}

export function openThreads(comments: Comment[]): Comment[] {
  return comments
    .filter((c) => !c.parent && !c.resolved)
    .sort((a, b) => a.doc.localeCompare(b.doc) || a.anchor.line - b.anchor.line);
}

export function openBlockers(comments: Comment[]): Comment[] {
  return openThreads(comments).filter(
    (c) => c.kind === "human" && c.type === "blocker",
  );
}

export function latestImplementation(view: SessionView): Run | undefined {
  return view.runs
    .filter((r) => r.task === "implement")
    .sort((a, b) => b.started_at.localeCompare(a.started_at))[0];
}

export function milestoneReview(view: SessionView): {
  run: Run;
  reviewable: boolean;
  committing: boolean;
} | null {
  const run = latestImplementation(view);
  if (view.state.stage !== "implementing" || !run?.milestone_pending)
    return null;
  const stepMode =
    run.implementation_mode === "step-by-step" || !!run.milestone_key;
  if (!stepMode) return null;
  const reviewable =
    run.status === "idle" || (!!run.milestone_key && run.status === "stopped");
  const committing =
    view.commit?.scope.kind === "milestone" &&
    view.commit.scope.run_id === run.id;
  return { run, reviewable, committing };
}

export function buildPosition(view: SessionView): { k: number; n: number } | null {
  const ms = view.state.milestones ?? [];
  if (!ms.length) return null;
  const finished = ms.filter(
    (m) => m.status === "approved" || m.status === "completed",
  ).length;
  return { k: Math.min(finished + 1, ms.length), n: ms.length };
}

const short = (sha: string | null | undefined) => (sha ? sha.slice(0, 7) : "");
const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

function docField(
  label: string,
  kind: DocKind,
  view: SessionView,
  sha?: string | null,
): NoticeField {
  const doc = view.docs.find((d) => d.kind === kind);
  const rev = sha ?? doc?.sha;
  return {
    label,
    value: doc?.exists && rev ? `${kind}.md rev ${short(rev)}` : `${kind}.md not written`,
  };
}

export function liveRun(view: SessionView): Run | undefined {
  return [...view.runs]
    .filter(isLive)
    .sort((a, b) => b.started_at.localeCompare(a.started_at))[0];
}

export function recoveringRun(view: SessionView): Run | undefined {
  const stage = view.state.stage;
  const latest = [...view.runs].sort((a, b) =>
    b.started_at.localeCompare(a.started_at),
  )[0];
  if (!latest || liveRun(view) || latest.milestone_pending) return undefined;
  const stuck =
    (latest.status === "failed" && stage !== "done") ||
    (latest.status === "stopped" &&
      !!latest.error &&
      !latest.idle_stopped &&
      (stage === "researching" || stage === "planning" || stage === "implementing"));
  return stuck ? latest : undefined;
}

export function pausedRun(view: SessionView): Run | undefined {
  const latest = [...view.runs].sort((a, b) =>
    b.started_at.localeCompare(a.started_at),
  )[0];
  if (!latest || liveRun(view) || latest.milestone_pending) return undefined;
  return latest.status === "stopped" && !recoveringRun(view) ? latest : undefined;
}

const FIRST_RUN_GUIDE = [
  "Start research runs a hosted agent from this page.",
  "Or copy the prompt into any agent in your terminal.",
];

const AGENT_STAGES: Stage[] = [
  "new",
  "researching",
  "planning",
  "approved",
  "implementing",
];

const REVISION_GATED = new Set<NoticeActionId>([
  "approve-plan",
  "approve-milestone",
  "start-build",
  "continue-build",
]);

export function noticeFor(view: SessionView, ctx: NoticeContext): Notice {
  const n = stageNotice(view, ctx);
  const plan = view.docs.find((d) => d.kind === "plan");
  if (!view.state.plan_revision_pending || !plan?.exists || !n.primary) return n;
  const reviewing = n.primary.id === "approve-plan" || n.primary.id === "approve-override";
  if (!reviewing && !REVISION_GATED.has(n.primary.id)) return n;
  const evidence = n.evidence.some((f) => f.label === "Application")
    ? n.evidence
    : [docField("Revision", "plan", view, ctx.planSha), ...n.evidence];
  if (n.primary.id === "approve-override")
    return {
      ...n,
      heading: "Your turn: accept the plan revision",
      evidence,
      note: "Plan scope changed since approval. Accepting the revision approves this plan; open blockers need a recorded override.",
    };
  return {
    ...n,
    heading: "Your turn: accept the plan revision",
    evidence,
    note: reviewing
      ? "Plan scope changed since approval. Accepting the revision approves this plan."
      : `Plan scope changed since approval. Accept the revision, then ${n.primary.label.charAt(0).toLowerCase()}${n.primary.label.slice(1)}.`,
    primary: { id: "accept-revision", label: "Accept plan revision" },
    milestone: n.milestone,
  };
}

function stageNotice(view: SessionView, ctx: NoticeContext): Notice {
  const stage = view.state.stage;
  const open = openThreads(ctx.comments);
  const blockers = openBlockers(ctx.comments);
  const plan = view.docs.find((d) => d.kind === "plan");
  const research = view.docs.find((d) => d.kind === "research");
  const progress = planProgress(view);
  const position = buildPosition(view);
  const agentStage = AGENT_STAGES.includes(stage);
  const status = {
    label: "Status",
    value: `Open: ${open.length} · Blockers: ${blockers.length}`,
  };
  const tasks: NoticeField | null = progress.total
    ? { label: "Tasks", value: `${progress.done} / ${progress.total} done` }
    : null;

  const live = liveRun(view);
  const latest = recoveringRun(view);

  const secondary: NoticeAction[] = [];
  if ((stage === "planning" || stage === "plan-review") && plan?.exists)
    secondary.push({ id: "revise-plan", label: "Revise with agent" });
  const activeDoc = view.docs.find((d) => d.kind === ctx.activeTab);
  if (
    activeDoc?.exists &&
    (activeDoc.kind === "research" || activeDoc.kind === "plan")
  )
    secondary.push({
      id: "critique",
      label: `Review ${activeDoc.kind} with agent`,
    });
  if (activeDoc?.exists && activeDoc.kind === "plan")
    for (const q of CRITIQUE_PRESETS)
      secondary.push({ id: "critique", label: q, prompt: critiquePrompt(q) });
  secondary.push({ id: "ask", label: "Ask agent…" });
  if (
    (stage === "implementation-review" || stage === "done") &&
    !view.session.pull_request
  )
    secondary.push({ id: "create-pr", label: "Create a PR…" });

  const alternates: NoticeAction[] = [];
  const evidence: NoticeField[] = [];
  if (view.state.pause_reason)
    evidence.push({ label: "Checkpoint", value: view.state.pause_reason });
  if (live) {
    evidence.push({
      label: "Agent",
      value: `${live.provider} · ${live.task ?? live.stage} · ${RUN_WORD[live.status]}`,
    });
  }
  if ((view.state.proposals ?? []).some((p) => p.kind !== "plan-revision"))
    alternates.push({ id: "open-details", label: "Review the agent's workspace proposal" });

  const base = { secondary, alternates, evidence };
  const formFor = (): string => {
    if (stage === "new" || stage === "researching" || stage === "research-review")
      return "Research";
    if (stage === "planning" || stage === "plan-review") return "Plan";
    if (stage === "approved" || stage === "implementing")
      return position ? `Build ${position.k} of ${position.n}` : "Build";
    if (stage === "implementation-review") return "Final review";
    return "Complete";
  };

  if (live && agentStage && live.status !== "idle") {
    const waiting = live.status === "waiting";
    if (!waiting) alternates.unshift({ id: "open-run", label: "Open run", run: live });
    return {
      ...base,
      form: formFor(),
      heading: waiting
        ? "The agent needs your answer"
        : stage === "new" || stage === "researching"
          ? "The agent is researching"
          : stage === "planning"
            ? "The agent is planning"
            : "The agent is building",
      evidence: [...evidence, ...(tasks && stage !== "new" ? [tasks] : [])],
      primary: waiting
        ? { id: "open-run", label: "Answer the agent", run: live }
        : null,
    };
  }

  if (live && (!agentStage || live.status === "idle"))
    alternates.push({
      id: "open-run",
      label: live.status === "waiting" ? "Answer the agent" : "Open run",
      run: live,
    });

  if (latest && agentStage) {
    if (stage === "implementing")
      alternates.push({ id: "review-changes", label: "Review changes" }, FINISH_BUILD);
    return {
      ...base,
      form: formFor(),
      heading: `Recovery needed: the run ${latest.status}`,
      evidence: [
        ...evidence,
        { label: "Run", value: `${latest.provider} · ${latest.task ?? latest.stage}` },
        {
          label: "Reason",
          value:
            latest.error ??
            (latest.idle_stopped ? "Stopped after sitting idle" : latest.status === "failed" ? "Failed" : "Stopped"),
        },
      ],
      primary: { id: "resume-run", label: "Resume agent run", run: latest },
    };
  }
  const paused = latest ?? pausedRun(view);
  if (paused)
    alternates.push({ id: "resume-run", label: "Resume the stopped run", run: paused });

  const milestone = milestoneReview(view);
  if (milestone) {
    alternates.push({ id: "review-changes", label: "Review changes" });
    const k = (milestone.run.milestones_approved ?? 0) + 1;
    return {
      ...base,
      form: "Milestone review",
      heading: position
        ? `Milestone ${k} of ${position.n} is ready`
        : `Milestone ${k} is ready`,
      evidence: [
        ...evidence,
        ...(tasks ? [tasks] : []),
        status,
      ],
      note: milestone.committing
        ? undefined
        : "With difftool, the agent handles new comments in this run. Approving commits the milestone on the session branch and the agent starts the next phase.",
      milestone: { run: milestone.run, committing: milestone.committing },
      primary: milestone.committing
        ? null
        : milestone.reviewable
          ? {
              id: "approve-milestone",
              label: `Commit milestone ${k}${position ? ` of ${position.n}` : ""} and continue`,
              run: milestone.run,
            }
          : isLive(milestone.run)
            ? null
            : {
                id: "resume-milestone",
                label: "Resume milestone review",
                run: milestone.run,
              },
    };
  }

  switch (stage) {
    case "new":
      secondary.unshift(
        { id: "skip-to-plan", label: "Skip to plan" },
        { id: "skip-to-build", label: "Skip to build" },
      );
      return {
        ...base,
        form: "Research",
        heading: "Start with research",
        evidence: [
          ...evidence,
          { label: "Brief", value: view.session.brief ? "written" : "none yet" },
        ],
        guide: FIRST_RUN_GUIDE,
        copyPrompt: true,
        note: view.session.brief
          ? undefined
          : "Add a brief in Session details so the agent knows what success looks like.",
        primary: { id: "start-research", label: "Start research" },
      };
    case "researching":
    case "research-review": {
      const written = !!research?.exists;
      const firstRun = stage === "researching" && !written;
      (firstRun ? secondary : alternates).push({ id: "skip-to-build", label: "Skip to build" });
      return {
        ...base,
        guide: firstRun ? FIRST_RUN_GUIDE : undefined,
        copyPrompt: firstRun,
        form: "Research",
        heading:
          stage === "research-review"
            ? "Your turn: review the research"
            : written
              ? "Research is written: plan it"
              : "Research not written yet",
        evidence: [...evidence, docField("Document", "research", view), status],
        primary:
          stage === "researching" && !written
            ? { id: "start-research", label: "Start research" }
            : { id: "start-plan", label: "Plan it" },
      };
    }
    case "planning":
    case "plan-review": {
      if (!plan?.exists)
        return {
          ...base,
          form: "Plan",
          heading: "No plan yet",
          evidence: [...evidence, docField("Application", "plan", view)],
          primary: { id: "start-plan", label: "Start planning" },
        };
      const first = blockers[0];
      return {
        ...base,
        form: "Notice of allowance",
        heading: "Your turn: approve the plan",
        evidence: [
          ...evidence,
          docField("Application", "plan", view, ctx.planSha),
          status,
        ],
        blockedBy: first
          ? {
              text: `${plural(blockers.length, "blocker")} ${blockers.length === 1 ? "stops" : "stop"} approval`,
              comment: first,
            }
          : undefined,
        primary: first
          ? { id: "approve-override", label: "Approve with override…" }
          : { id: "approve-plan", label: "Approve plan" },
      };
    }
    case "approved":
      alternates.unshift({ id: "start-build-steps", label: "Build step by step" });
      return {
        ...base,
        form: formFor(),
        heading: "Plan approved: start the build",
        evidence: [
          ...evidence,
          docField("Allowed", "plan", view, view.state.approved?.sha),
          ...(tasks ? [tasks] : []),
        ],
        note: plan?.exists
          ? undefined
          : "No plan: the agent writes the tasks to plan.md first, then works through them.",
        primary: { id: "start-build", label: "Start build" },
      };
    case "implementing": {
      const allTicked = progress.total > 0 && progress.done >= progress.total;
      alternates.push({ id: "review-changes", label: "Review changes" });
      if (live || !allTicked) alternates.push(FINISH_BUILD);
      else alternates.push({ id: "continue-build", label: "Continue the build" });
      return {
        ...base,
        alternates: live
          ? alternates.filter((a) => a.id !== "open-run")
          : alternates,
        form: formFor(),
        heading: live
          ? "The agent finished its turn"
          : allTicked
            ? "Every task is ticked: review the build"
            : "The build is paused",
        evidence: [...evidence, ...(tasks ? [tasks] : []), status],
        primary: live
          ? { id: "open-run", label: "Reply to the agent", run: live }
          : allTicked
            ? FINISH_BUILD
            : { id: "continue-build", label: "Continue the build" },
      };
    }
    case "implementation-review":
      alternates.unshift({ id: "review-changes", label: "Review changes" });
      return {
        ...base,
        form: "Final review",
        heading: "Your turn: accept the implementation",
        evidence: [...evidence, ...(tasks ? [tasks] : []), status],
        primary: { id: "accept", label: "Accept and finish" },
      };
    case "done": {
      const pr = view.session.pull_request;
      return {
        ...base,
        form: "Complete",
        heading: "Done",
        evidence: [
          ...evidence,
          ...(tasks ? [{ label: "Shipped", value: `${progress.done} / ${progress.total} tasks` }] : []),
          pr
            ? {
                label: "Pull request",
                value: `#${pr.number} · ${pr.state.toLowerCase()}${pr.draft ? " draft" : ""}`,
                href: pr.url,
              }
            : { label: "Pull request", value: "none" },
        ],
        primary: null,
      };
    }
  }
}
