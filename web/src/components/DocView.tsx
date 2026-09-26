import { useEffect, useMemo, useState, type ComponentProps, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Element } from "hast";
import { collectBlocks, blockForLine, type Block } from "../lib/blocks";
import { splitSlides, slideForLine } from "../lib/slides";
import { getDraft, moveDraft } from "../lib/drafts";
import type { Comment, DocKind, DocResponse } from "../types";
import { Composer } from "./Composer";
import { CopyButton } from "./CopyButton";
import { Mermaid } from "./Markdown";
import { Thread, type ThreadActions } from "./Thread";

export type ViewMode = "rendered" | "slides" | "source";

interface Props {
  kind: DocKind;
  doc: DocResponse | null;
  path: string | null;
  comments: Comment[];
  actions: ThreadActions;
  onAdd: (kind: DocKind, line: number, body: string) => Promise<void>;
  mode: ViewMode;
  target: { line: number; nonce: number } | null;
  highlightComment: string | null;
  showResolved: boolean;
  prompt?: string | null;
  onStartRun?: () => void;
}

interface Attach {
  threads: { root: Comment; replies: Comment[] }[];
}

const TAG_TYPE: Record<string, string> = { h1: "heading", h2: "heading", h3: "heading", h4: "heading", h5: "heading", h6: "heading", p: "paragraph", li: "listItem", pre: "code", table: "table", hr: "thematicBreak" };

