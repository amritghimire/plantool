import { useEffect, useRef, useState } from "react";
import type { CommentType } from "../types";
import { clearDraft, getDraft, setDraft } from "../lib/drafts";

export function Composer({ placeholder, onSubmit, onCancel, autoFocus = true, submitLabel = "Comment", draftKey, showType = false }: {
  placeholder: string;
  onSubmit: (body: string, type?: CommentType) => Promise<void>;
  onCancel?: () => void;
  autoFocus?: boolean;
  submitLabel?: string;
  /** Keeps the text across re-renders and document refreshes. */
  draftKey?: string;
  showType?: boolean;
}) {
  const [body, setBodyState] = useState(() => (draftKey ? getDraft(draftKey) : ""));
  const setBody = (v: string) => {
    setBodyState(v);
    if (draftKey) setDraft(draftKey, v);
  };
  const [commentType, setCommentType] = useState<CommentType>("note");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (!autoFocus) return;
    const el = ref.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, [autoFocus]);
  const cancel = () => {
    if (draftKey) clearDraft(draftKey);
    onCancel?.();
  };
  const submit = async () => {
    if (!body.trim() || busy) return;
    setBusy(true);
    setErr(null);
    try {
      await onSubmit(body, showType ? commentType : undefined);
      setBodyState("");
      if (draftKey) clearDraft(draftKey);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="composer">
      {showType && <label>Feedback type <select aria-label="Feedback type" value={commentType} onChange={(e) => setCommentType(e.target.value as CommentType)}>{["note", "blocker", "question", "suggestion", "change-approach"].map((t) => <option key={t} value={t}>{t.replace("-", " ")}</option>)}</select></label>}
      <textarea
        ref={ref}
        value={body}
        placeholder={placeholder}
        rows={3}
        onChange={(e) => setBody(e.target.value)}
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") void submit();
          if (e.key === "Escape") cancel();
        }}
      />
      {err && <div className="error">{err}</div>}
      <div className="composer-actions">
        <span className="muted">Markdown, ⌘↩ to send</span>
        <span className="spacer" />
        {onCancel && (
          <button className="btn ghost" onClick={cancel} type="button">
            Cancel
          </button>
        )}
        <button className="btn primary" onClick={() => void submit()} disabled={busy || !body.trim()} type="button">
          {submitLabel}
        </button>
      </div>
    </div>
  );
}
