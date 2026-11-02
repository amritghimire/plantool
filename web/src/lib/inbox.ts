import type { SessionView } from "../types";
import {
  buildPosition,
  liveRun,
  milestoneReview,
  openBlockers,
  planProgress,
  recoveringRun,
} from "./notice";

export type Queue = "waiting" | "running" | "done" | "idle";
export type Mark = "waiting" | "question" | "recovery" | "running" | "done" | "idle";

export interface Attention {
  queue: Queue;
  mark: Mark;
  status: string;
  action: string;
  open: number;
  reason: string;
  since: string;
}

export const QUEUES: { id: Queue; title: string }[] = [
  { id: "waiting", title: "Waiting on you" },
  { id: "running", title: "Agent running" },
  { id: "done", title: "Recently done" },
  { id: "idle", title: "Not started or idle" },
];

const STATUS: Record<Mark, string> = {
  waiting: "Waiting on you",
  question: "Agent needs your answer",
  recovery: "Recovery needed",
  running: "Agent running",
  done: "Done",
  idle: "Not started",
};

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

function classify(view: SessionView): { mark: Mark; action: string; counts?: boolean } {
  const stage = view.state.stage;
  const live = liveRun(view);
  const position = buildPosition(view);
  const build = position ? `Build ${position.k} of ${position.n}` : "Build";

  if (live?.status === "waiting") return { mark: "question", action: "Agent needs your answer" };

  const stopped = recoveringRun(view);
  if (stopped && ["new", "researching", "planning", "approved", "implementing"].includes(stage))
    return { mark: "recovery", action: `Agent ${stopped.status} · resume` };

  const milestone = milestoneReview(view);
  if (milestone) {
    const k = (milestone.run.milestones_approved ?? 0) + 1;
    const name = `Milestone ${k}${position ? ` of ${position.n}` : ""}`;
    if (milestone.committing) return { mark: "running", action: `Committing ${name.toLowerCase()}` };
    if (milestone.reviewable || !live) return { mark: "waiting", action: `${name} ready to commit`, counts: true };
    return { mark: "running", action: `${build} running` };
  }

  if (live && live.status !== "idle") {
    if (stage === "new" || stage === "researching") return { mark: "running", action: "Researching" };
    if (stage === "planning") return { mark: "running", action: "Planning" };
    if (stage === "approved" || stage === "implementing") return { mark: "running", action: `${build} running` };
  }
  if (live?.status === "idle" && stage === "implementing")
    return { mark: "waiting", action: "Agent finished its turn · reply" };

  const research = view.docs.find((d) => d.kind === "research")?.exists;
  const plan = view.docs.find((d) => d.kind === "plan")?.exists;
  switch (stage) {
    case "new":
      return { mark: "idle", action: "Not started" };
    case "researching":
      return research
        ? { mark: "waiting", action: "Research written · plan it", counts: true }
        : { mark: "idle", action: "No research yet" };
    case "research-review":
      return { mark: "waiting", action: "Review research", counts: true };
    case "planning":
      return plan
        ? { mark: "waiting", action: "Approve plan", counts: true }
        : { mark: "idle", action: "No plan yet" };
    case "plan-review":
      return plan
        ? { mark: "waiting", action: "Approve plan", counts: true }
        : { mark: "idle", action: "No plan yet" };
    case "approved":
      return { mark: "waiting", action: "Plan approved · start build" };
    case "implementing": {
      const { done, total } = planProgress(view);
      return total > 0 && done >= total
        ? { mark: "waiting", action: "Every task ticked · review the build", counts: true }
        : { mark: "waiting", action: `${build} paused · continue`, counts: true };
    }
    case "implementation-review":
      return { mark: "waiting", action: "Accept implementation", counts: true };
    case "done":
      return { mark: "done", action: "Done" };
  }
}

const QUEUE_OF: Record<Mark, Queue> = {
  waiting: "waiting",
  question: "waiting",
  recovery: "waiting",
  running: "running",
  done: "done",
  idle: "idle",
};

export function sessionAttention(view: SessionView): Attention {
  const { mark, action, counts } = classify(view);
  const open = view.open_comments;
  const blockers = counts ? openBlockers(view.state.comments).length : 0;
  const blockerText = blockers ? plural(blockers, "blocker") : null;
  return {
    queue: QUEUE_OF[mark],
    mark,
    status: STATUS[mark],
    action: [action, blockerText].filter(Boolean).join(" · "),
    open,
    reason: [action, counts && open ? `${open} open` : null, blockerText]
      .filter(Boolean)
      .join(" · "),
    since: view.state.updated_at || view.session.created_at,
  };
}

export function groupQueue(views: SessionView[]): Record<Queue, SessionView[]> {
  const groups: Record<Queue, SessionView[]> = { waiting: [], running: [], done: [], idle: [] };
  const attention = new Map(views.map((v) => [v.key, sessionAttention(v)]));
  for (const v of views) groups[attention.get(v.key)!.queue].push(v);
  const at = (v: SessionView) => attention.get(v.key)!.since;
  groups.waiting.sort((a, b) => at(a).localeCompare(at(b)));
  for (const q of ["running", "done", "idle"] as const)
    groups[q].sort((a, b) => at(b).localeCompare(at(a)));
  return groups;
}

export function age(iso: string, now = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const m = Math.max(0, Math.round((now - t) / 60_000));
  if (m < 1) return "now";
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}h`;
  const d = Math.round(h / 24);
  return d < 60 ? `${d}d` : `${Math.round(d / 30)}mo`;
}
