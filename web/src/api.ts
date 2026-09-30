import type { ChangeScope, ChangesResponse, Comment, DocKind, DocResponse, FileDiff, NavTarget, RepoInfo, Run, Session, SessionView, Stage, WorktreeDirSetting } from "./types";

export type PromptStage = "research" | "plan" | "implement" | "review" | "resume" | "critique" | "assist" | "draft-pr" | "next";

export function token(): string {
  const t = window.__PLANTOOL_TOKEN__ ?? "";
  return t === "__PLANTOOL_TOKEN__" ? "" : t;
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { "x-plantool-actor": token(), "x-plantool-author": authorName() };
  if (body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      msg = (JSON.parse(text) as { error?: string }).error ?? text;
    } catch {
      // keep raw text
    }
    throw new ApiError(res.status, msg);
  }
  return text ? (JSON.parse(text) as T) : (undefined as T);
}

export function authorName(): string {
  try {
    return localStorage.getItem("plantool.author") || "you";
  } catch {
    return "you";
  }
}

export function setAuthorName(name: string) {
  try {
    localStorage.setItem("plantool.author", name);
  } catch {
    // ignore
  }
}

export const api = {
  sessions: () => request<SessionView[]>("GET", "/api/sessions"),
  session: (key: string) => request<SessionView>("GET", `/api/sessions/${key}`),
  repos: () => request<{ repos: RepoInfo[] }>("GET", "/api/repos"),
  createSession: (body: { slug: string; repo: string; title?: string; brief?: string; worktree?: boolean; base?: string }) =>
    request<{ created: boolean; session: SessionView; url: string }>("POST", "/api/sessions", body),
  removeSession: (key: string, worktree: boolean) => request<{ removed: string; worktree_removed: boolean }>("DELETE", `/api/sessions/${key}?worktree=${worktree}`),
  workspaceCandidates: (key: string) => request<{ recorded: string | null; missing: boolean; candidates: { root: string; branch: string }[] }>("GET", `/api/sessions/${key}/workspace/candidates`),
  adoptWorkspace: (key: string, path: string) => request<{ session: Session }>("POST", `/api/sessions/${key}/workspace/adopt`, { path }),
  prDraft: (key: string) => request<{ title: string; body: string; exists: boolean }>("GET", `/api/sessions/${key}/pr/draft`),
  savePrDraft: (key: string, title: string, body: string) => request<{ saved: boolean }>("POST", `/api/sessions/${key}/pr/draft`, { title, body }),
  prPreview: (key: string) => request<{ head: string; base: string; repository: string; push_remote: string; dirty: boolean; changed_files: string[]; live_run: boolean; commits_ahead: number; existing: NonNullable<Session["pull_request"]> | null }>("GET", `/api/sessions/${key}/pr/preview`),
  prCommitDraft: (key: string) => request<{ message: string; exists: boolean }>("GET", `/api/sessions/${key}/pr/commit`),
  prCommit: (key: string, message: string) => request<{ sha: string; rewritten: string[] }>("POST", `/api/sessions/${key}/pr/commit`, { message }),
  cancelCommit: (key: string) => request<{ ok: boolean }>("POST", `/api/sessions/${key}/commit/cancel`),
  createPr: (key: string, body: { head: string; repository: string; title: string; body: string; draft: boolean }) => request<{ pull_request: NonNullable<Session["pull_request"]>; session: Session }>("POST", `/api/sessions/${key}/pr`, body),
  refreshPr: (key: string) => request<{ pull_request: NonNullable<Session["pull_request"]>; session: Session }>("POST", `/api/sessions/${key}/pr/refresh`),
  doc: (key: string, kind: DocKind) => request<DocResponse>("GET", `/api/sessions/${key}/docs/${kind}`),
  docPath: (key: string, kind: DocKind) => request<{ kind: DocKind; path: string; exists: boolean }>("GET", `/api/sessions/${key}/docs/${kind}/path`),
  comments: (key: string) => request<{ seq: number; comments: Comment[] }>("GET", `/api/sessions/${key}/comments`),
  addComment: (key: string, c: { doc: DocKind; line?: number; match?: string; body: string; parent?: string }) =>
    request<{ comments: Comment[]; seq: number }>("POST", `/api/sessions/${key}/comments/batch`, { comments: [c] }),
  editComment: (key: string, id: string, body: string) => request<{ comments: Comment[] }>("POST", `/api/sessions/${key}/comments/edit`, { edits: [{ id, body }] }),
  resolve: (key: string, ids: string[], resolved: boolean) => request<{ comments: Comment[] }>("POST", `/api/sessions/${key}/comments/resolve`, { ids, resolved }),
  removeComments: (key: string, ids: string[]) => request<{ removed: number }>("POST", `/api/sessions/${key}/comments/remove`, { ids }),
  setStage: (key: string, to: Stage) => request<{ stage: Stage }>("POST", `/api/sessions/${key}/stage`, { to }),
  navigate: (key: string, target: Partial<NavTarget>) => request<{ viewers: number }>("POST", `/api/sessions/${key}/navigate`, target),
  changes: (key: string, scope?: ChangeScope) => request<ChangesResponse>("GET", `/api/sessions/${key}/changes${scope ? `?scope=${scope}` : ""}`),
  openChanges: (key: string, scope?: ChangeScope) => request<ChangesResponse>("POST", `/api/sessions/${key}/changes/open`, { scope: scope ?? null }),
  changesFile: (key: string, path: string, scope?: ChangeScope) => request<FileDiff>("GET", `/api/sessions/${key}/changes/file?path=${encodeURIComponent(path)}${scope ? `&scope=${scope}` : ""}`),
  startRun: (key: string, body: { provider: string; stage: string; model?: string; prompt?: string; permission_mode?: string; worktree?: boolean; resume_run?: string; implementation_mode?: string }) => request<{ run: Run; prompt: string }>("POST", `/api/sessions/${key}/runs`, body),
  prompt: (key: string, stage: PromptStage, extra?: string, implementationMode?: string, resumeRun?: string) => {
    const query = new URLSearchParams();
    if (extra) query.set("extra", extra);
    if (implementationMode) query.set("implementation_mode", implementationMode);
    if (resumeRun) query.set("resume_run", resumeRun);
    return request<{ stage: Exclude<PromptStage, "next">; prompt: string; session_stage: Stage }>("GET", `/api/sessions/${key}/prompt/${stage}${query.size ? `?${query}` : ""}`);
  },
  setBrief: (key: string, brief: string | null) => request<{ brief: string | null; session: Session }>("POST", `/api/sessions/${key}/brief`, { brief }),
  runInput: (key: string, id: string, body: unknown) => request<{ ok: boolean }>("POST", `/api/sessions/${key}/runs/${id}/input`, body),
  milestone: (key: string, id: string) => request<{ subject: string; dirty: boolean; milestone: number; pending: boolean; live: boolean }>("GET", `/api/sessions/${key}/runs/${id}/milestone`),
  approveMilestone: (key: string, id: string, commit: boolean, message: string) => request<{ ok: boolean; committed: boolean; sha: string }>("POST", `/api/sessions/${key}/runs/${id}/milestone/approve`, { commit, message }),
  stopRun: (key: string, id: string) => request<{ ok: boolean }>("POST", `/api/sessions/${key}/runs/${id}/stop`, {}),
  removeRun: (key: string, id: string) => request<{ ok: boolean; removed: string }>("DELETE", `/api/sessions/${key}/runs/${id}`),
  runEvents: (key: string, id: string, since = 0) => request<{ events: { seq: number; at: string; event: unknown }[] }>("GET", `/api/sessions/${key}/runs/${id}/events?since=${since}`),
  worktreeDir: (repo?: string, preview?: string) => {
    const query = new URLSearchParams();
    if (repo) query.set("repo", repo);
    if (preview) query.set("preview", preview);
    return request<WorktreeDirSetting>("GET", `/api/settings/worktree-dir${query.size ? `?${query}` : ""}`);
  },
  setWorktreeDir: (scope: "global" | "repo", value: string | null, repo?: string) => request<WorktreeDirSetting>("POST", "/api/settings/worktree-dir", { scope, repo, value }),
  providers: () => request<{ providers: { id: string; available: boolean; version?: string; error?: string; models: { id: string; label: string }[] }[] }>("GET", "/api/providers"),
};

export function liveUrl(key: string): string {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  return `${proto}://${location.host}/api/sessions/${key}/live`;
}
