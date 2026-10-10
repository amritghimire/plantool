import { useEffect, useReducer, useRef, type ClipboardEvent, type DragEvent } from "react";
import { api } from "../api";
import { getDraft, setDraft } from "../lib/drafts";

const LIMIT = 10 * 1024 * 1024;
type Attachment = { id: string; file: File; preview?: string; status: "uploading" | "ready" | "failed"; path?: string; error?: string; invalid?: boolean };
type Draft = { text: string; attachments: Attachment[]; sending: boolean; error?: string };

export function useAttachmentDraft(sessionKey: string, destination: string) {
  const drafts = useRef(new Map<string, Draft>());
  const key = `attachments:${JSON.stringify([sessionKey, destination])}`;
  const currentKey = useRef(key);
  const generation = useRef(0);
  if (currentKey.current !== key) generation.current += 1;
  currentKey.current = key;
  const [, update] = useReducer((n: number) => n + 1, 0);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      for (const draft of drafts.current.values()) for (const item of draft.attachments) if (item.preview) URL.revokeObjectURL(item.preview);
    };
  }, []);
  let draft = drafts.current.get(key);
  if (!draft) {
    draft = { text: getDraft(key), attachments: [], sending: false };
    drafts.current.set(key, draft);
  }
  const selected = draft;
  const refresh = () => { if (mounted.current) update(); };
  const setText = (text: string) => { selected.text = text; setDraft(key, text); refresh(); };
  const upload = async (item: Attachment) => {
    item.status = "uploading";
    item.error = undefined;
    refresh();
    try {
      const result = await api.uploadAttachment(sessionKey, item.file);
      if (!mounted.current || !selected.attachments.includes(item)) return;
      item.path = result.path;
      item.status = "ready";
    } catch (e) {
      if (!mounted.current || !selected.attachments.includes(item)) return;
      item.status = "failed";
      item.error = (e as Error).message;
    }
    refresh();
  };
  const addFiles = (files: File[]) => {
    for (const file of files) {
      const invalid = file.size === 0 || file.size > LIMIT;
      const item: Attachment = { id: crypto.randomUUID(), file, invalid, status: invalid ? "failed" : "uploading", error: invalid ? "Choose a file between 1 byte and 10 MB." : undefined };
      if (!invalid && file.type.startsWith("image/")) item.preview = URL.createObjectURL(file);
      selected.attachments.push(item);
      if (!invalid) void upload(item);
    }
    refresh();
  };
  const remove = (id: string) => {
    const item = selected.attachments.find((a) => a.id === id);
    if (item?.preview) URL.revokeObjectURL(item.preview);
    selected.attachments = selected.attachments.filter((a) => a.id !== id);
    refresh();
  };
  const canSend = !selected.sending && !!(selected.text.trim() || selected.attachments.length) && selected.attachments.every((a) => a.status === "ready");
  const send = async (onSend: (message: string) => Promise<void>) => {
    if (selected.sending || !(selected.text.trim() || selected.attachments.length) || selected.attachments.some((a) => a.status !== "ready")) return false;
    const sentGeneration = generation.current;
    const text = selected.text;
    const files = [...selected.attachments];
    const message = [text.trim(), ...files.map((a) => `Attached file: ${a.path}`)].filter(Boolean).join("\n\n");
    selected.sending = true;
    selected.error = undefined;
    refresh();
    try {
      await onSend(message);
      if (selected.text === text) { selected.text = ""; setDraft(key, ""); }
      for (const item of files) if (item.preview) URL.revokeObjectURL(item.preview);
      selected.attachments = selected.attachments.filter((a) => !files.includes(a));
      return mounted.current && generation.current === sentGeneration && currentKey.current === key && !selected.text && !selected.attachments.length;
    } catch (e) {
      selected.error = (e as Error).message;
      return false;
    } finally {
      selected.sending = false;
      refresh();
    }
  };
  return { draft: selected, setText, addFiles, remove, retry: (id: string) => { const item = selected.attachments.find((a) => a.id === id); if (item && !item.invalid && item.status === "failed") void upload(item); }, canSend, send };
}

export type AttachmentDraft = ReturnType<typeof useAttachmentDraft>;

export function AttachmentComposer({ composer, placeholder, label, rows = 2, onSend, disabled = false, autoFocus = false }: { composer: AttachmentDraft; placeholder: string; label: string; rows?: number; onSend: () => void; disabled?: boolean; autoFocus?: boolean }) {
  const picker = useRef<HTMLInputElement>(null);
  const paste = (event: ClipboardEvent<HTMLTextAreaElement>) => {
    const files = Array.from(event.clipboardData.files);
    if (!files.length) return;
    composer.addFiles(files);
    if (!event.clipboardData.getData("text/plain")) event.preventDefault();
  };
  const drop = (event: DragEvent<HTMLDivElement>) => {
    if (!event.dataTransfer.types.includes("Files")) return;
    event.preventDefault();
    composer.addFiles(Array.from(event.dataTransfer.files));
  };
  return <div className="attachment-composer" onDragOver={(event) => { if (event.dataTransfer.types.includes("Files")) event.preventDefault(); }} onDrop={drop}>
    <textarea autoFocus={autoFocus} rows={rows} value={composer.draft.text} placeholder={placeholder} aria-label={label} onChange={(event) => composer.setText(event.target.value)} onPaste={paste} onKeyDown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === "Enter") { event.preventDefault(); if (composer.canSend && !disabled) onSend(); } }} />
    {composer.draft.attachments.length > 0 && <ul className="attachment-drafts" aria-label="Attachments">{composer.draft.attachments.map((item) => <li key={item.id}>
      {item.preview && <img src={item.preview} alt={`Preview of ${item.file.name}`} />}
      <div className="attachment-info"><strong>{item.file.name}</strong><span className="muted small">{formatSize(item.file.size)}</span><span className="small" role="status">{item.status === "uploading" ? "Uploading…" : item.status === "ready" ? "Uploaded" : "Upload failed"}</span>{item.error && <span className="error small" role="alert">{item.error}</span>}</div>
      <div className="button-row">{item.status === "failed" && !item.invalid && <button className="btn ghost small" type="button" onClick={() => composer.retry(item.id)}>Retry {item.file.name}</button>}<button className="btn ghost small" type="button" aria-label={`Remove ${item.file.name}`} onClick={() => composer.remove(item.id)}>Remove</button></div>
    </li>)}</ul>}
    <div className="button-row"><input ref={picker} type="file" multiple hidden aria-label="Attach files" onChange={(event) => { composer.addFiles(Array.from(event.target.files ?? [])); event.target.value = ""; }} /><button className="btn ghost small" type="button" onClick={() => picker.current?.click()}>＋ Attach files</button><span className="muted small">Paste images or drop files · up to 10 MB each</span></div>
    {composer.draft.error && <p className="error" role="alert">{composer.draft.error}</p>}
  </div>;
}

function formatSize(size: number) {
  return size < 1024 ? `${size} bytes` : size < 1024 * 1024 ? `${(size / 1024).toFixed(1)} KB` : `${(size / (1024 * 1024)).toFixed(1)} MB`;
}
