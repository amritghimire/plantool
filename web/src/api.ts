import type { ChangesResponse, Comment, DocKind, DocResponse, NavTarget, Run, SessionView, Stage } from "./types";

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
  changes: (key: string) => request<ChangesResponse>("GET", `/api/sessions/${key}/changes`),
  openChanges: (key: string) => request<ChangesResponse>("POST", `/api/sessions/${key}/changes/open`, {}),
  startRun: (key: string, body: { provider: string; stage: string; model?: string; prompt?: string }) => request<{ run: Run }>("POST", `/api/sessions/${key}/runs`, body),
  runInput: (key: string, id: string, body: unknown) => request<{ ok: boolean }>("POST", `/api/sessions/${key}/runs/${id}/input`, body),
  stopRun: (key: string, id: string) => request<{ ok: boolean }>("POST", `/api/sessions/${key}/runs/${id}/stop`, {}),
  runEvents: (key: string, id: string, since = 0) => request<{ events: { seq: number; at: string; event: unknown }[] }>("GET", `/api/sessions/${key}/runs/${id}/events?since=${since}`),
  providers: () => request<{ providers: { id: string; available: boolean; version?: string; error?: string; models: { id: string; label: string }[] }[] }>("GET", "/api/providers"),
};

export function liveUrl(key: string): string {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  return `${proto}://${location.host}/api/sessions/${key}/live`;
}
