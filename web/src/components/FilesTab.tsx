import { useEffect, useState } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { ArrowClockwiseIcon, FileIcon, FileImageIcon, FileTextIcon } from "@phosphor-icons/react";
import { api } from "../api";
import { fileUrl, sessionFilePath, type SessionFile } from "../lib/files";
import { CopyButton } from "./CopyButton";

export function fileLinkComponents(sessionKey: string, onOpenFile: (path: string) => void, base = ""): Components {
  const resolve = (href: string | undefined) => sessionFilePath(href, base);
  return {
    img: ({ node: _node, src, alt, ...rest }) => {
      const path = resolve(typeof src === "string" ? src : undefined);
      return <img {...rest} src={path ? fileUrl(sessionKey, path) : src} alt={alt ?? ""} loading="lazy" />;
    },
    a: ({ node: _node, href, children, ...rest }) => {
      const path = resolve(href);
      if (!path) return <a {...rest} href={href} target={href?.startsWith("#") ? undefined : "_blank"} rel="noreferrer">{children}</a>;
      return <a {...rest} href={fileUrl(sessionKey, path)} onClick={(event) => { event.preventDefault(); onOpenFile(path); }}>{children}</a>;
    },
  };
}

function size(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function Viewer({ sessionKey, file, onOpenFile }: { sessionKey: string; file: SessionFile; onOpenFile: (path: string) => void }) {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const readable = file.kind === "markdown" || file.kind === "text";
  useEffect(() => {
    setText(null);
    setError(null);
    if (!readable) return;
    let cancelled = false;
    api.fileText(sessionKey, file.path).then((t) => !cancelled && setText(t), (e: Error) => !cancelled && setError(e.message));
    return () => {
      cancelled = true;
    };
  }, [sessionKey, file.path, readable]);
  const url = fileUrl(sessionKey, file.path);
  const dir = file.path.includes("/") ? file.path.slice(0, file.path.lastIndexOf("/")) : "";
  return (
    <section className="file-viewer" aria-label={file.path}>
      <header className="file-viewer-head">
        <code>{file.path}</code>
        <span className="muted small">{size(file.size)}</span>
        <span className="spacer" />
        <a className="btn ghost small" href={url} target="_blank" rel="noreferrer">Open raw</a>
      </header>
      {error && <p className="banner" role="status">This file could not be read: {error}</p>}
      {file.kind === "image" && <img className="file-image" src={url} alt={file.path} />}
      {readable && text === null && !error && <p className="muted" role="status">Loading…</p>}
      {file.kind === "markdown" && text !== null && (
        <article className="doc">
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={fileLinkComponents(sessionKey, onOpenFile, dir)}>{text}</ReactMarkdown>
        </article>
      )}
      {file.kind === "text" && text !== null && <pre className="file-text">{text}</pre>}
      {file.kind === "other" && <p className="muted">plantool cannot preview this file type. Use Open raw to download it.</p>}
    </section>
  );
}

export function FilesTab({ sessionKey, selected, onSelect }: { sessionKey: string; selected: string | null; onSelect: (path: string) => void }) {
  const [files, setFiles] = useState<SessionFile[] | null>(null);
  const [dir, setDir] = useState("");
  const [error, setError] = useState<string | null>(null);
  const load = () => {
    setError(null);
    api.files(sessionKey).then((r) => { setFiles(r.files); setDir(r.dir); }, (e: Error) => setError(e.message));
  };
  useEffect(load, [sessionKey]);
  useEffect(() => {
    if (selected && files && !files.some((f) => f.path === selected)) load();
  }, [selected]);
  const current = files?.find((f) => f.path === selected) ?? null;
  return (
    <div className="files-tab">
      <nav className="files-list" aria-label="Session files">
        <div className="files-list-head">
          <span className="muted small">Session folder</span>
          <button className="btn ghost small" type="button" onClick={load} aria-label="Refresh file list"><ArrowClockwiseIcon size={15} aria-hidden="true" /></button>
        </div>
        {error && <p className="banner" role="status">{error}</p>}
        {files === null && !error && <p className="muted" role="status">Loading…</p>}
        {files?.length === 0 && (
          <div className="files-empty">
            <p>No other files yet. Agents can save screenshots and notes in the session folder and link them from the plan.</p>
            {dir && <CopyButton text={dir} label="Copy folder path" />}
          </div>
        )}
        {files?.map((f) => (
          <button key={f.path} type="button" className={`files-item ${f.path === selected ? "active" : ""}`} aria-current={f.path === selected ? "page" : undefined} onClick={() => onSelect(f.path)}>
            {f.kind === "image" ? <FileImageIcon size={16} aria-hidden="true" /> : f.kind === "other" ? <FileIcon size={16} aria-hidden="true" /> : <FileTextIcon size={16} aria-hidden="true" />}
            <span>{f.path}</span>
          </button>
        ))}
      </nav>
      {current ? <Viewer sessionKey={sessionKey} file={current} onOpenFile={onSelect} /> : files && files.length > 0 && <p className="muted files-hint">Choose a file to open it.</p>}
    </div>
  );
}
