import { useEffect, useMemo, useState, type ComponentProps, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { ArrowLeftIcon, ArrowRightIcon, NotePencilIcon, PlusIcon } from "@phosphor-icons/react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Element } from "hast";
import { collectBlocks, blockForLine, type Block } from "../lib/blocks";
import { splitSlides, slideForLine } from "../lib/slides";
import { getDraft, moveDraft } from "../lib/drafts";
import type { AnchorScope, CommentType, Comment, DocKind, DocResponse } from "../types";
import { Composer } from "./Composer";
import { CopyButton } from "./CopyButton";
import { Mermaid } from "./Markdown";
import { RefStack } from "./RefNumeral";
import { referenceNumerals } from "../lib/comments";
import { fileLinkComponents } from "./FilesTab";

export type ViewMode = "rendered" | "slides" | "source";

interface Props {
  kind: DocKind;
  doc: DocResponse | null;
  path: string | null;
  comments: Comment[];
  onAdd: (kind: DocKind, line: number, body: string, type?: CommentType, scope?: AnchorScope) => Promise<void>;
  mode: ViewMode;
  target: { line: number; nonce: number; compose?: boolean } | null;
  highlightComment: string | null;
  onOpenThread: (c: Comment) => void;
  prompt?: string | null;
  onStartRun?: () => void;
  numerals?: Map<string, number>;
  onToggleTask?: (line: number, checked: boolean) => Promise<void>;
  sessionKey?: string;
  onOpenFile?: (path: string) => void;
}

interface Mark {
  open: Comment[];
  ids: Set<string>;
}

const NUMBERED = new Set(["paragraph", "listItem", "code", "table"]);

const TAG_TYPE: Record<string, string> = { h1: "heading", h2: "heading", h3: "heading", h4: "heading", h5: "heading", h6: "heading", p: "paragraph", li: "listItem", pre: "code", table: "table", hr: "thematicBreak" };

export function DocView(p: Props) {
  const [composing, setComposingState] = useState<number | null>(null);
  const [composingText, setComposingText] = useState<string | null>(null);
  const [movedNote, setMovedNote] = useState<string | null>(null);
  const setComposing = (line: number | null) => {
    if (line === null && composing !== null) requestAnimationFrame(() => findLineElement(composing)?.focus());
    setComposingState(line);
    setComposingText(line !== null && p.doc ? (p.doc.content.split("\n")[line - 1] ?? null) : null);
    setMovedNote(null);
  };
  const [scope, setScope] = useState<AnchorScope>("line");
  const [slide, setSlide] = useState(0);
  const blocks = useMemo(() => (p.doc ? collectBlocks(p.doc.content) : []), [p.doc]);
  const slides = useMemo(() => (p.doc ? splitSlides(p.doc.content, p.doc.headings) : []), [p.doc]);
  const slidesOn = p.mode === "slides" && slides.length > 0;
  const current = Math.min(slide, Math.max(slides.length - 1, 0));
  const roots = useMemo(() => p.comments.filter((c) => !c.parent).sort((a, b) => a.anchor.line - b.anchor.line || a.seq - b.seq), [p.comments]);
  const marks = useMemo(() => {
    const map = new Map<string, Mark>();
    for (const c of roots) {
      const b = blockForLine(blocks, c.anchor.line);
      if (!b) continue;
      const k = `${b.type}:${b.start}`;
      const m = map.get(k) ?? { open: [], ids: new Set<string>() };
      m.ids.add(c.id);
      if (!c.resolved) m.open.push(c);
      map.set(k, m);
    }
    return map;
  }, [roots, blocks]);
  const numerals = useMemo(() => p.numerals ?? referenceNumerals(p.comments), [p.numerals, p.comments]);
  const numbers = useMemo(() => {
    const map = new Map<string, number>();
    let n = 0;
    for (const b of blocks) if (NUMBERED.has(b.type)) map.set(`${b.type}:${b.start}`, ++n);
    return map;
  }, [blocks]);

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
    if (p.target?.compose) { setScope("section"); setComposing(p.target.line); }
  }, [p.target]);

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
      el.scrollIntoView({ block: "center", behavior: window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
      el.classList.add("flash");
      const t = window.setTimeout(() => el.classList.remove("flash"), 1600);
      return () => window.clearTimeout(t);
    }
  }, [p.target, current]);

  if (!p.doc) {
    return (
      <div className="doc-empty">
        <h2>Waiting for {p.kind}.md</h2>
        <p>
          The agent writes it to <code className="doc-empty-path">{p.path ?? "…"}</code>. It appears here the moment the file lands.
        </p>
        {p.prompt && (
          <div className="prompt-box">
            <CopyButton text={p.prompt} label="Copy prompt" className="btn prompt-copy" />
            <details className="disclosure">
              <summary>Show the prompt</summary>
              <pre className="prompt-text">{p.prompt}</pre>
            </details>
          </div>
        )}
      </div>
    );
  }

  const submit = async (line: number, body: string, type?: CommentType) => {
    await p.onAdd(p.kind, line, body, type, scope);
    setComposing(null);
    setScope("line");
  };

  const documentAction = <button className="link doc-action" type="button" onClick={() => { setScope("document"); setComposing(1); }}>Comment on document</button>;
  const documentComposer = composing === 1 && scope === "document" && <Composer showType draftKey={`new:${p.kind}:document`} placeholder="Comment on the whole document…" onCancel={() => setComposing(null)} onSubmit={(b, t) => submit(1, b, t)} />;
  const documentComments = <div className="document-comments">
    {documentAction}
    {documentComposer}
  </div>;
  const lines = p.doc.content.split("\n");
  const titleStart = slidesOn ? undefined : blocks.find((b) => b.type === "heading" && /^#\s/.test(lines[b.start - 1] ?? ""))?.start;

  if (p.mode === "source") {
    return (
      <div className="source-document">{documentComments}<SourceView
        content={p.doc.content}
        roots={roots}
        numerals={numerals}
        onOpenThread={p.onOpenThread}
        composing={scope === "document" ? null : composing}
        setComposing={(line) => { setScope("line"); setComposing(line); }}
        submit={submit}
        onSection={(line) => { setScope("section"); setComposing(line); }}
        highlightComment={p.highlightComment}
      /></div>
    );
  }

  const offset = slidesOn ? slides[current].start - 1 : 0;

  const blockKeys = (e: ReactKeyboardEvent<HTMLElement>, start: number) => {
    if (e.target !== e.currentTarget) return;
    if (e.key === "c" || e.key === "Enter") { e.preventDefault(); setScope("line"); setComposing(start); }
    if (["ArrowDown", "ArrowUp", "j", "k"].includes(e.key)) {
      e.preventDefault();
      const items = Array.from(document.querySelectorAll<HTMLElement>(".blk[tabindex]"));
      const index = items.indexOf(e.currentTarget);
      items[index + (["ArrowDown", "j"].includes(e.key) ? 1 : -1)]?.focus();
    }
  };

  const wrap = (tag: string, node: Element | undefined, inner: ReactNode, extraClass = ""): ReactNode => {
    const type = TAG_TYPE[tag];
    const start = node?.position?.start.line ? node.position.start.line + offset : undefined;
    if (!type || !start) return inner;
    const block = blocks.find((b) => b.type === type && b.start === start);
    if (!block) return inner;
    const key = `${type}:${start}`;
    const mark = marks.get(key);
    const num = slidesOn ? undefined : numbers.get(key);
    const selected = !!p.highlightComment && !!mark?.ids.has(p.highlightComment);
    const hasDraft = composing !== start && getDraft(`new:${p.kind}:${start}`) !== "";
    const where = num ? `paragraph ${num}` : type === "heading" ? `the section at line ${start}` : `line ${start}`;
    const open = mark?.open ?? [];
    const rails = (
      <>
        {num !== undefined && <span className="para-n" aria-hidden="true">[{String(num).padStart(4, "0")}]</span>}
        <span className="blk-margin">
          <RefStack open={open} numerals={numerals} where={where} selected={p.highlightComment} onOpen={p.onOpenThread} />
          <button className={`gutter ${hasDraft ? "has-draft" : ""}`} title={hasDraft ? `Unsent comment on line ${start}` : `Comment on line ${start}`} aria-label={hasDraft ? `Unsent comment on line ${start}` : `Comment on line ${start}`} onClick={() => { setScope("line"); setComposing(composing === start ? null : start); }} type="button">
            {hasDraft ? <NotePencilIcon size={14} aria-hidden="true" /> : <PlusIcon size={14} aria-hidden="true" />}
          </button>
        </span>
      </>
    );
    const composer = composing === start && scope !== "document" && (
      <>
        {movedNote && <div className="banner small">{movedNote}</div>}
        <Composer showType draftKey={`new:${p.kind}:${start}`} placeholder={`Comment on line ${start}…`} onCancel={() => setComposing(null)} onSubmit={(b, t) => submit(start, b, t)} />
      </>
    );
    const cls = `blk blk-${tag} ${selected ? "selected" : ""} ${open.length ? "has-comments" : ""} ${extraClass}`;
    const stack = open.length > 1 ? ({ "--stack": Math.min(open.length, 4) - 1 } as CSSProperties) : undefined;
    if (tag === "li") {
      return (
        <li className={cls} style={stack} tabIndex={0} onKeyDown={(e) => blockKeys(e, start)} data-line={start} data-end={block.end}>
          {rails}
          {inner}
          {composer}
        </li>
      );
    }
    return (
      <div className={cls} style={stack} tabIndex={0} onKeyDown={(e) => blockKeys(e, start)} data-line={start} data-end={block.end}>
        {rails}
        {start === titleStart ? <div className="heading-row title-row">{inner}{documentAction}</div> : type === "heading" ? <div className="heading-row">{inner}<button className="link section-action" onClick={() => { setScope("section"); setComposing(start); }} type="button">Comment on section</button></div> : inner}
        {start === titleStart && documentComposer}
        {composer}
      </div>
    );
  };

  const heading = (tag: "h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
    const H = tag;
    return ({ node, children, ...rest }: ComponentProps<typeof H> & { node?: Element }) => wrap(tag, node, <H {...rest}>{children}</H>);
  };

  const components: Components = {
    ...(p.sessionKey && p.onOpenFile ? fileLinkComponents(p.sessionKey, p.onOpenFile) : {}),
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
    input: ({ node: _node, ...rest }) => {
      const toggle = p.onToggleTask;
      if (!toggle || rest.type !== "checkbox")
        return <input {...rest} disabled aria-label={rest.checked ? "Task completed" : "Task not completed"} className="task" />;
      return (
        <input
          {...rest}
          disabled={false}
          className="task editable"
          aria-label={rest.checked ? "Task completed. Untick" : "Task not completed. Tick"}
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
          onChange={(e) => {
            const input = e.currentTarget;
            const line = Number(input.closest<HTMLElement>("li[data-line]")?.dataset.line);
            if (!line) return;
            input.disabled = true;
            void toggle(line, input.checked).finally(() => {
              input.disabled = false;
            });
          }}
        />
      );
    },
  };

  if (slidesOn) {
    const s = slides[current];
    const openOn = (sl: { start: number; end: number }) => roots.filter((c) => !c.resolved && c.anchor.line >= sl.start && c.anchor.line <= sl.end);
    return (
      <div className="slides-document">
        {documentComments}
      <div className="slides">
        <nav className="outline" aria-label="Slides">
          {slides.map((sl) => {
            const open = openOn(sl);
            return (
              <div key={sl.index} className={`outline-row ${sl.index === current ? "active" : ""}`}>
                <button className="outline-item" aria-current={sl.index === current ? "step" : undefined} onClick={() => setSlide(sl.index)} type="button">
                  <span className="outline-n">{sl.index + 1}</span>
                  <span className="outline-title">{sl.title}</span>
                </button>
                {open.length > 0 && (
                  <span className="outline-refs">
                    <RefStack open={open} numerals={numerals} where={`slide ${sl.index + 1}`} selected={p.highlightComment} leader={false} onOpen={p.onOpenThread} />
                  </span>
                )}
              </div>
            );
          })}
        </nav>
        <section className="slide-stage">
          <div className="slide-progress">
            <div className="fill" style={{ width: `${((current + 1) / slides.length) * 100}%` }} />
          </div>
          <div className="slide doc no-numbers" key={current}>
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
              {s.content}
            </ReactMarkdown>
          </div>
          <footer className="slide-nav">
            <button className="btn ghost" disabled={current === 0} onClick={() => setSlide(current - 1)} type="button" title="← / k">
              <ArrowLeftIcon size={14} aria-hidden="true" /> Previous
            </button>
            <span className="muted small">
              {current + 1} / {slides.length} · {s.title}
            </span>
            <span className="spacer" />
            <span className="muted small hint">← → to move · f fullscreen</span>
            <button className="btn" disabled={current === slides.length - 1} onClick={() => setSlide(current + 1)} type="button" title="→ / j / space">
              Next <ArrowRightIcon size={14} aria-hidden="true" />
            </button>
          </footer>
        </section>
      </div>
      </div>
    );
  }

  return (
    <div className="doc">
      {titleStart === undefined && documentComments}
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {p.doc.content}
      </ReactMarkdown>
    </div>
  );
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

function SourceView({ content, roots, numerals, onOpenThread, composing, setComposing, submit, highlightComment, onSection }: {
  content: string;
  roots: Comment[];
  numerals: Map<string, number>;
  onOpenThread: (c: Comment) => void;
  composing: number | null;
  setComposing: (l: number | null) => void;
  onSection: (line: number) => void;
  submit: (line: number, body: string, type?: CommentType) => Promise<void>;
  highlightComment: string | null;
}) {
  const lines = content.split("\n");
  if (lines.length && lines[lines.length - 1] === "") lines.pop();
  const byLine = new Map<number, Comment[]>();
  for (const c of roots) {
    const l = byLine.get(c.anchor.line) ?? [];
    l.push(c);
    byLine.set(c.anchor.line, l);
  }
  return (
    <div className="source">
      {lines.map((text, i) => {
        const n = i + 1;
        const all = byLine.get(n) ?? [];
        const open = all.filter((c) => !c.resolved);
        const selected = all.some((c) => c.id === highlightComment);
        return (
          <div key={n} className={`src-line blk ${selected ? "selected" : ""}`} style={open.length > 1 ? ({ "--stack": Math.min(open.length, 4) - 1 } as CSSProperties) : undefined} data-line={n} data-end={n}>
            <span className="ln">{n}</span>
            <span className="txt">{text || " "}
            {/^(#{1,6})\s/.test(text) && <button type="button" className="link section-action" onClick={() => onSection(n)}>Comment on section</button>}</span>
            <span className="blk-margin">
              <RefStack open={open} numerals={numerals} where={`line ${n}`} selected={highlightComment} onOpen={onOpenThread} />
              <button className="gutter" title={`Comment on line ${n}`} aria-label={`Comment on line ${n}`} onClick={() => setComposing(composing === n ? null : n)} type="button">
                <PlusIcon size={14} aria-hidden="true" />
              </button>
            </span>
            {composing === n && (
              <div className="src-extras">
                <Composer showType placeholder={`Comment on line ${n}…`} onCancel={() => setComposing(null)} onSubmit={(b, t) => submit(n, b, t)} />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

export type { Block };
