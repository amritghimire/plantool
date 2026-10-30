import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { Modal } from "./Modal";
import { useState } from "react";
import type { SessionView } from "../types";

export function DropSessionDialog({
  view,
  busy,
  onClose,
  onDelete,
}: {
  view: SessionView;
  busy: boolean;
  onClose: () => void;
  onDelete: (removeWorktree: boolean) => Promise<void>;
}) {
  const [removeWorktree, setRemoveWorktree] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <Modal
      destructive
      title={`Drop ${view.key}?`}
      busy={busy}
      onClose={onClose}
    >
      {" "}
      <h3 id="drop-session-title">Drop {view.key}?</h3>
      <p id="drop-session-description">
        This deletes the session's documents, comments, and run transcripts. You
        cannot undo it.
      </p>
      {view.session.worktree && (
        <label className="check check-row">
          <input
            type="checkbox"
            checked={removeWorktree}
            disabled={busy}
            onChange={(e) => setRemoveWorktree(e.target.checked)}
          />
          <span>
            Also remove worktree{" "}
            <span className="workspace-path">{view.session.worktree}</span>
          </span>
        </label>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="composer-actions button-row dialog-actions">
        <span className="spacer" />
        <AlertDialog.Cancel asChild>
          <button className="btn ghost" disabled={busy} type="button">
            Cancel
          </button>
        </AlertDialog.Cancel>
        <button
          className="btn danger"
          disabled={busy}
          onClick={() =>
            void onDelete(removeWorktree).catch((e: Error) =>
              setError(e.message),
            )
          }
          type="button"
        >
          Drop session
        </button>
      </div>
    </Modal>
  );
}
