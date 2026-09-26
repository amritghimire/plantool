import { fireEvent, render, screen } from "@testing-library/react";
import { DocView } from "./DocView";
import type { Comment, DocResponse } from "../types";

const doc: DocResponse = {
  kind: "plan",
  sha: "x",
  content: "# Plan\n\nFirst paragraph.\n\n- [ ] task one\n- [x] task two\n",
  captured_at: "",
  path: "/tmp/plan.md",
  lines: 6,
  headings: [],
  progress: [],
};

const comment: Comment = {
  id: "c1",
  doc: "plan",
  anchor: { line: 5, text: "- [ ] task one", outdated: false },
  body: "why?",
  kind: "human",
  author: "me",
  parent: null,
  resolved: false,
  created_at: new Date().toISOString(),
  updated_at: null,
  seq: 1,
};

const actions = { reply: async () => {}, resolve: async () => {}, edit: async () => {}, remove: async () => {} };

describe("DocView", () => {
  it("anchors a new comment to the clicked block's source line", async () => {
    const onAdd = vi.fn(async () => {});
    render(<DocView kind="plan" doc={doc} path={null} comments={[]} actions={actions} onAdd={onAdd} mode="rendered" target={null} highlightComment={null} showResolved={false} />);
    fireEvent.click(screen.getByTitle("Comment on line 3"));
    const box = screen.getByPlaceholderText("Comment on line 3…");
    fireEvent.change(box, { target: { value: "hello" } });
    fireEvent.click(screen.getByText("Comment"));
    await vi.waitFor(() => expect(onAdd).toHaveBeenCalledWith("plan", 3, "hello"));
  });

  it("renders an existing thread under its list item", () => {
    render(<DocView kind="plan" doc={doc} path={null} comments={[comment]} actions={actions} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} showResolved={false} />);
    const thread = document.getElementById("c-c1");
    expect(thread).not.toBeNull();
    expect(thread!.closest("li")?.getAttribute("data-line")).toBe("5");
    expect(screen.getByText("why?")).toBeInTheDocument();
  });

  it("shows the waiting state when the doc is missing", () => {
    render(<DocView kind="research" doc={null} path="/x/research.md" comments={[]} actions={actions} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} showResolved={false} />);
    expect(screen.getByText("Waiting for research.md")).toBeInTheDocument();
    expect(screen.getByText("/x/research.md")).toBeInTheDocument();
  });

  it("source mode anchors to the exact line", async () => {
    const onAdd = vi.fn(async () => {});
    render(<DocView kind="plan" doc={doc} path={null} comments={[]} actions={actions} onAdd={onAdd} mode="source" target={null} highlightComment={null} showResolved={false} />);
    const buttons = screen.getAllByText("+");
    fireEvent.click(buttons[5]);
    fireEvent.change(screen.getByPlaceholderText("Comment on line 6…"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Comment"));
    await vi.waitFor(() => expect(onAdd).toHaveBeenCalledWith("plan", 6, "x"));
  });
});
