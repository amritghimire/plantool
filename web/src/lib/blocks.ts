import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import type { Node, Parent } from "unist";

export interface Block {
  type: string;
  start: number;
  end: number;
}

const ATTACH_TYPES = new Set(["heading", "paragraph", "listItem", "code", "table", "html", "thematicBreak", "math"]);

function isParent(n: Node): n is Parent {
  return Array.isArray((n as Parent).children);
}

export function collectBlocks(markdown: string): Block[] {
  const tree = unified().use(remarkParse).use(remarkGfm).parse(markdown);
  const out: Block[] = [];
  const walk = (node: Node, parentType: string | null) => {
    const pos = node.position;
    if (pos && ATTACH_TYPES.has(node.type)) {
      const skip = node.type === "paragraph" && parentType === "listItem";
      if (!skip) out.push({ type: node.type, start: pos.start.line, end: pos.end.line });
    }
    if (isParent(node)) for (const c of node.children) walk(c, node.type);
  };
  walk(tree, null);
  return out;
}

export function blockForLine(blocks: Block[], line: number): Block | null {
  let best: Block | null = null;
  for (const b of blocks) {
    if (line < b.start || line > b.end) continue;
    if (!best || b.end - b.start <= best.end - best.start) best = b;
  }
  return best;
}

export function lineToBlockStart(blocks: Block[], line: number): number | null {
  return blockForLine(blocks, line)?.start ?? null;
}