export function DocView(p: Props) {
  const [composing, setComposingState] = useState<number | null>(null);
  const [composingText, setComposingText] = useState<string | null>(null);
  const [movedNote, setMovedNote] = useState<string | null>(null);
  const setComposing = (line: number | null) => {
    setComposingState(line);
    setComposingText(line !== null && p.doc ? (p.doc.content.split("\n")[line - 1] ?? null) : null);
    setMovedNote(null);
  };
  const [slide, setSlide] = useState(0);
  const blocks = useMemo(() => (p.doc ? collectBlocks(p.doc.content) : []), [p.doc]);
  const slides = useMemo(() => (p.doc ? splitSlides(p.doc.content, p.doc.headings) : []), [p.doc]);
  const slidesOn = p.mode === "slides" && slides.length > 0;
  const current = Math.min(slide, Math.max(slides.length - 1, 0));
  const threads = useMemo(() => buildThreads(p.comments, p.showResolved), [p.comments, p.showResolved]);
  const attachments = useMemo(() => {
    const map = new Map<string, Attach>();
    const orphans: Attach = { threads: [] };
    for (const t of threads) {
      const b = blockForLine(blocks, t.root.anchor.line);
      if (!b) {
        orphans.threads.push(t);
        continue;
      }
      const k = `${b.type}:${b.start}`;
      const a = map.get(k) ?? { threads: [] };
      a.threads.push(t);
      map.set(k, a);
    }
    return { map, orphans };
  }, [threads, blocks]);

  useEffect(() => {
    setSlide(0);
  }, [p.kind]);

  // The agent rewrote the document while a comment was being typed: follow the line the
  // composer was opened on, the way saved comments are re-anchored.
  useEffect(() => {
    if (composing === null || composingText === null || !p.doc) return;
    const lines = p.doc.content.split("\n");
    if (lines[composing - 1] === composingText) return;
    const matches = lines.map((l, i) => (l === composingText ? i + 1 : 0)).filter(Boolean);
    const draftFrom = `new:${p.kind}:${composing}`;
    if (matches.length >= 1) {
      const next = matches.reduce((best, l) => (Math.abs(l - composing) < Math.abs(best - composing) ? l : best), matches[0]);
      moveDraft(draftFrom, `new:${p.kind}:${next}`);
      setComposingState(next);
      setMovedNote(next === composing ? null : `The document changed; your unsent comment moved from line ${composing} to line ${next}.`);
    } else {
      const clamped = Math.min(composing, lines.length);
      if (clamped !== composing) moveDraft(draftFrom, `new:${p.kind}:${clamped}`);
      setComposingState(clamped);
      setMovedNote(`The document changed and the line you were commenting on is gone. Your text is kept on line ${clamped}; move it or send it anyway.`);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p.doc?.sha]);

  useEffect(() => {
    if (!p.target || !slidesOn) return;
    setSlide(slideForLine(slides, p.target.line));
  }, [p.target, slidesOn, slides]);

  useEffect(() => {
    if (!p.highlightComment || !slidesOn) return;
    const c = p.comments.find((x) => x.id === p.highlightComment);
    if (c) setSlide(slideForLine(slides, c.anchor.line));
  }, [p.highlightComment, slidesOn, slides, p.comments]);

  useEffect(() => {
    if (!slidesOn) return;
    const scroller = document.querySelector(".content") ?? document.scrollingElement;
    scroller?.scrollTo({ top: 0 });
  }, [slidesOn, current]);

  useEffect(() => {
    if (!slidesOn) return;
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "TEXTAREA" || t.tagName === "INPUT" || t.tagName === "SELECT" || t.isContentEditable)) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "ArrowRight" || e.key === "PageDown" || e.key === " " || e.key === "j") {
        e.preventDefault();
        setSlide((s) => Math.min(s + 1, slides.length - 1));
      } else if (e.key === "ArrowLeft" || e.key === "PageUp" || e.key === "k") {
        e.preventDefault();
        setSlide((s) => Math.max(s - 1, 0));
      } else if (e.key === "Home") {
        setSlide(0);
      } else if (e.key === "End") {
        setSlide(slides.length - 1);
      } else if (e.key === "f") {
        const el = document.querySelector(".slides");
        if (el && !document.fullscreenElement) void el.requestFullscreen?.();
        else if (document.fullscreenElement) void document.exitFullscreen?.();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [slidesOn, slides.length]);

  useEffect(() => {
    if (!p.target) return;
    const line = p.target.line;
    const el = findLineElement(line);
    if (el) {
      el.scrollIntoView({ block: "center", behavior: "smooth" });
      el.classList.add("flash");
      const t = window.setTimeout(() => el.classList.remove("flash"), 1600);
      return () => window.clearTimeout(t);
    }
  }, [p.target, current]);

  useEffect(() => {
    if (!p.highlightComment) return;
    const el = document.getElementById(`c-${p.highlightComment}`);
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [p.highlightComment, current]);

  if (!p.doc) {
    return (
      <div className="doc-empty">
        <h3>Waiting for {p.kind}.md</h3>
        <p>The agent writes this document to</p>
        <pre>{p.path ?? "…"}</pre>
        {p.prompt && (
          <div className="prompt-box">
            <div className="prompt-head">
              <span>Paste this into your agent</span>
              <span className="spacer" />
              <CopyButton text={p.prompt} label="Copy prompt" />
              {p.onStartRun && (
                <button className="btn primary small" onClick={p.onStartRun} type="button">
                  Run it here
                </button>
              )}
            </div>
            <pre className="prompt-text">{p.prompt}</pre>
          </div>
        )}
        <p className="muted">
          It appears here the moment the file lands. The prompt tells the agent to run <code>plantool skill</code>; set a brief in the sidebar to say what you want.
        </p>
      </div>
    );
  }

  const submit = async (line: number, body: string) => {
    await p.onAdd(p.kind, line, body);
    setComposing(null);
  };

  if (p.mode === "source") {
    return (
      <SourceView
        content={p.doc.content}
        threads={threads}
        actions={p.actions}
        composing={composing}
        setComposing={setComposing}
        submit={submit}
        highlightComment={p.highlightComment}
      />
    );
  }

  const offset = slidesOn ? slides[current].start - 1 : 0;

  const wrap = (tag: string, node: Element | undefined, inner: ReactNode, extraClass = ""): ReactNode => {
    const type = TAG_TYPE[tag];
    const start = node?.position?.start.line ? node.position.start.line + offset : undefined;
    if (!type || !start) return inner;
    const block = blocks.find((b) => b.type === type && b.start === start);
    if (!block) return inner;
    const attach = attachments.map.get(`${type}:${start}`);
    const hasDraft = composing !== start && getDraft(`new:${p.kind}:${start}`) !== "";
    const gutter = (
      <button className={`gutter ${hasDraft ? "has-draft" : ""}`} title={hasDraft ? `Unsent comment on line ${start}` : `Comment on line ${start}`} onClick={() => setComposing(composing === start ? null : start)} type="button">
        {hasDraft ? "…" : "+"}
      </button>
    );
    const extras = (
      <>
        {attach?.threads.map((t) => (
          <Thread key={t.root.id} root={t.root} replies={t.replies} actions={p.actions} highlighted={p.highlightComment === t.root.id} />
        ))}
        {composing === start && (
          <>
            {movedNote && <div className="banner small">{movedNote}</div>}
            <Composer draftKey={`new:${p.kind}:${start}`} placeholder={`Comment on line ${start}…`} onCancel={() => setComposing(null)} onSubmit={(b) => submit(start, b)} />
          </>
        )}
      </>
    );
    if (tag === "li") {
      return (
        <li className={`blk ${extraClass}`} data-line={start} data-end={block.end}>
          {gutter}
          {inner}
          {extras}
        </li>
      );
    }
    return (
      <div className={`blk ${extraClass}`} data-line={start} data-end={block.end}>
        {gutter}
        {inner}
        {extras}
      </div>
    );
  };

  const heading = (tag: "h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
    const H = tag;
    return ({ node, children, ...rest }: ComponentProps<typeof H> & { node?: Element }) => wrap(tag, node, <H {...rest}>{children}</H>);
  };

  const components: Components = {
    h1: heading("h1"),
    h2: heading("h2"),
    h3: heading("h3"),
    h4: heading("h4"),
    h5: heading("h5"),
    h6: heading("h6"),
    p: ({ node, children, ...rest }) => wrap("p", node, <p {...rest}>{children}</p>),
    li: ({ node, children, ...rest }) => {
      const cls = (rest as { className?: string }).className ?? "";
      const inner = <>{children}</>;
      const type = TAG_TYPE.li;
      const start = node?.position?.start.line ? node.position.start.line + offset : undefined;
      if (!start || !blocks.find((b) => b.type === type && b.start === start)) return <li {...rest}>{children}</li>;
      return wrap("li", node, inner, cls);
    },
    hr: ({ node }) => wrap("hr", node, <hr />),
    table: ({ node, children }) => wrap("table", node, <div className="table-wrap"><table>{children}</table></div>),
    pre: ({ node, children }) => {
      const code = node?.children?.[0];
      const cls = code && code.type === "element" ? ((code.properties?.className as string[] | undefined) ?? []) : [];
      if (cls.includes("language-mermaid") && code && code.type === "element") {
        const text = code.children.map((c) => (c.type === "text" ? c.value : "")).join("");
        return wrap("pre", node, <Mermaid code={text} />);
      }
      return wrap("pre", node, <pre>{children}</pre>);
    },
    input: ({ node: _node, ...rest }) => <input {...rest} disabled className="task" />,
  };

  const orphans = attachments.orphans.threads.length > 0 && (
    <div className="orphans">
      <h4 className="muted">Comments on lines that no longer exist</h4>
      {attachments.orphans.threads.map((t) => (
        <Thread key={t.root.id} root={t.root} replies={t.replies} actions={p.actions} highlighted={p.highlightComment === t.root.id} />
      ))}
    </div>
  );

  if (slidesOn) {
    const s = slides[current];
    const openOn = (sl: { start: number; end: number }) => threads.filter((t) => !t.root.resolved && t.root.anchor.line >= sl.start && t.root.anchor.line <= sl.end).length;
    return (
      <div className="slides">
        <nav className="outline" aria-label="Slides">
          {slides.map((sl) => {
            const n = openOn(sl);
            return (
              <button key={sl.index} className={`outline-item ${sl.index === current ? "active" : ""}`} onClick={() => setSlide(sl.index)} type="button">
                <span className="outline-n">{sl.index + 1}</span>
                <span className="outline-title">{sl.title}</span>
                {n > 0 && <span className="badge">{n}</span>}
              </button>
            );
          })}
        </nav>
        <section className="slide-stage">
          <div className="slide-progress">
            <div className="fill" style={{ width: `${((current + 1) / slides.length) * 100}%` }} />
          </div>
          <div className="slide doc" key={current}>
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
              {s.content}
            </ReactMarkdown>
            {current === slides.length - 1 && orphans}
          </div>
          <footer className="slide-nav">
            <button className="btn ghost" disabled={current === 0} onClick={() => setSlide(current - 1)} type="button" title="← / k">
              ← Previous
            </button>
            <span className="muted small">
              {current + 1} / {slides.length} · {s.title}
            </span>
            <span className="spacer" />
            <span className="muted small hint">← → to move · f fullscreen</span>
            <button className="btn primary" disabled={current === slides.length - 1} onClick={() => setSlide(current + 1)} type="button" title="→ / j / space">
              Next →
            </button>
          </footer>
        </section>
      </div>
    );
  }

  return (
    <div className="doc">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {p.doc.content}
      </ReactMarkdown>
      {orphans}
    </div>
  );
}

export function buildThreads(comments: Comment[], showResolved: boolean) {
  const roots = comments.filter((c) => !c.parent && (showResolved || !c.resolved));
  return roots
    .sort((a, b) => a.anchor.line - b.anchor.line || a.seq - b.seq)
    .map((root) => ({ root, replies: comments.filter((c) => c.parent === root.id).sort((a, b) => a.seq - b.seq) }));
}

function findLineElement(line: number): HTMLElement | null {
  const els = Array.from(document.querySelectorAll<HTMLElement>("[data-line]"));
  let best: HTMLElement | null = null;
  let bestStart = -1;
  for (const el of els) {
    const start = Number(el.dataset.line);
    const end = Number(el.dataset.end ?? start);
    if (start <= line && line <= end && start > bestStart) {
      best = el;
      bestStart = start;
    }
  }
  if (best) return best;
  for (const el of els) {
    const start = Number(el.dataset.line);
    if (start <= line && start > bestStart) {
      best = el;
      bestStart = start;
    }
  }
  return best;
}

function SourceView({ content, threads, actions, composing, setComposing, submit, highlightComment }: {
  content: string;
  threads: { root: Comment; replies: Comment[] }[];
  actions: ThreadActions;
  composing: number | null;
  setComposing: (l: number | null) => void;
  submit: (line: number, body: string) => Promise<void>;
  highlightComment: string | null;
}) {
  const lines = content.split("\n");
  if (lines.length && lines[lines.length - 1] === "") lines.pop();
  const byLine = new Map<number, { root: Comment; replies: Comment[] }[]>();
  for (const t of threads) {
    const l = byLine.get(t.root.anchor.line) ?? [];
    l.push(t);
    byLine.set(t.root.anchor.line, l);
  }
  return (
    <div className="source">
      {lines.map((text, i) => {
        const n = i + 1;
        const ts = byLine.get(n);
        return (
          <div key={n} className="src-line blk" data-line={n} data-end={n}>
            <button className="gutter" onClick={() => setComposing(composing === n ? null : n)} type="button">
              +
            </button>
            <span className="ln">{n}</span>
            <span className="txt">{text || " "}</span>
            {(ts || composing === n) && (
              <div className="src-extras">
                {ts?.map((t) => (
                  <Thread key={t.root.id} root={t.root} replies={t.replies} actions={actions} highlighted={highlightComment === t.root.id} />
                ))}
                {composing === n && <Composer placeholder={`Comment on line ${n}…`} onCancel={() => setComposing(null)} onSubmit={(b) => submit(n, b)} />}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

export type { Block };
