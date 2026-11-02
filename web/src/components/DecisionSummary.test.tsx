import { fireEvent, render, screen } from "@testing-library/react";
import { DecisionSummary } from "./DecisionSummary";
import type { DocResponse } from "../types";
it("uses heading links without repeating document content", () => {
  const onNavigate = vi.fn();
  const doc: DocResponse = { kind: "plan", content: "## Risks\nSensitive risk text", headings: [{ level: 2, line: 1, text: "Risks" }], sha: "x", path: "", lines: 2, captured_at: "", progress: [] };
  render(<DecisionSummary doc={doc} onNavigate={onNavigate} />);
  fireEvent.click(screen.getByRole("button", { name: "Risks" }));
  expect(onNavigate).toHaveBeenCalledWith(1);
  expect(screen.queryByText("Sensitive risk text")).toBeNull();
});
it("renders nothing when the plan has no decision sections", () => {
  const doc: DocResponse = { kind: "plan", content: "## Summary", headings: [{ level: 2, line: 1, text: "Summary" }], sha: "x", path: "", lines: 1, captured_at: "", progress: [] };
  const { container } = render(<DecisionSummary doc={doc} onNavigate={() => {}} />);
  expect(container).toBeEmptyDOMElement();
});
