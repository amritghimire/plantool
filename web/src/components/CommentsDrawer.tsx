import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { CaretDownIcon, CaretRightIcon, QuestionIcon } from "@phosphor-icons/react";
import type { Comment, DocKind, DocResponse, Run } from "../types";
import { blockForLine, collectBlocks } from "../lib/blocks";
import { anchorLabel, documentOrder, filterThreads, isHumanBlocker, previewOf, referenceNumerals, type CommentFilter } from "../lib/comments";
import { cn } from "../lib/cn";
import { CopyButton } from "./CopyButton";
import { Thread, type ThreadActions } from "./Thread";

const QUICK_REPLIES = [
  "Agreed, go ahead.",
  "No, keep it as it is.",
  "Not now; note it as a follow-up.",
];

const PREVIEW_ELEMENTS = ["p", "strong", "em", "code", "del"];

const FILTERS: [CommentFilter, string][] = [
  ["open", "Open"],
  ["blockers", "Blockers"],
  ["outdated", "Outdated"],
  ["mine", "Mine"],
];

const EMPTY: Record<CommentFilter, string> = {
  open: "No open comments. Hover a paragraph and press + to add one.",
  blockers: "No open blockers.",
  outdated: "Nothing is outdated.",
  mine: "You have no open comments.",
};

