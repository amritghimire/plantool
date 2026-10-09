import { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { relTime } from "../lib/format";
import type { Comment } from "../types";
import { Composer } from "./Composer";

export interface ThreadActions {
  promote?: (root: Comment) => Promise<void>;
  reply: (parent: Comment, body: string) => Promise<void>;
  resolve: (root: Comment, resolved: boolean) => Promise<void>;
  edit: (c: Comment, body: string) => Promise<void>;
  remove: (c: Comment) => Promise<void>;
}

export function Thread({ root, replies, actions, highlighted }: { root: Comment; replies: Comment[]; actions: ThreadActions; highlighted?: boolean }) {
  const [replying, setReplying] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(root.resolved);
  const all = [root, ...replies];
  const latestAgent = replies.filter((r) => r.kind === "agent").sort((a, b) => b.seq - a.seq)[0];
  const status = root.resolved ? "Resolved" : latestAgent?.proposes_resolve ? "Agent proposes resolution" : latestAgent ? "Agent replied · awaiting your review" : root.anchor.outdated ? "Outdated · unaddressed" : "Open";
  return (
    <div id={`c-${root.id}`} className={`thread ${root.resolved ? "resolved" : ""} ${highlighted ? "highlight" : ""}`}>
      <div className="thread-head">
        <button className="link" aria-expanded={!collapsed} onClick={() => setCollapsed((c) => !c)} type="button">{status}</button>
        <span>{(root.type ?? "note").replace("-", " ")}</span>
        <span className={`kind kind-${root.kind}`}>{root.kind}</span>
        <span className="muted">
          {root.anchor.code ? `${root.anchor.code.path}:${root.anchor.code.line}` : root.anchor.scope === "document" ? "Document" : root.anchor.scope === "section" ? `Section · line ${root.anchor.line}` : `line ${root.anchor.line}`}
          {root.anchor.outdated && <span className="outdated"> · outdated</span>}
          {root.resolved && " · resolved"}
        </span>
        <span className="spacer" />
        <span className="muted">{all.length > 1 ? `${all.length} comments` : ""}</span>
        <span className="muted">{collapsed ? "▸" : "▾"}</span>
      </div>
      {!collapsed && (
        <>
          {all.map((c) => (
            <div key={c.id} className="comment">
              <div className="comment-meta">
                <strong>{c.author}</strong>
                <span className="muted">{relTime(c.created_at)}</span>
                {c.updated_at && <span className="muted">(edited)</span>}
                <span className="spacer" />
                <button className="link" onClick={() => setEditing(editing === c.id ? null : c.id)} type="button">
                  edit
                </button>
                <button className="link" onClick={() => void actions.remove(c)} type="button">
                  delete
                </button>
              </div>
              {editing === c.id ? (
                <EditBox initial={c.body} onCancel={() => setEditing(null)} onSubmit={async (b) => { await actions.edit(c, b); setEditing(null); }} />
              ) : (
                <div className="comment-body">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>{c.body}</ReactMarkdown>
                </div>
              )}
            </div>
          ))}
          <div className="thread-actions">
            {replying ? (
              <Composer draftKey={`reply:${root.id}`} placeholder="Reply…" submitLabel="Reply" onCancel={() => setReplying(false)} onSubmit={async (b) => { await actions.reply(root, b); setReplying(false); }} />
            ) : (
              <>
                {root.kind === "agent" && actions.promote && <button className="btn ghost" type="button" onClick={() => void actions.promote?.(root)}>Promote to blocker</button>}
                <button className="btn ghost" onClick={() => setReplying(true)} type="button">
                  Reply
                </button>
                <button className="btn ghost" onClick={() => void actions.resolve(root, !root.resolved)} type="button">
                  {root.resolved ? "Reopen" : "Resolve"}
                </button>
              </>
            )}
          </div>
        </>
      )}
    </div>
  );
}

function EditBox({ initial, onSubmit, onCancel }: { initial: string; onSubmit: (b: string) => Promise<void>; onCancel: () => void }) {
  const [body, setBody] = useState(initial);
  return (
    <div className="composer">
      <textarea value={body} rows={3} onChange={(e) => setBody(e.target.value)} />
      <div className="composer-actions">
        <span className="spacer" />
        <button className="btn ghost" onClick={onCancel} type="button">
          Cancel
        </button>
        <button className="btn primary" onClick={() => void onSubmit(body)} type="button">
          Save
        </button>
      </div>
    </div>
  );
}
