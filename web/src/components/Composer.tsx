import { useEffect, useRef, useState } from "react";
import { clearDraft, getDraft, setDraft } from "../lib/drafts";

export function Composer({ placeholder, onSubmit, onCancel, autoFocus = true, submitLabel = "Comment", draftKey }: {
  placeholder: string;
  onSubmit: (body: string) => Promise<void>;
  onCancel?: () => void;
  autoFocus?: boolean;
  submitLabel?: string;
  /** Keeps the text across re-renders and document refreshes. */
  draftKey?: string;
}) {
  const [body, setBodyState] = useState(() => (draftKey ? getDraft(draftKey) : ""));
  const setBody = (v: string) => {
    setBodyState(v);
    if (draftKey) setDraft(draftKey, v);
  };
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
      await onSubmit(body);
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
