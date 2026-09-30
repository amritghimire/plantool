export type Stage =
  | "new"
  | "researching"
  | "research-review"
  | "planning"
  | "plan-review"
  | "approved"
  | "implementing"
  | "implementation-review"
  | "done";

export const STAGES: Stage[] = [
  "new",
  "researching",
  "research-review",
  "planning",
  "plan-review",
  "approved",
  "implementing",
  "implementation-review",
  "done",
];

export const STAGE_LABEL: Record<Stage, string> = {
  new: "New",
  researching: "Researching",
  "research-review": "Research review",
  planning: "Planning",
  "plan-review": "Plan review",
  approved: "Approved",
  implementing: "Implementing",
  "implementation-review": "Implementation review",
  done: "Done",
};

export type DocKind = "research" | "plan" | "investigation" | "quick-fix" | "design";
export type CommentKind = "human" | "agent";
export type Actor = "human" | "agent";

export interface Anchor {
  line: number;
  text: string;
  outdated: boolean;
}

export interface Comment {
  id: string;
  doc: DocKind;
  anchor: Anchor;
  body: string;
  kind: CommentKind;
  author: string;
  parent: string | null;
  resolved: boolean;
  created_at: string;
  updated_at: string | null;
  seq: number;
}

export interface PhaseProgress {
  name: string;
  done: number;
  total: number;
}

export interface DocSummary {
  kind: DocKind;
  path: string;
  exists: boolean;
  sha: string | null;
  lines: number;
  title: string | null;
  captured_at: string | null;
  progress: PhaseProgress[];
}

export interface Checkout {
  root: string;
  common_dir: string;
  branch: string;
}

export interface Session {
  repo_slug: string;
  slug: string;
  title: string;
  repo: Checkout;
  worktree: string | null;
  pull_request?: { number: number; url: string; state: string; draft: boolean; updated_at: string } | null;
  base: string;
  mirror: boolean;
  brief?: string | null;
  created_in?: string | null;
  created_at: string;
}

export interface WorktreeDirSetting {
  key: string;
  default: string;
  global: string | null;
  repo: { root: string; value: string | null } | null;
  effective: string;
  example: string | null;
}

export interface RepoInfo {
  root: string;
  repo_slug: string;
  branch: string;
  base: string;
  sessions: number;
  last_used: string;
}

export type ChangeReview =
  | { tool: "difftool"; url: string; review: string | null; opened_at: string }
  | { tool: "git-difftool"; opened_at: string };

export interface State {
  stage: Stage;
  comments: Comment[];
  docs: Record<string, { sha: string; captured_at: string; lines: number }>;
  seq: number;
  review: ChangeReview | null;
  updated_at: string;
}

export type Provider = "claude" | "codex" | "opencode";
export type PermissionMode = "ask" | "accept-edits" | "auto" | "allow-all";
export const PERMISSION_MODES: { id: PermissionMode; label: string; hint: string }[] = [
  { id: "ask", label: "Ask", hint: "Every edit and command is confirmed here." },
  { id: "accept-edits", label: "Accept edits", hint: "File edits go through; commands still ask." },
  { id: "auto", label: "Auto", hint: "The agent's own judgement decides; risky actions still ask." },
  { id: "allow-all", label: "Allow all", hint: "Nothing asks. Only for a sandboxed checkout." },
];
export type RunStatus = "starting" | "running" | "waiting" | "idle" | "stopped" | "failed";
export type ImplementationMode = "all-at-once" | "step-by-step";

export interface Run {
  id: string;
  provider: Provider;
  provider_session_id: string | null;
  stage: Stage;
  implementation_mode?: ImplementationMode;
  milestone_pending?: boolean;
  milestone_review?: ChangeReview | null;
  milestone_base?: string | null;
  milestones_approved?: number;
  milestone_commit?: string | null;
  implementation_base?: string | null;
  task?: string;
  cwd: string;
  status: RunStatus;
  model: string | null;
  permission_mode?: PermissionMode;
  started_at: string;
  ended_at: string | null;
  error: string | null;
  seq: number;
}

