import { cn } from "../lib/cn";
import { isHumanBlocker } from "../lib/comments";
import type { Comment } from "../types";

const SHOWN = 3;

export function RefNumeral({
  n,
  label,
  blocker = false,
  selected = false,
  leader = true,
  onOpen,
}: {
  n: number;
  label: string;
  blocker?: boolean;
  selected?: boolean;
  leader?: boolean;
  onOpen: () => void;
}) {
  return (
    <button
      type="button"
      className={cn("refnum", blocker && "blocker", selected && "selected")}
      aria-label={label}
      title={label}
      onClick={onOpen}
    >
      {leader && (
        <svg className="leader" viewBox="0 0 24 14" preserveAspectRatio="none" aria-hidden="true">
          <path d="M0 12 C 10 12, 12 3, 24 3" fill="none" stroke="currentColor" strokeWidth="1" vectorEffect="non-scaling-stroke" />
        </svg>
      )}
      <span className="refnum-n">{n}</span>
    </button>
  );
}

export function RefStack({
  open,
  numerals,
  where,
  selected,
  leader = true,
  onOpen,
}: {
  open: Comment[];
  numerals: Map<string, number>;
  where: string;
  selected: string | null;
  leader?: boolean;
  onOpen: (c: Comment) => void;
}) {
  if (open.length === 0) return null;
  const shown = open.slice(0, SHOWN);
  const hidden = open.slice(SHOWN);
  const more = hidden.find((c) => c.id === selected) ?? hidden[0];
  return (
    <>
      {shown.map((c) => {
        const n = numerals.get(c.id) ?? 0;
        return (
          <RefNumeral
            key={c.id}
            n={n}
            label={numeralLabel(n, c, where)}
            blocker={isHumanBlocker(c)}
            selected={c.id === selected}
            leader={leader}
            onOpen={() => onOpen(c)}
          />
        );
      })}
      {more && (
        <button
          type="button"
          className={cn("refnum-more", hidden.some((c) => c.id === selected) && "selected")}
          aria-label={`${hidden.length} more open ${hidden.length === 1 ? "comment" : "comments"} on ${where}`}
          title={`${hidden.length} more on ${where}`}
          onClick={() => onOpen(more)}
        >
          +{hidden.length}
        </button>
      )}
    </>
  );
}

export function numeralLabel(n: number, c: Comment, where: string): string {
  return `Comment ${n}${isHumanBlocker(c) ? ", your blocker," : ""} on ${where}`;
}
