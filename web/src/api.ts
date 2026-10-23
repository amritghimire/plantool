import { authenticatedFetch } from "./auth";
import type { ChangeScope, ChangesResponse, Comment, DocKind, DocResponse, FileDiff, NavTarget, RepoInfo, Run, Session, SessionView, Stage, WorktreeDirSetting } from "./types";

export type PromptStage = "research" | "plan" | "implement" | "review" | "resume" | "critique" | "assist" | "draft-pr" | "next";

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { "x-plantool-author": authorName() };
  if (body !== undefined) headers["content-type"] = "application/json";
  const res = await authenticatedFetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }, body);
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
  githubPlan: (id:string) => request<{gate:{number:number;hash:string;state:string};summary:string;plan:string;messages:{role:string;body:string}[]}>("GET",`/api/github/plans/${encodeURIComponent(id)}`),
  githubSettings: () => request<{repositories:{project:string;repo_id:number;full_name:string;visibility?:string|null;preflight?:{warnings:string[];checked_at?:string}|null}[];status?:{supported:boolean;worker:{available:boolean;message:string}};allowlist:{id:number;login:string}[];sync:{project:string;error:string|null;synced_at:string|null}[]}>("GET","/api/settings/github"),
  configureGithub: (body:{app_id:number;installation_id:number;repo:string;checkout:string;private_key:string|null}) => request<{preflight:{warnings:string[]}}>("POST","/api/settings/github",body),
  githubPreflight: (project:string) => request<{warnings:string[]}>("POST","/api/settings/github/preflight",{project}),
  githubAllowlist: (login:string,remove:boolean) => request("POST","/api/settings/github/allowlist",{login,remove}),
  modelSettings: () => request<{yaml:string;projects:{id:string;checkout:string;visibility?:string|null;allowed_providers:string[]}[];routes:{route:string;available:boolean;model:string|null;error:string|null}[]}>("GET","/api/settings/models"),
  saveModels: (body:{yaml:string;project:string|null;allowed_providers:string[]|null}) => request("POST","/api/settings/models",body),
  verifyModels: () => request("POST","/api/settings/models/verify",{}),
  workerStatus: () => request<{worker:{available:boolean;message:string}}>("GET","/api/engineering-os/status"),
  repairWorker: () => request<{worker:{available:boolean;message:string}}>("POST","/api/engineering-os/repair", {}),

  retentionPolicy: () => request<{transcript_days:number;evidence_days:number;disk_cap_bytes:number}>("GET","/api/settings/retention"),
  saveRetentionPolicy: (policy:{transcript_days:number;evidence_days:number;disk_cap_bytes:number}) => request<{ok:boolean}>("POST","/api/settings/retention",policy),
  pinRun: (key:string,id:string,pinned:boolean) => request<{ok:boolean;pinned:boolean}>("POST",`/api/sessions/${key}/runs/${id}/pin`,{pinned}),
  cleanBreakNotice: () => request<{backup:string|null}>("GET","/api/notices/clean-break"),
  dismissCleanBreakNotice: () => request<{ok:boolean}>("POST","/api/notices/clean-break",{}),
  executionStatus: () => request<{ execution: { stopped: boolean; generation: number }; sandbox_supported: boolean }>("GET", "/api/execution/status"),
  stopAll: () => request<{ execution: { stopped: boolean } }>("POST", "/api/execution/stop", {}),
  resumeExecution: () => request<{ execution: { stopped: boolean } }>("POST", "/api/execution/resume", {}),
  neverSent: (repo: string) => request<{ defaults: string[]; patterns: string[] }>("GET", `/api/settings/never-sent?repo=${encodeURIComponent(repo)}`),
  setNeverSent: (repo: string, patterns: string[]) => request("POST", "/api/settings/never-sent", { repo, patterns }),
  setContext: (key: string, body: { base?: string; branch?: string; workspace?: string; difftool?: string; pause_rule?: { mode: string; rule?: string } }) => request<{ view: SessionView }>("POST", `/api/sessions/${key}/where`, body),
  contextProposal: (key: string, id: string, dismiss: boolean) => request<{ view: SessionView }>("POST", `/api/sessions/${key}/proposals`, { id, dismiss }),
  diffToolSetting: () => request<{ value: string | null; difftool: string | null }>("GET", "/api/settings/difftool"),
  setDiffToolSetting: (value: string) => request("POST", "/api/settings/difftool", { value }),
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
  planRevisions: (key: string) => request<{ current: DocResponse | null; approved: DocResponse | null; viewed: DocResponse | null; change: string | null; pending: boolean }>("GET", `/api/sessions/${key}/docs/plan/revisions`),
  markViewed: (key: string, kind: DocKind, sha: string) => request("POST", `/api/sessions/${key}/docs/${kind}/viewed`, { sha }),
  docPath: (key: string, kind: DocKind) => request<{ kind: DocKind; path: string; exists: boolean }>("GET", `/api/sessions/${key}/docs/${kind}/path`),
  comments: (key: string) => request<{ seq: number; comments: Comment[] }>("GET", `/api/sessions/${key}/comments`),
  addComment: (key: string, c: { doc: DocKind; line?: number; match?: string; body: string; parent?: string; type?: import("./types").CommentType; scope?: import("./types").AnchorScope; code?: import("./types").CodeAnchor }) =>
    request<{ comments: Comment[]; seq: number }>("POST", `/api/sessions/${key}/comments/batch`, { comments: [c] }),
  editComment: (key: string, id: string, body: string) => request<{ comments: Comment[] }>("POST", `/api/sessions/${key}/comments/edit`, { edits: [{ id, body }] }),
  promoteBlocker: (key: string, id: string) => request<{ comment: Comment }>("POST", `/api/sessions/${key}/comments/${id}/promote`, {}),
  resolve: (key: string, ids: string[], resolved: boolean) => request<{ comments: Comment[] }>("POST", `/api/sessions/${key}/comments/resolve`, { ids, resolved }),
  removeComments: (key: string, ids: string[]) => request<{ removed: number }>("POST", `/api/sessions/${key}/comments/remove`, { ids }),
  setStage: (key: string, to: Stage, override_reason?: string, plan_sha?: string) => request<{ stage: Stage }>("POST", `/api/sessions/${key}/stage`, { to, override_reason, plan_sha }),
  navigate: (key: string, target: Partial<NavTarget>) => request<{ viewers: number }>("POST", `/api/sessions/${key}/navigate`, target),
  changes: (key: string, scope?: ChangeScope) => request<ChangesResponse>("GET", `/api/sessions/${key}/changes${scope ? `?scope=${scope}` : ""}`),
  openChanges: (key: string, scope?: ChangeScope) => request<ChangesResponse>("POST", `/api/sessions/${key}/changes/open`, { scope: scope ?? null }),
  changesFile: (key: string, path: string, scope?: ChangeScope) => request<FileDiff>("GET", `/api/sessions/${key}/changes/file?path=${encodeURIComponent(path)}${scope ? `&scope=${scope}` : ""}`),
  startRun: (key: string, body: { sandbox?: boolean; milestone?: string; pause_rule?: { mode: string; rule?: string }; provider: string; stage: string; model?: string; effort?: string; prompt?: string; permission_mode?: string; worktree?: boolean; resume_run?: string; implementation_mode?: string }) => request<{ run: Run; prompt: string }>("POST", `/api/sessions/${key}/runs`, body),
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
  uploadAttachment: async (key: string, file: File): Promise<{ name: string; path: string }> => {
    const res = await fetch(`/api/sessions/${key}/attachments`, {
      method: "POST",
      headers: { "x-plantool-filename": file.name.replace(/[^\x20-\x7e]/g, "_") },
      body: file,
    });
    const data = await res.json();
    if (!res.ok) throw new ApiError(res.status, data.error ?? "Upload failed");
    return data;
  },
};

export function liveUrl(key: string): string {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  return `${proto}://${location.host}/api/sessions/${key}/live`;
}
