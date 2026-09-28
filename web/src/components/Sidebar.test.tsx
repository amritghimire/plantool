import { fireEvent, render, screen } from "@testing-library/react";
import { api } from "../api";
import { Sidebar } from "./Sidebar";
import type { Comment, SessionView } from "../types";

const view: SessionView = {
  key: "repo/x",
  url_path: "/s/repo/x",
  dir: "/d",
  session: { repo_slug: "repo", slug: "x", title: "x", repo: { root: "/r", common_dir: "/r/.git", branch: "main" }, worktree: null, base: "main", mirror: false, created_at: new Date().toISOString() },
  state: { stage: "research-review", comments: [], docs: {}, seq: 3, review: null, updated_at: "" },
  docs: [
    { kind: "research", path: "/d/research.md", exists: true, sha: "a", lines: 10, title: null, captured_at: null, progress: [] },
    { kind: "plan", path: "/d/plan.md", exists: false, sha: null, lines: 0, title: null, captured_at: null, progress: [] },
  ],
  runs: [{ id: "r1", provider: "claude", provider_session_id: null, stage: "researching", cwd: "/r", status: "idle", model: null, started_at: new Date().toISOString(), ended_at: null, error: null, seq: 1 }],
  open_comments: 1,
};

const question: Comment = {
  id: "q1",
  doc: "research",
  anchor: { line: 4, text: "Keep, hide, or remove Storage.", outdated: false },
  body: "Keep, hide, or remove Storage?",
  kind: "agent",
  author: "agent",
  parent: null,
  resolved: false,
  created_at: new Date().toISOString(),
  updated_at: null,
  seq: 2,
};

function setup(currentView = view) {
  const actions = { reply: vi.fn(async () => {}), resolve: vi.fn(async () => {}), edit: vi.fn(async () => {}), remove: vi.fn(async () => {}) };
  const onSendToRun = vi.fn(async () => {});
  const onContinueMilestone = vi.fn();
  const onApproveMilestone = vi.fn();
  const onDelete = vi.fn(async () => {});
  render(
    <Sidebar
      view={currentView}
      comments={[question]}
      activeTab="research"
      onTab={() => {}}
      onStage={async () => {}}
      onBrief={async () => {}}
      onDelete={onDelete}
      onJump={() => {}}
      onStartRun={() => {}}
      onContinueMilestone={onContinueMilestone}
      onApproveMilestone={onApproveMilestone}
      onOpenChanges={() => {}}
      onSelectRun={() => {}}
      onRemoveRun={async () => {}}
      selectedRun={null}
      busy={false}
      actions={actions}
      reviewPrompt="Act on the review comments"
      onSendToRun={onSendToRun}
    />,
  );
  return { actions, onSendToRun, onContinueMilestone, onApproveMilestone, onDelete };
}

describe("Sidebar comments", () => {
  it("replies with a quick reaction and resolves from the list", async () => {
    const { actions } = setup();
    fireEvent.click(screen.getByTitle("Reply"));
    fireEvent.click(screen.getByText("Agreed, go ahead."));
    await vi.waitFor(() => expect(actions.reply).toHaveBeenCalledWith(question, "Agreed, go ahead."));
    await vi.waitFor(() => expect(screen.queryByPlaceholderText("Reply… (⌘↩ to send)")).toBeNull());
    fireEvent.click(screen.getByTitle("Resolve"));
    await vi.waitFor(() => expect(actions.resolve).toHaveBeenCalledWith(question, true));
  });

  it("expands a thread in place and asks the running agent about it", async () => {
    const { onSendToRun } = setup();
    fireEvent.click(screen.getByTitle("Read the conversation"));
    expect(screen.getByText("Keep, hide, or remove Storage?")).toBeInTheDocument();
    fireEvent.click(screen.getByTitle("Ask the running agent to answer this thread"));
    await vi.waitFor(() => expect(onSendToRun).toHaveBeenCalled());
    expect((onSendToRun.mock.calls[0] as unknown as [string])[0]).toContain("--parent q1");
  });

  it("sends the review prompt to the live run", async () => {
    const { onSendToRun } = setup();
    fireEvent.click(screen.getByText("Send comments to the running agent"));
    await vi.waitFor(() => expect(onSendToRun).toHaveBeenCalledWith("Act on the review comments"));
  });
});

it("shows the session's active workspace", () => {
  setup();
  expect(screen.getByText("/r", { selector: ".workspace-path" })).toBeInTheDocument();
});

it("shows the worktree as the active workspace", () => {
  setup({ ...view, session: { ...view.session, worktree: "/r/.worktree/x" } });
  expect(screen.getByText("/r/.worktree/x", { selector: ".workspace-path" })).toBeInTheDocument();
});

it("confirms session deletion once and makes worktree removal optional", () => {
  const { onDelete } = setup({ ...view, session: { ...view.session, worktree: "/worktrees/x" } });
  fireEvent.click(screen.getByRole("button", { name: "Drop session" }));
  expect(screen.getByRole("dialog", { name: "Drop repo/x?" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Drop session" }));
  fireEvent.click(screen.getByRole("checkbox", { name: /Also remove worktree/ }));
  fireEvent.click(screen.getAllByRole("button", { name: "Drop session" }).at(-1)!);
  expect(onDelete).toHaveBeenCalledWith(true);
});

it("offers milestone approval after a step-by-step turn with an editable commit message", async () => {
  vi.spyOn(api, "milestone").mockResolvedValue({ subject: "Milestone 1: add the route", dirty: true, milestone: 1, pending: true, live: true });
  const run = { ...view.runs[0], stage: "implementing" as const, task: "implement", status: "idle" as const, implementation_mode: "step-by-step" as const, milestone_pending: true };
  const currentView = { ...view, state: { ...view.state, stage: "implementing" as const }, runs: [run] };
  const { onApproveMilestone } = setup(currentView);
  const message = await screen.findByDisplayValue("Milestone 1: add the route");
  fireEvent.change(message, { target: { value: "Milestone 1: guest home route" } });
  fireEvent.click(screen.getByText("Approve milestone and continue"));
  expect(onApproveMilestone).toHaveBeenCalledWith(run, true, "Milestone 1: guest home route");
  expect(screen.getByText("Accept implementation")).toBeDisabled();

  fireEvent.click(screen.getByLabelText("commit this milestone"));
  expect(screen.queryByLabelText("Commit message")).toBeNull();
  fireEvent.click(screen.getByText("Approve milestone and continue"));
  expect(onApproveMilestone).toHaveBeenLastCalledWith(run, false, "Milestone 1: guest home route");
});
