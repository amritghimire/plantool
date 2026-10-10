import { fireEvent, render, screen } from "@testing-library/react";
import { DecisionSummary } from "./DecisionSummary";
import type { DocResponse } from "../types";
it("uses heading links without repeating document content", () => {
  const onNavigate = vi.fn();
  const doc: DocResponse = { kind: "plan", content: "## Risks\nSensitive risk text", headings: [{ level: 2, line: 1, text: "Risks" }], sha: "x", path: "", lines: 2, captured_at: "", progress: [] };
  render(<DecisionSummary doc={doc} onNavigate={onNavigate} onCritique={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "Risks" }));
  expect(onNavigate).toHaveBeenCalledWith(1);
  expect(screen.queryByText("Sensitive risk text")).toBeNull();
  expect(screen.getByText("Help me review this plan").closest("details")).not.toHaveAttribute("open");
});