export function CommentsDrawer({
  sessionKey,
  comments,
  docs,
  author,
  selected,
  onSelect,
  liveRun,
  reviewPrompt,
  onSendToRun,
  actions,
  busy,
  numerals,
}: {
  sessionKey: string;
  comments: Comment[];
  docs: Partial<Record<DocKind, DocResponse | null>>;
  author: string;
  selected: string | null;
  onSelect: (c: Comment) => void;
  liveRun: Run | null;
  reviewPrompt: string | null;
  onSendToRun: (text: string) => Promise<void>;
  actions: ThreadActions;
  busy: boolean;
  numerals?: Map<string, number>;
}) {
  const refs = useMemo(() => numerals ?? referenceNumerals(comments), [numerals, comments]);
  const [filter, setFilter] = useState<CommentFilter>("open");
  const [showResolved, setShowResolved] = useState(false);
  const blocks = useMemo(() => {
    const map = new Map<DocKind, ReturnType<typeof collectBlocks>>();
    for (const [kind, doc] of Object.entries(docs) as [DocKind, DocResponse | null][])
      if (doc) map.set(kind, collectBlocks(doc.content));
    return map;
  }, [docs]);
  const isOrphan = (c: Comment) => {
    if (c.anchor.code || c.anchor.scope === "document") return false;
    const b = blocks.get(c.doc);
    return !!b && blockForLine(b, c.anchor.line) === null;
  };
  const lists = Object.fromEntries(
    FILTERS.map(([f]) => [f, filterThreads(comments, f, author, isOrphan)]),
  ) as Record<CommentFilter, Comment[]>;
  const shown = lists[filter];
  const resolved = documentOrder(comments.filter((c) => !c.parent && c.resolved));
  const repliesOf = (root: Comment) =>
    comments.filter((c) => c.parent === root.id).sort((a, b) => a.seq - b.seq);
  const askAgent = (c: Comment) =>
    onSendToRun(
      `Answer the comment thread ${c.id} on ${c.doc}.md line ${c.anchor.line} in plantool session ${sessionKey}. Read it with \`plantool session comment context --session ${sessionKey} --id ${c.id}\`, check the code it refers to, and reply on the thread with \`plantool session comment add --session ${sessionKey} --parent ${c.id} --body "…"\`. Answer the question there; only change the document if the thread asks for a change, and then say what changed. Leave the thread open for me to resolve.`,
    );

  useEffect(() => {
    if (!selected) return;
    const c = comments.find((x) => x.id === selected);
    if (!c) return;
    if (c.resolved) setShowResolved(true);
    else if (!lists[filter].some((x) => x.id === selected)) setFilter("open");
    requestAnimationFrame(() =>
      document.querySelector(`[data-thread="${selected}"]`)?.scrollIntoView({ block: "nearest" }),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected]);

  const row = (c: Comment) => (
    <ThreadRow
      key={c.id}
      c={c}
      n={refs.get(c.id)}
      replies={repliesOf(c)}
      selected={c.id === selected}
      outdated={c.anchor.outdated || isOrphan(c)}
      onSelect={() => onSelect(c)}
      actions={actions}
      onAsk={liveRun && !c.resolved ? () => void askAgent(c) : undefined}
      busy={busy}
    />
  );

  return (
    <div className="comments-drawer">
      <div className="cd-filters" role="group" aria-label="Filter comments">
        {FILTERS.map(([f, label]) => (
          <button
            key={f}
            type="button"
            className="chip"
            aria-pressed={filter === f}
            onClick={() => setFilter(f)}
          >
            {label} <span className="num">{lists[f].length}</span>
          </button>
        ))}
      </div>
      <p className="cd-hint">
        <kbd>n</kbd> / <kbd>p</kbd> to step through
      </p>
      {lists.open.length > 0 && reviewPrompt && (
        <div className="comment-tools">
          {liveRun ? (
            <button
              className="btn small"
              disabled={busy}
              onClick={() => void onSendToRun(reviewPrompt)}
              title="Tells the running agent to read the open comments, act on them, reply and resolve"
              type="button"
            >
              Send comments to the running agent
            </button>
          ) : (
            <span className="muted small">No run is live. Paste this into your agent:</span>
          )}
          <CopyButton text={reviewPrompt} label="Copy prompt to act on comments" className="btn small" />
        </div>
      )}
      {shown.length === 0 ? (
        <p className="muted small">{EMPTY[filter]}</p>
      ) : (
        groups(shown).map(([label, items]) => (
          <section key={label} className="cd-group" aria-label={label}>
            <h3 className="cd-group-head">{label}</h3>
            <ul className="cd-list">{items.map(row)}</ul>
          </section>
        ))
      )}
      {resolved.length > 0 && (
        <div className="cd-resolved">
          <button
            className="link resolved-toggle"
            onClick={() => setShowResolved((s) => !s)}
            aria-expanded={showResolved}
            type="button"
          >
            {showResolved ? <CaretDownIcon size={14} aria-hidden="true" /> : <CaretRightIcon size={14} aria-hidden="true" />}
            Resolved <span className="num">{resolved.length}</span>
          </button>
          {showResolved && <ul className="cd-list resolved-list">{resolved.map(row)}</ul>}
        </div>
      )}
    </div>
  );
}

function groups(items: Comment[]): [string, Comment[]][] {
  const out: [string, Comment[]][] = [];
  for (const c of items) {
    const label = c.anchor.code ? "Changes" : `${c.doc}.md`;
    const last = out[out.length - 1];
    if (last && last[0] === label) last[1].push(c);
    else out.push([label, [c]]);
  }
  return out;
}

function ThreadRow({
  c,
  n,
  replies,
  selected,
  outdated,
  onSelect,
  actions,
  onAsk,
  busy,
}: {
  c: Comment;
  n?: number;
  replies: Comment[];
  selected: boolean;
  outdated: boolean;
  onSelect: () => void;
  actions: ThreadActions;
  onAsk?: () => void;
  busy: boolean;
}) {
  const [sending, setSending] = useState(false);
  const quick = async (body: string) => {
    setSending(true);
    try {
      await actions.reply(c, body);
    } finally {
      setSending(false);
    }
  };
  const type = c.type ?? "note";
  const where = anchorLabel(c);
  return (
    <li data-thread={c.id} className={cn("cd-row", selected && "selected", c.resolved && "resolved")}>
      <button
        type="button"
        className="cd-head"
        aria-expanded={selected}
        aria-label={`${n !== undefined ? `${n}, ` : ""}${type.replace("-", " ")} by ${c.author}${c.kind === "agent" ? " (agent)" : ""} on ${c.doc} ${where}`}
        onClick={onSelect}
      >
        {n !== undefined && (
          <span className={cn("cd-ref", isHumanBlocker(c) && "blocker")} aria-hidden="true">
            {n}
          </span>
        )}
        <span className={cn("tag", `tag-${type}`, isHumanBlocker(c) && "human")}>{type.replace("-", " ")}</span>
        <span className={cn("cd-author", c.kind)}>
          {c.author}
          {c.kind === "agent" && c.author !== "agent" && <span className="cd-agent">agent</span>}
        </span>
        <span className="cd-line">{where}</span>
        {outdated && <span className="cd-outdated">outdated</span>}
        <span className="spacer" />
        {replies.length > 0 && (
          <span className="cd-replies">
            {replies.length} {replies.length === 1 ? "reply" : "replies"}
          </span>
        )}
      </button>
      {selected ? (
        <div className="cd-thread">
          <Thread root={c} replies={replies} actions={actions} compact />
          {!c.resolved && (
            <div className="chips" aria-label="Quick replies">
              {QUICK_REPLIES.map((q) => (
                <button key={q} className="chip" disabled={busy || sending} onClick={() => void quick(q)} type="button">
                  {q}
                </button>
              ))}
              {onAsk && (
                <button
                  className="chip"
                  disabled={busy}
                  onClick={onAsk}
                  title="Ask the running agent to answer this thread"
                  type="button"
                >
                  <QuestionIcon size={12} aria-hidden="true" /> Ask the running agent
                </button>
              )}
            </div>
          )}
        </div>
      ) : (
        <div className="cd-preview" onClick={onSelect}>
          <ReactMarkdown remarkPlugins={[remarkGfm]} allowedElements={PREVIEW_ELEMENTS} unwrapDisallowed>
            {previewOf(c.body)}
          </ReactMarkdown>
        </div>
      )}
    </li>
  );
}
