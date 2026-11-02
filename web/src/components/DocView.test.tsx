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

const deck: DocResponse = {
  kind: "research",
  sha: "y",
  content: "# Research\n\nIntro here.\n\n## Flow\n\nFlow text.\n\n## Risks\n\nRisk text.\n",
  captured_at: "",
  path: "/tmp/research.md",
  lines: 11,
  headings: [
    { line: 1, level: 1, text: "Research" },
    { line: 5, level: 2, text: "Flow" },
    { line: 9, level: 2, text: "Risks" },
  ],
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


describe("DocView", () => {
  it("tasks are read-only without a toggle handler", () => {
    render(<DocView kind="plan" doc={doc} path={null} comments={[]} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
    expect(screen.getByRole("checkbox", { name: "Task not completed" })).toBeDisabled();
  });

  it("ticks and unticks a task by its source line", async () => {
    const onToggleTask = vi.fn(async () => {});
    render(<DocView kind="plan" doc={doc} path={null} comments={[]} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} onToggleTask={onToggleTask} />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Task not completed. Tick" }));
    expect(onToggleTask).toHaveBeenCalledWith(5, true);
    fireEvent.click(screen.getByRole("checkbox", { name: "Task completed. Untick" }));
    expect(onToggleTask).toHaveBeenCalledWith(6, false);
  });

  it("anchors a new comment to the clicked block's source line", async () => {
    const onAdd = vi.fn(async () => {});
    render(<DocView kind="plan" doc={doc} path={null} comments={[]} onAdd={onAdd} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
    fireEvent.click(screen.getByTitle("Comment on line 3"));
    const box = screen.getByPlaceholderText("Comment on line 3…");
    fireEvent.change(box, { target: { value: "hello" } });
    fireEvent.click(screen.getByText("Comment"));
    await vi.waitFor(() => expect(onAdd).toHaveBeenCalledWith("plan", 3, "hello", "note", "line"));
  });

  it("numbers paragraphs and shows one reference numeral per open thread instead of the thread", () => {
    const onOpenThread = vi.fn();
    const second: Comment = { ...comment, id: "c2", seq: 2, kind: "agent", author: "council" };
    render(<DocView kind="plan" doc={doc} path={null} comments={[comment, second, { ...comment, id: "c3", seq: 3, resolved: true }]} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={onOpenThread} />);
    expect(screen.queryByText("why?")).toBeNull();
    expect(document.getElementById("c-c1")).toBeNull();
    const block = document.querySelector('[data-line="5"]')!;
    expect(block.querySelector(".para-n")).toHaveTextContent("[0002]");
    expect(document.querySelector('[data-line="3"] .para-n')).toHaveTextContent("[0001]");
    expect(document.querySelector('[data-line="1"] .para-n')).toBeNull();
    const numeral = screen.getByRole("button", { name: "Comment 10 on paragraph 2" });
    expect(block.contains(numeral)).toBe(true);
    expect(numeral).toHaveTextContent("10");
    expect(numeral).not.toHaveClass("blocker");
    expect(numeral).not.toHaveClass("selected");
    expect(screen.queryByRole("button", { name: /Comment 14/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Comment 12 on paragraph 2" }));
    expect(onOpenThread).toHaveBeenCalledWith(second);
  });

  it("marks human blockers and the selected block on the numeral", () => {
    const blocker: Comment = { ...comment, type: "blocker" };
    const { rerender } = render(<DocView kind="plan" doc={doc} path={null} comments={[blocker]} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
    expect(screen.getByRole("button", { name: "Comment 10, your blocker, on paragraph 2" })).toHaveClass("blocker");
    rerender(<DocView kind="plan" doc={doc} path={null} comments={[{ ...blocker, kind: "agent" }]} onAdd={async () => {}} mode="rendered" target={null} highlightComment="c1" onOpenThread={() => {}} />);
    const numeral = screen.getByRole("button", { name: "Comment 10 on paragraph 2" });
    expect(numeral).not.toHaveClass("blocker");
    expect(numeral).toHaveClass("selected");
    expect(document.querySelector('[data-line="5"]')).toHaveClass("selected");
  });

  it("stacks three numerals on a block and folds the rest into +k", () => {
    const onOpenThread = vi.fn();
    const many = [1, 2, 3, 4, 5].map((seq) => ({ ...comment, id: `m${seq}`, seq }));
    render(<DocView kind="plan" doc={doc} path={null} comments={many} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={onOpenThread} />);
    const margin = document.querySelector('[data-line="5"] .blk-margin')!;
    expect(Array.from(margin.querySelectorAll(".refnum")).map((b) => b.textContent)).toEqual(["10", "12", "14"]);
    const more = screen.getByRole("button", { name: "2 more open comments on paragraph 2" });
    expect(more).toHaveTextContent("+2");
    fireEvent.click(more);
    expect(onOpenThread).toHaveBeenCalledWith(many[3]);
  });

  it("uses the numerals it is given so the drawer and margin agree", () => {
    render(<DocView kind="plan" doc={doc} path={null} comments={[comment]} numerals={new Map([["c1", 24]])} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
    expect(screen.getByRole("button", { name: "Comment 24 on paragraph 2" })).toHaveTextContent("24");
  });

  it("puts Comment on document on the title row", () => {
    render(<DocView kind="plan" doc={doc} path={null} comments={[]} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
    const action = screen.getByRole("button", { name: "Comment on document" });
    expect(action.closest(".title-row")).toHaveTextContent("Plan");
    fireEvent.click(action);
    expect(screen.getByPlaceholderText("Comment on the whole document…")).toBeInTheDocument();
  });

  it("keeps orphaned comments out of the document", () => {
    render(<DocView kind="plan" doc={doc} path={null} comments={[{ ...comment, anchor: { line: 40, text: "gone", outdated: true } }]} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
    expect(screen.queryByText("why?")).toBeNull();
    expect(screen.queryByRole("button", { name: /^Comment \d/ })).toBeNull();
  });

  it("keeps an unsent comment and follows its line when the document is rewritten", () => {
    const onAdd = vi.fn(async () => {});
    const props = { kind: "plan" as const, path: null, comments: [], onAdd, mode: "rendered" as const, target: null, highlightComment: null, onOpenThread: () => {} };
    const { rerender } = render(<DocView {...props} doc={doc} />);
    fireEvent.click(screen.getByTitle("Comment on line 3"));
    fireEvent.change(screen.getByPlaceholderText("Comment on line 3…"), { target: { value: "keep me" } });
    const rewritten: DocResponse = { ...doc, sha: "z", content: "# Plan\n\nNew intro.\n\nFirst paragraph.\n\n- [ ] task one\n- [x] task two\n", lines: 8 };
    rerender(<DocView {...props} doc={rewritten} />);
    const box = screen.getByPlaceholderText("Comment on line 5…") as HTMLTextAreaElement;
    expect(box.value).toBe("keep me");
    expect(screen.getByText(/moved from line 3 to line 5/)).toBeInTheDocument();
  });

  it("shows the waiting state when the doc is missing", () => {
    render(<DocView kind="research" doc={null} path="/x/research.md" comments={[]} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
    expect(screen.getByText("Waiting for research.md")).toBeInTheDocument();
    expect(screen.getByText("/x/research.md")).toBeInTheDocument();
  });

  it("shows the stage prompt with a copy button in the waiting state", async () => {
    const writeText = vi.fn(async () => {});
    Object.assign(navigator, { clipboard: { writeText } });
    const onStartRun = vi.fn();
    render(<DocView kind="research" doc={null} path="/x/research.md" comments={[]} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} prompt="Research for `repo/x`" onStartRun={onStartRun} />);
    expect(screen.getByText("Research for `repo/x`").closest("details")).not.toHaveAttribute("open");
    fireEvent.click(screen.getByText("Copy prompt"));
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith("Research for `repo/x`"));
    expect(await screen.findByText("Copied")).toBeInTheDocument();
    expect(screen.queryByText("Run it here")).toBeNull();
    expect(onStartRun).not.toHaveBeenCalled();
  });

  it("slides mode shows one section at a time and anchors to absolute lines", async () => {
    const onAdd = vi.fn(async () => {});
    const riskComment: Comment = { ...comment, id: "c9", doc: "research", anchor: { line: 11, text: "Risk text.", outdated: false }, body: "hmm" };
    const onOpenThread = vi.fn();
    render(<DocView kind="research" doc={deck} path={null} comments={[riskComment]} onAdd={onAdd} mode="slides" target={null} highlightComment={null} onOpenThread={onOpenThread} />);
    expect(document.querySelector(".para-n")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Comment 10 on slide 3" }));
    expect(onOpenThread).toHaveBeenCalledWith(riskComment);
    expect(screen.getByText("Intro here.")).toBeInTheDocument();
    expect(screen.queryByText("Flow text.")).toBeNull();
    expect(screen.getByText("1 / 3 · Research")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByText("Flow text.")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(screen.getByText("Risk text.")).toBeInTheDocument();
    expect(screen.queryByText("hmm")).toBeNull();
    expect(screen.getByRole("button", { name: "Comment 10 on line 11" })).toBeInTheDocument();
    fireEvent.click(screen.getByTitle("Comment on line 11"));
    fireEvent.change(screen.getByPlaceholderText("Comment on line 11…"), { target: { value: "note" } });
    fireEvent.click(screen.getByText("Comment"));
    await vi.waitFor(() => expect(onAdd).toHaveBeenCalledWith("research", 11, "note", "note", "line"));
    fireEvent.click(screen.getByText("Flow"));
    expect(screen.getByText("Flow text.")).toBeInTheDocument();
  });

  it("source mode anchors to the exact line", async () => {
    const onAdd = vi.fn(async () => {});
    render(<DocView kind="plan" doc={doc} path={null} comments={[]} onAdd={onAdd} mode="source" target={null} highlightComment={null} onOpenThread={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Comment on line 6" }));
    fireEvent.change(screen.getByPlaceholderText("Comment on line 6…"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Comment"));
    await vi.waitFor(() => expect(onAdd).toHaveBeenCalledWith("plan", 6, "x", "note", "line"));
  });
});

it("creates a document blocker without a line selection", async () => {
  const onAdd = vi.fn(async () => {});
  render(<DocView kind="plan" doc={doc} path={null} comments={[]} onAdd={onAdd} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
  fireEvent.click(screen.getByText("Comment on document"));
  fireEvent.change(screen.getByLabelText("Feedback type"), { target: { value: "blocker" } });
  fireEvent.change(screen.getByPlaceholderText("Comment on the whole document…"), { target: { value: "Missing risks" } });
  fireEvent.click(screen.getByText("Comment"));
  await vi.waitFor(() => expect(onAdd).toHaveBeenCalledWith("plan", 1, "Missing risks", "blocker", "document"));
});

it("anchors section feedback to its heading", async () => {
  const onAdd = vi.fn(async () => {});
  render(<DocView kind="research" doc={deck} path={null} comments={[]} onAdd={onAdd} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
  fireEvent.click(screen.getAllByText("Comment on section")[1]);
  fireEvent.change(screen.getByPlaceholderText("Comment on line 9…"), { target: { value: "Explain risk" } });
  fireEvent.click(screen.getByText("Comment"));
  await vi.waitFor(() => expect(onAdd).toHaveBeenCalledWith("research", 9, "Explain risk", "note", "section"));
});

it("opens and cancels a comment with the keyboard and returns focus", async () => {
  render(<DocView kind="plan" doc={doc} path={null} comments={[]} onAdd={async () => {}} mode="rendered" target={null} highlightComment={null} onOpenThread={() => {}} />);
  const block = document.querySelector<HTMLElement>('[data-line="3"]')!;
  block.focus();
  fireEvent.keyDown(block, { key: "c" });
  const box = screen.getByPlaceholderText("Comment on line 3…");
  expect(box).toHaveFocus();
  fireEvent.keyDown(box, { key: "Escape" });
  await vi.waitFor(() => expect(document.querySelector('[data-line="3"]')).toHaveFocus());
});

it("shows per-line numerals in source view", () => {
  const onOpenThread = vi.fn();
  render(<DocView kind="plan" doc={doc} path={null} comments={[comment]} onAdd={async () => {}} mode="source" target={null} highlightComment={null} onOpenThread={onOpenThread} />);
  expect(screen.queryByText("why?")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Comment 10 on line 5" }));
  expect(onOpenThread).toHaveBeenCalledWith(comment);
});

it("allows typed general comments in source view", async () => {
  const onAdd = vi.fn(async () => {});
  render(<DocView kind="plan" doc={doc} path={null} comments={[]} onAdd={onAdd} mode="source" target={null} highlightComment={null} onOpenThread={() => {}} />);
  fireEvent.click(screen.getByText("Comment on document"));
  fireEvent.change(screen.getByLabelText("Feedback type"), { target: { value: "question" } });
  fireEvent.change(screen.getByPlaceholderText("Comment on the whole document…"), { target: { value: "What is the assumption?" } });
  fireEvent.click(screen.getByText("Comment"));
  await vi.waitFor(() => expect(onAdd).toHaveBeenCalledWith("plan", 1, "What is the assumption?", "question", "document"));
});

it.each(["rendered", "source", "slides"] as const)("navigates without composing in %s mode", (mode) => {
  HTMLElement.prototype.scrollIntoView = vi.fn();
  render(<DocView kind="research" doc={deck} path={null} comments={[]} onAdd={async () => {}} mode={mode} target={{ line: 9, nonce: 1 }} highlightComment={null} onOpenThread={() => {}} />);
  expect(document.querySelector('[data-line="9"]')).not.toBeNull();
  expect(screen.queryByRole("textbox")).toBeNull();
  if (mode === "slides") expect(document.querySelector(".slides")?.firstElementChild).toHaveAttribute("aria-label", "Slides");
});
it.each(["source", "slides"] as const)("preserves section scope in %s mode", async (mode) => {
  const onAdd = vi.fn(async () => {});
  render(<DocView kind="research" doc={deck} path={null} comments={[]} onAdd={onAdd} mode={mode} target={null} highlightComment={null} onOpenThread={() => {}} />);
  fireEvent.click(screen.getAllByText("Comment on section")[0]);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Section feedback" } });
  fireEvent.click(screen.getByText("Comment"));
  await vi.waitFor(() => expect(onAdd).toHaveBeenCalledWith("research", 1, "Section feedback", "note", "section"));
});
