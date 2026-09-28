import { useEffect, useState } from "react";
import { api } from "../api";
import type { RepoInfo, WorktreeDirSetting } from "../types";

const GLOBAL = "";

export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const [repos, setRepos] = useState<RepoInfo[]>([]);
  const [scope, setScope] = useState<string>(GLOBAL);
  const [setting, setSetting] = useState<WorktreeDirSetting | null>(null);
  const [value, setValue] = useState("");
  const [example, setExample] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const previewRepo = scope || repos[0]?.root;

  useEffect(() => {
    api.repos().then((r) => setRepos(r.repos)).catch((e: Error) => setErr(e.message));
  }, []);

  useEffect(() => {
    let active = true;
    setSaved(false);
    api.worktreeDir(previewRepo)
      .then((s) => {
        if (!active) return;
        setSetting(s);
        setValue((scope ? s.repo?.value : s.global) ?? "");
        setErr(null);
      })
      .catch((e: Error) => active && setErr(e.message));
    return () => {
      active = false;
    };
  }, [scope, previewRepo]);

  const inherited = setting ? (scope ? setting.global ?? setting.default : setting.default) : "";
  const template = value.trim() || inherited;

  useEffect(() => {
    if (!previewRepo || !template) {
      setExample(null);
      return;
    }
    let active = true;
    const timer = window.setTimeout(() => {
      api.worktreeDir(previewRepo, template)
        .then((s) => active && setExample(s.example))
        .catch(() => active && setExample(null));
    }, 200);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [previewRepo, template]);

  const save = async (next: string | null) => {
    setBusy(true);
    setErr(null);
    try {
      const s = await api.setWorktreeDir(scope ? "repo" : "global", next, previewRepo);
      setSetting(s);
      setValue((scope ? s.repo?.value : s.global) ?? "");
      setSaved(true);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>Settings</h3>
        <label>
          Worktree location <span className="muted small">(where session worktrees are created)</span>
          <select value={scope} onChange={(e) => setScope(e.target.value)}>
            <option value={GLOBAL}>All repositories (git config --global)</option>
            {repos.map((r) => (
              <option key={r.root} value={r.root}>
                {r.root} only
              </option>
            ))}
          </select>
        </label>
        <label>
          Path
          <input value={value} placeholder={inherited} onChange={(e) => { setValue(e.target.value); setSaved(false); }} />
          <span className="muted small">
            <code>{"{repo}"}</code> is the repository folder name and <code>{"{slug}"}</code> the session slug. Relative paths start at the repository; <code>~/</code> is your home. Leave empty to use {scope ? "the global setting" : "the default"} (<code>{inherited}</code>).
          </span>
        </label>
        {example && (
          <div className="small">
            <span className="muted">New worktrees go to</span> <code>{example}</code>
          </div>
        )}
        {setting && <div className="muted small">Stored as <code>{setting.key}</code>; the CLI flags <code>--worktree-dir</code> and <code>--dir</code> still win.</div>}
        {err && <div className="error">{err}</div>}
        <div className="composer-actions">
          {saved && <span className="muted small">Saved</span>}
          <span className="spacer" />
          <button className="btn ghost" onClick={onClose} type="button">
            Close
          </button>
          <button className="btn" disabled={busy || !setting || !(scope ? setting.repo?.value : setting.global)} onClick={() => void save(null)} type="button">
            Reset
          </button>
          <button className="btn primary" disabled={busy || !setting} onClick={() => void save(value.trim() || null)} type="button">
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
