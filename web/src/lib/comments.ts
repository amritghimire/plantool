import type { Comment, DocKind } from "../types";

export type CommentFilter = "open" | "blockers" | "outdated" | "mine";

const DOC_ORDER: DocKind[] = ["research", "plan", "investigation", "quick-fix", "design"];

function docRank(c: Comment): number {
  if (c.anchor.code) return DOC_ORDER.length;
  const i = DOC_ORDER.indexOf(c.doc);
  return i < 0 ? DOC_ORDER.length - 1 : i;
}

export function documentOrder(roots: Comment[]): Comment[] {
  return [...roots].sort(
    (a, b) =>
      docRank(a) - docRank(b) ||
      (a.anchor.code?.path ?? "").localeCompare(b.anchor.code?.path ?? "") ||
      (a.anchor.code?.line ?? a.anchor.line) - (b.anchor.code?.line ?? b.anchor.line) ||
      a.seq - b.seq,
  );
}

export function isHumanBlocker(c: Comment): boolean {
  return c.kind === "human" && c.type === "blocker";
}

export function filterThreads(
  comments: Comment[],
  filter: CommentFilter,
  author: string,
  isOrphan: (c: Comment) => boolean = () => false,
): Comment[] {
  const open = documentOrder(comments.filter((c) => !c.parent && !c.resolved));
  switch (filter) {
    case "open":
      return open;
    case "blockers":
      return open.filter(isHumanBlocker);
    case "outdated":
      return open.filter((c) => c.anchor.outdated || isOrphan(c));
    case "mine":
      return open.filter((c) => c.kind === "human" && c.author === author);
  }
}

export function stepThread(order: Comment[], current: string | null, dir: 1 | -1): Comment | null {
  if (order.length === 0) return null;
  const i = current ? order.findIndex((c) => c.id === current) : -1;
  if (i < 0) return dir === 1 ? order[0] : order[order.length - 1];
  return order[(i + dir + order.length) % order.length];
}

export function anchorLabel(c: Comment): string {
  if (c.anchor.code) return `${c.anchor.code.path}:${c.anchor.code.line}`;
  if (c.anchor.scope === "document") return "document";
  if (c.anchor.scope === "section") return `section · line ${c.anchor.line}`;
  return `line ${c.anchor.line}`;
}

export function previewOf(body: string): string {
  return body.trim().split(/\n\s*\n/)[0] ?? "";
}

export function referenceNumerals(comments: Comment[]): Map<string, number> {
  const roots = documentOrder(comments.filter((c) => !c.parent));
  return new Map(roots.map((c, i) => [c.id, 10 + 2 * i]));
}
