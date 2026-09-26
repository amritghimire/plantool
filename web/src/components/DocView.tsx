import { useEffect, useMemo, useState, type ComponentProps, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Element } from "hast";
import { collectBlocks, blockForLine, type Block } from "../lib/blocks";
import type { Comment, DocKind, DocResponse } from "../types";
import { Composer } from "./Composer";
import { CopyButton } from "./CopyButton";
import { Mermaid } from "./Markdown";
import { Thread, type ThreadActions } from "./Thread";

export type ViewMode = "rendered" | "source";

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
  const [composing, setComposing] = useState<number | null>(null);
  const blocks = useMemo(() => (p.doc ? collectBlocks(p.doc.content) : []), [p.doc]);
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
    if (!p.target) return;
    const line = p.target.line;
    const el = findLineElement(line);
    if (el) {
      el.scrollIntoView({ block: "center", behavior: "smooth" });
      el.classList.add("flash");
      const t = window.setTimeout(() => el.classList.remove("flash"), 1600);
      return () => window.clearTimeout(t);
    }
  }, [p.target]);

  useEffect(() => {
    if (!p.highlightComment) return;
    const el = document.getElementById(`c-${p.highlightComment}`);
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [p.highlightComment]);

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

  const wrap = (tag: string, node: Element | undefined, inner: ReactNode, extraClass = ""): ReactNode => {
    const type = TAG_TYPE[tag];
    const start = node?.position?.start.line;
    if (!type || !start) return inner;
    const block = blocks.find((b) => b.type === type && b.start === start);
    if (!block) return inner;
    const attach = attachments.map.get(`${type}:${start}`);
    const gutter = (
      <button className="gutter" title={`Comment on line ${start}`} onClick={() => setComposing(composing === start ? null : start)} type="button">
        +
      </button>
    );
    const extras = (
      <>
        {attach?.threads.map((t) => (
          <Thread key={t.root.id} root={t.root} replies={t.replies} actions={p.actions} highlighted={p.highlightComment === t.root.id} />
        ))}
        {composing === start && <Composer placeholder={`Comment on line ${start}…`} onCancel={() => setComposing(null)} onSubmit={(b) => submit(start, b)} />}
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
      const start = node?.position?.start.line;
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

  return (
    <div className="doc">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {p.doc.content}
      </ReactMarkdown>
      {attachments.orphans.threads.length > 0 && (
        <div className="orphans">
          <h4 className="muted">Comments on lines that no longer exist</h4>
          {attachments.orphans.threads.map((t) => (
            <Thread key={t.root.id} root={t.root} replies={t.replies} actions={p.actions} highlighted={p.highlightComment === t.root.id} />
          ))}
        </div>
      )}
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
