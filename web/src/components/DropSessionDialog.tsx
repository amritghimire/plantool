import { useEffect, useRef, useState } from "react";
import type { SessionView } from "../types";

export function DropSessionDialog({ view, busy, onClose, onDelete }: { view: SessionView; busy: boolean; onClose: () => void; onDelete: (removeWorktree: boolean) => Promise<void> }) {
  const [removeWorktree, setRemoveWorktree] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialogRef.current?.querySelector<HTMLButtonElement>("button[data-cancel]")?.focus();
    return () => previous?.focus();
  }, []);
  return <div className="modal-backdrop" onClick={() => !busy && onClose()}>
    <div ref={dialogRef} className="modal" role="dialog" aria-modal="true" aria-labelledby="drop-session-title" aria-describedby="drop-session-description" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => {
      if (e.key === "Escape" && !busy) onClose();
      if (e.key !== "Tab") return;
      const controls = [...(dialogRef.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)') ?? [])];
      if (e.shiftKey && document.activeElement === controls[0]) { e.preventDefault(); controls[controls.length - 1]?.focus(); }
      else if (!e.shiftKey && document.activeElement === controls[controls.length - 1]) { e.preventDefault(); controls[0]?.focus(); }
    }}>
      <h3 id="drop-session-title">Drop {view.key}?</h3>
      <p id="drop-session-description">This deletes the session's documents, comments, and run transcripts. You cannot undo it.</p>
      {view.session.worktree && <label className="check"><input type="checkbox" checked={removeWorktree} disabled={busy} onChange={(e) => setRemoveWorktree(e.target.checked)} /><span>Also remove worktree <span className="workspace-path">{view.session.worktree}</span></span></label>}
      {error && <p className="error">{error}</p>}
      <div className="composer-actions"><span className="spacer" /><button className="btn ghost" data-cancel disabled={busy} onClick={onClose} type="button">Cancel</button><button className="btn danger" disabled={busy} onClick={() => void onDelete(removeWorktree).catch((e: Error) => setError(e.message))} type="button">Drop session</button></div>
    </div>
  </div>;
}