export interface SessionView {
  key: string;
  url_path: string;
  dir: string;
  session: Session;
  state: State;
  docs: DocSummary[];
  runs: Run[];
  open_comments: number;
  workspace_branch?: string | null;
  commit?: CommitJob | null;
}

export type CommitScope = { kind: "milestone"; run_id: string } | { kind: "pr" };
export type CommitPhase = "staging" | "hooks" | "done" | "failed" | "cancelled";

export interface CommitJob {
  id: string;
  scope: CommitScope;
  phase: CommitPhase;
  started_at: string;
  lines: string[];
  error?: string | null;
}

export interface Heading {
  line: number;
  level: number;
  text: string;
}

export interface DocResponse {
  kind: DocKind;
  sha: string;
  content: string;
  captured_at: string;
  path: string;
  lines: number;
  headings: Heading[];
  progress: PhaseProgress[];
}

export interface NavTarget {
  doc: DocKind | null;
  line: number | null;
  comment: string | null;
  tab: string | null;
}

export interface FileStat {
  path: string;
  added: number;
  deleted: number;
}

export type ChangeScope = "step" | "all";

export interface ChangesResponse {
  tool: "difftool" | "git-difftool" | "none";
  scope: ChangeScope;
  base: string;
  label: string;
  step_available: boolean;
  stat: FileStat[];
  review: ChangeReview | null;
  message?: string;
}

export interface FileDiff {
  path: string;
  diff: string;
  truncated: boolean;
}

export type LiveEvent =
  | { type: "hello"; seq: number; key: string }
  | { type: "resync"; seq: number }
  | { type: "comments-since"; seq: number; comments: Comment[] }
  | { type: "comment-added"; seq: number; comment: Comment }
  | { type: "comments-added"; seq: number; comments: Comment[] }
  | { type: "comment-updated"; seq: number; comment: Comment }
  | { type: "comment-removed"; seq: number; ids: string[] }
  | { type: "thread-resolution"; seq: number; ids: string[]; resolved: boolean }
  | { type: "doc-refreshed"; seq: number; kind: DocKind; sha: string; lines: number; reanchored: number; outdated: number }
  | { type: "stage-changed"; seq: number; from: Stage; to: Stage; actor: Actor }
  | { type: "session-updated"; seq: number; session: Session }
  | { type: "session-removed"; seq: number; key: string }
  | { type: "run-started"; seq: number; run: Run }
  | { type: "run-updated"; seq: number; run: Run }
  | { type: "run-event"; seq: number; at?: string; run_id: string; event: RunEvent }
  | { type: "run-ended"; seq: number; run: Run }
  | { type: "run-removed"; seq: number; id: string }
  | { type: "changes-opened"; seq: number; review: ChangeReview }
  | { type: "navigate"; seq: number; target: NavTarget; viewers: number }
  | { type: "commit-progress"; seq: number; commit: CommitJob };

export type RunEvent =
  | { type: "message"; id: string; role: "user" | "assistant"; content: string }
  | { type: "text-delta"; delta: string; segment?: string }
  | { type: "activity-start"; id: string; kind: string; title: string; detail?: string }
  | { type: "activity-output"; id: string; delta: string }
  | { type: "activity-complete"; id: string; status: "completed" | "failed" | "denied"; output?: string }
  | { type: "permission"; request_id: string; kind: string; title: string; detail?: string; options?: { id: string; label: string }[] }
  | { type: "input-request"; request_id: string; title: string; questions: { id: string; text: string; options?: string[] }[] }
  | { type: "request-resolved"; request_id: string }
  | { type: "turn-started"; turn_id: string }
  | { type: "turn-completed"; turn_id: string; status: "completed" | "interrupted" | "failed"; error?: string }
  | { type: "provider-session"; session_id: string }
  | { type: "status"; label: string; detail?: string }
  | { type: "raw"; line: string };
