import type { DocResponse } from "../types";
export function DecisionSummary({ doc, onComment, onCritique }: { doc: DocResponse; onComment: (line: number) => void; onCritique: (prompt: string) => void }) {
  const lines = doc.content.split("\n");
  const cards = doc.headings.filter((h) => ["assumptions", "risks", "decisions needed"].includes(h.text.toLowerCase())).map((h) => {
    const end = doc.headings.find((next) => next.line > h.line && next.level <= h.level)?.line ?? lines.length + 1;
    return { ...h, content: lines.slice(h.line, end - 1).join("\n") };
  });
  return <section className="decision-summary" aria-label="Review decisions">
    {cards.map((card) => <article className="banner" key={card.line}><h3>{card.text}</h3><pre style={{ whiteSpace: "pre-wrap" }}>{card.content}</pre><button className="btn ghost" type="button" onClick={() => onComment(card.line)}>Comment on {card.text.toLowerCase()}</button></article>)}
    <details><summary>Help me review this plan</summary>{["What could go wrong?", "What was assumed?", "What is missing?"].map((question) => <button type="button" className="btn ghost" key={question} onClick={() => onCritique(`Review the plan with this question: ${question} Check claims against the code and leave focused comments with risks and open questions.`)}>{question}</button>)}</details>
  </section>;
}
