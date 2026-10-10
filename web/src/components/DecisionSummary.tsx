import type { DocResponse } from "../types";
export function DecisionSummary({ doc, onNavigate, onCritique }: { doc: DocResponse; onNavigate: (line: number) => void; onCritique: (prompt: string) => void }) {
  const headings = doc.headings.filter((h) => ["assumptions", "risks", "decisions needed"].includes(h.text.toLowerCase()));
  return <section className="decision-summary side-block" aria-label="Review decisions">
    <nav className="button-row" aria-label="Plan sections">{headings.map((h) => <button className="link" type="button" key={h.line} onClick={() => onNavigate(h.line)}>{h.text}</button>)}</nav>
    <details className="disclosure"><summary>Help me review this plan</summary>{["What could go wrong?", "What was assumed?", "What is missing?"].map((question) => <button type="button" className="btn ghost small" key={question} onClick={() => onCritique(`Review the plan with this question: ${question} Check claims against the code and leave focused comments with risks and open questions.`)}>{question}</button>)}</details>
  </section>;
}
