import { useEffect, useRef, useState } from "react";

export function Composer({ placeholder, onSubmit, onCancel, autoFocus = true, submitLabel = "Comment" }: {
  placeholder: string;
  onSubmit: (body: string) => Promise<void>;
  onCancel?: () => void;
  autoFocus?: boolean;
  submitLabel?: string;
}) {
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (autoFocus) ref.current?.focus();
  }, [autoFocus]);
  const submit = async () => {
    if (!body.trim() || busy) return;
    setBusy(true);
    setErr(null);
    try {
      await onSubmit(body);
      setBody("");
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
          if (e.key === "Escape") onCancel?.();
        }}
      />
      {err && <div className="error">{err}</div>}
      <div className="composer-actions">
        <span className="muted">Markdown, ⌘↩ to send</span>
        <span className="spacer" />
        {onCancel && (
          <button className="btn ghost" onClick={onCancel} type="button">
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
