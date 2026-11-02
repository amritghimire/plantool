import type { DocResponse } from "../types";

export function DecisionSummary({ doc, onNavigate }: { doc: DocResponse; onNavigate: (line: number) => void }) {
  const headings = doc.headings.filter((h) => ["assumptions", "risks", "decisions needed"].includes(h.text.toLowerCase()));
  if (!headings.length) return null;
  return (
    <nav className="notice-jumps" aria-label="Plan sections">
      <span>Read first</span>
      {headings.map((h) => (
        <button className="link" type="button" key={h.line} onClick={() => onNavigate(h.line)}>
          {h.text}
        </button>
      ))}
    </nav>
  );
}
