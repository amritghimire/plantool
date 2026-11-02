import { useState } from "react";
import { Modal } from "./Modal";
import { SHORTCUT_GROUPS, useShortcuts } from "../lib/shortcuts";

export function ShortcutsHelp() {
  const [open, setOpen] = useState(false);
  useShortcuts({ "?": () => setOpen(true) });
  if (!open) return null;
  return (
    <Modal title="Keyboard shortcuts" onClose={() => setOpen(false)} className="shortcuts-help">
      <h3>Keyboard shortcuts</h3>
      {SHORTCUT_GROUPS.map((g) => (
        <section key={g.title} aria-label={g.title}>
          <h4>{g.title}</h4>
          <dl>
            {g.keys.map(([keys, what]) => (
              <div key={what}>
                <dt>
                  {keys.map((k, i) => (
                    <span key={k}>
                      {i > 0 && " then "}
                      <kbd>{k}</kbd>
                    </span>
                  ))}
                </dt>
                <dd>{what}</dd>
              </div>
            ))}
          </dl>
        </section>
      ))}
      <p className="muted small">Shortcuts pause while you type or while a dialog is open.</p>
      <div className="button-row dialog-actions">
        <span className="spacer" />
        <button className="btn" type="button" onClick={() => setOpen(false)}>
          Close
        </button>
      </div>
    </Modal>
  );
}
