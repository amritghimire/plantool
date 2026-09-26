import { fireEvent, render, screen } from "@testing-library/react";
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
  render(
    <Sidebar
      view={currentView}
      comments={[question]}
      activeTab="research"
      onTab={() => {}}
      onStage={async () => {}}
      onBrief={async () => {}}
      onDelete={async () => {}}
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
  return { actions, onSendToRun, onContinueMilestone, onApproveMilestone };
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

it("offers milestone approval after a step-by-step turn", () => {
  const run = { ...view.runs[0], stage: "implementing" as const, task: "implement", status: "idle" as const, implementation_mode: "step-by-step" as const, milestone_pending: true };
  const currentView = { ...view, state: { ...view.state, stage: "implementing" as const }, runs: [run] };
  const { onApproveMilestone } = setup(currentView);
  fireEvent.click(screen.getByText("Approve milestone and continue"));
  expect(onApproveMilestone).toHaveBeenCalledWith(run);
  expect(screen.getByText("Accept implementation")).toBeDisabled();
});
