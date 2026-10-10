import { useState } from "react";
import { api } from "../api";
import type { Comment, CommentType } from "../types";
import { Composer } from "./Composer";
import type { ThreadActions } from "./Thread";

export function CodeReview({ sessionKey, path, line, context, comments, onAdded }: { sessionKey: string; path: string; line: number; context: string; comments: Comment[]; actions: ThreadActions; onAdded: () => void }) {
  const [open, setOpen] = useState(false);
  const count = comments.filter((c) => !c.parent && c.anchor.code?.path === path && c.anchor.code.line === line).length;
  const add = async (body: string, type?: CommentType) => {
    await api.addComment(sessionKey, { doc: "plan", body, type, code: { path, side: "new", line, context } });
    setOpen(false); onAdded();
  };
  return <div className="code-review button-row">
    <button type="button" className="link" aria-expanded={open} aria-label={`Comment on ${path}:${line}`} onClick={() => setOpen(!open)}>Comment on line {line}</button>
    {open && <Composer showType draftKey={`code:${sessionKey}:${path}:${line}`} placeholder={`Comment on ${path}:${line}…`} onSubmit={add} onCancel={() => setOpen(false)} />}
    {count > 0 && <span className="muted small">{count} comment{count === 1 ? "" : "s"} · see Code review comments above</span>}
  </div>;
}
