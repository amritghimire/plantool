import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { api } from "../api";
import { SessionSpine, type SessionSpineProps } from "./SessionSpine";
import { CommentsDrawer } from "./CommentsDrawer";
import { AgentDrawer } from "./AgentDrawer";
import { DetailsDrawer } from "./DetailsDrawer";
import type { Comment, DocResponse, Run, SessionView } from "../types";

const now = new Date().toISOString();

const view: SessionView = {
  key: "repo/x",
  url_path: "/s/repo/x",
  dir: "/d",
  session: { repo_slug: "repo", slug: "x", title: "Fix the inbox", repo: { root: "/r", common_dir: "/r/.git", branch: "main" }, worktree: null, base: "main", mirror: false, created_at: now },
  state: { stage: "research-review", comments: [], docs: {}, seq: 3, review: null, updated_at: "" },
  docs: [
    { kind: "research", path: "/d/research.md", exists: true, sha: "a", lines: 10, title: null, captured_at: null, progress: [] },
    { kind: "plan", path: "/d/plan.md", exists: false, sha: null, lines: 0, title: null, captured_at: null, progress: [] },
  ],
  runs: [{ id: "r1", provider: "claude", provider_session_id: null, stage: "researching", cwd: "/r", status: "idle", model: null, started_at: now, ended_at: null, error: null, seq: 1 }],
  open_comments: 1,
};

const withPlan = (v: SessionView, stage: SessionView["state"]["stage"]): SessionView => ({
  ...v,
  state: { ...v.state, stage },
  docs: [v.docs[0], { ...v.docs[1], exists: true, sha: "4f2c19a8e1", progress: [{ name: "Phase 1", done: 3, total: 15 }] }],
});

const question: Comment = {
  id: "q1",
  doc: "research",
  anchor: { line: 4, text: "Keep, hide, or remove Storage.", outdated: false },
  body: "Keep, hide, or remove Storage?",
  kind: "agent",
  author: "agent",
  parent: null,
  resolved: false,
  created_at: now,
  updated_at: null,
  seq: 2,
};

const blocker: Comment = { ...question, id: "b1", doc: "plan", kind: "human", author: "me", type: "blocker", body: "Wrong scope" };

const planDoc: DocResponse = { kind: "plan", sha: "4f2c19a8e1", content: "# P\n## Risks\nx", captured_at: now, path: "/d/plan.md", lines: 3, headings: [{ level: 2, line: 2, text: "Risks" }], progress: [] };

function spine(currentView: SessionView = view, comments: Comment[] = [question], extra: Partial<SessionSpineProps> = {}) {
  const p: SessionSpineProps = {
    view: currentView,
    comments,
    activeTab: "research",
    planDoc: null,
    busy: false,
    onStartRun: vi.fn(),
    onStage: vi.fn(async () => {}),
    onJump: vi.fn(),
    onNavigate: vi.fn(),
    onAskAgent: vi.fn(),
    onPr: vi.fn(),
    onOpenRun: vi.fn(),
    onOpenChanges: vi.fn(),
    onContinueMilestone: vi.fn(),
    onApproveMilestone: vi.fn(),
    onCancelCommit: vi.fn(),
    onDetails: vi.fn(),
    onSettings: vi.fn(),
    ...extra,
  };
  render(
    <MemoryRouter>
      <SessionSpine {...p} />
    </MemoryRouter>,
  );
  return p;
}

const actions = () => ({ reply: vi.fn(async () => {}), resolve: vi.fn(async () => {}), edit: vi.fn(async () => {}), remove: vi.fn(async () => {}) });

function details(currentView: SessionView = view) {
  const p = {
    onBrief: vi.fn(async () => {}),
    onStage: vi.fn(async () => {}),
    onStartRun: vi.fn(),
    onRefreshPr: vi.fn(),
    onUpdated: vi.fn(),
    onWorkspace: vi.fn(),
    onDelete: vi.fn(async () => {}),
  };
  render(<DetailsDrawer view={currentView} busy={false} {...p} />);
  return p;
}

function openMenu() {
  fireEvent.click(screen.getByRole("button", { name: "Ask agent" }));
  return screen.getByRole("dialog", { name: "Ask agent" });
}

describe("Sidebar inventory: every old control has one new home", () => {
  it("identity, steps and the Notice live in the spine", () => {
    spine(withPlan(view, "plan-review"), [blocker], { activeTab: "plan", planDoc });
    expect(screen.getByRole("link", { name: "Inbox" })).toHaveAttribute("href", "/");
    expect(screen.getByText("Fix the inbox")).toHaveClass("spine-title");
    expect(screen.queryByRole("heading", { level: 1 })).toBeNull();
    expect(screen.getByText("repo")).toBeInTheDocument();
    const steps = screen.getByRole("list", { name: "Steps" });
    expect(within(steps).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "FIG. 1Researchdone",
      "FIG. 2Plancurrent",
      "FIG. 3Build3 / 15 tasks",
      "FIG. 4Reviewpending",
      "FIG. 5Donepending",
    ]);
    expect(within(steps).getByText("Plan").closest("li")).toHaveAttribute("aria-current", "step");
    expect(screen.getByText("Notice of allowance")).toBeInTheDocument();
  });

  it("onStage (approve), the blockers gate and the override reason live in the Notice", async () => {
    const p = spine(withPlan(view, "plan-review"), [blocker], { activeTab: "plan", planDoc });
    expect(screen.getByText("1 blocker stops approval")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Review blocker" }));
    expect(p.onJump).toHaveBeenCalledWith(blocker);
    fireEvent.click(screen.getByRole("button", { name: "Approve with override…" }));
    const dialog = screen.getByRole("alertdialog");
    expect(dialog).toHaveTextContent("plan.md rev 4f2c19a");
    const confirm = within(dialog).getByRole("button", { name: "Approve with recorded override" });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Why are you overriding the blockers?"), { target: { value: "Tracked separately" } });
    fireEvent.click(confirm);
    await vi.waitFor(() => expect(p.onStage).toHaveBeenCalledWith("approved", "Tracked separately"));
  });

  it("onStartRun covers research, plan, skips, build (incl. step by step) and resume", async () => {
    const p = spine({ ...view, state: { ...view.state, stage: "new" }, runs: [] }, []);
    fireEvent.click(screen.getByRole("button", { name: "Start research" }));
    fireEvent.click(screen.getByRole("button", { name: "Ask agent" }));
    fireEvent.click(screen.getByRole("button", { name: "Skip to plan" }));
    expect(p.onStartRun).toHaveBeenNthCalledWith(1, "research");
    expect(p.onStartRun).toHaveBeenNthCalledWith(2, "plan");
    fireEvent.click(screen.getByRole("button", { name: "Ask agent" }));
    fireEvent.click(screen.getByRole("button", { name: "Skip to build" }));
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Continue" }));
    await vi.waitFor(() => expect(p.onStage).toHaveBeenCalledWith("approved"));
  });

  it("start build all at once or step by step", () => {
    const p = spine({ ...withPlan(view, "approved"), runs: [] }, []);
    fireEvent.click(screen.getByRole("button", { name: "Start build" }));
    fireEvent.click(screen.getByRole("button", { name: "Build step by step" }));
    expect(p.onStartRun).toHaveBeenNthCalledWith(1, "implement");
    expect(p.onStartRun).toHaveBeenNthCalledWith(2, "implement", { initialMode: "step-by-step" });
  });

  it("resume a stopped run from the recovery Notice", () => {
    const stopped: Run = { ...view.runs[0], id: "s1", status: "stopped", error: "Rate limited", stage: "implementing", task: "implement", provider_session_id: "ps" };
    const p = spine({ ...withPlan(view, "implementing"), runs: [stopped] }, []);
    fireEvent.click(screen.getByRole("button", { name: "Resume agent run" }));
    expect(p.onStartRun).toHaveBeenCalledWith("implement", { resumeId: "s1" });
  });

  it("an idle-stopped run is a pause, not a recovery, and can still be resumed", () => {
    const stopped: Run = { ...view.runs[0], id: "s2", status: "stopped", idle_stopped: true, stage: "implementing", task: "implement", provider_session_id: "ps" };
    const p = spine({ ...withPlan(view, "implementing"), runs: [stopped] }, []);
    expect(screen.queryByText(/Recovery needed/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Resume the stopped run" }));
    expect(p.onStartRun).toHaveBeenCalledWith("implement", { resumeId: "s2" });
  });

  it("onAskAgent, revise, critique and the critique presets live in the Ask agent menu", () => {
    const p = spine(withPlan(view, "plan-review"), [], { activeTab: "plan", planDoc });
    fireEvent.click(within(openMenu()).getByRole("button", { name: "Revise with agent" }));
    expect(p.onStartRun).toHaveBeenLastCalledWith("plan");
    fireEvent.click(within(openMenu()).getByRole("button", { name: "Review plan with agent" }));
    expect(p.onStartRun).toHaveBeenLastCalledWith("critique", undefined);
    fireEvent.click(within(openMenu()).getByRole("button", { name: "What could go wrong?" }));
    expect(p.onStartRun).toHaveBeenLastCalledWith("critique", { initialPrompt: expect.stringContaining("What could go wrong?") });
    fireEvent.click(within(openMenu()).getByRole("button", { name: "Ask agent…" }));
    expect(p.onAskAgent).toHaveBeenCalled();
  });

  it("decision summary jumps sit in the Notice of allowance", () => {
    const p = spine(withPlan(view, "plan-review"), [], { activeTab: "plan", planDoc });
    fireEvent.click(screen.getByRole("button", { name: "Risks" }));
    expect(p.onNavigate).toHaveBeenCalledWith(2);
  });

  it("onPr lives in the Ask agent menu; onOpenChanges and accept in the Notice", async () => {
    const p = spine(withPlan(view, "implementation-review"), [question], { activeTab: "changes" });
    fireEvent.click(within(openMenu()).getByRole("button", { name: "Create a PR…" }));
    expect(p.onPr).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Review changes" }));
    expect(p.onOpenChanges).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Accept and finish" }));
    const dialog = screen.getByRole("alertdialog");
    expect(dialog).toHaveTextContent("3 / 15 done");
    expect(dialog).toHaveTextContent("1 (0 yours)");
    expect(dialog).toHaveTextContent("The session moves to Done.");
    fireEvent.click(within(dialog).getByRole("button", { name: "Accept and finish" }));
    await vi.waitFor(() => expect(p.onStage).toHaveBeenCalledWith("done"));
  });

  it("live-run notice opens the run (onSelectRun)", () => {
    const live: Run = { ...view.runs[0], status: "running", stage: "researching", task: "research" };
    const p = spine({ ...view, state: { ...view.state, stage: "researching" }, runs: [live] });
    fireEvent.click(screen.getByRole("button", { name: "Open run" }));
    expect(p.onOpenRun).toHaveBeenCalledWith("r1");
  });

  it("session details and settings sit in the spine footer", () => {
    const p = spine();
    fireEvent.click(screen.getByRole("button", { name: "Session details" }));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(p.onDetails).toHaveBeenCalled();
    expect(p.onSettings).toHaveBeenCalled();
    expect(screen.getByRole("link", { name: "Documentation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Theme:/ })).toBeInTheDocument();
  });

  it("onBrief, workspace, Where, export, history, PR refresh, run any stage, set stage and delete live in DetailsDrawer", async () => {
    const p = details({ ...view, session: { ...view.session, brief: "Old", pull_request: { number: 7, url: "https://x/7", state: "OPEN", draft: false, updated_at: now } } });
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.change(screen.getByLabelText("Brief"), { target: { value: "New brief" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(p.onBrief).toHaveBeenCalledWith("New brief"));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(p.onRefreshPr).toHaveBeenCalled();
    expect(screen.getByText("Refresh workspace")).toBeInTheDocument();
    expect(screen.getByText(/Where · workspace/)).toBeInTheDocument();
    expect(screen.getByText("Export or import session")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Run any stage…" }));
    expect(p.onStartRun).toHaveBeenCalledWith("research");
    expect(screen.getByRole("button", { name: "Drop session" })).toBeInTheDocument();
  });

  it("onSendToRun, onJump and the thread actions live in the Comments drawer; runs in the Agent drawer", () => {
    const onSendToRun = vi.fn(async () => {});
    const onJump = vi.fn();
    render(<CommentsDrawer sessionKey="repo/x" comments={[question]} docs={{}} author="you" selected={null} onSelect={onJump} liveRun={view.runs[0]} reviewPrompt="Act" onSendToRun={onSendToRun} actions={actions()} busy={false} />);
    fireEvent.click(screen.getByRole("button", { name: /on research line 4/ }));
    expect(onJump).toHaveBeenCalledWith(question);
    const onSelectRun = vi.fn();
    render(<AgentDrawer onClose={() => {}} sessionKey="repo/x" runs={view.runs} selectedRun={null} lines={[]} onSelectRun={onSelectRun} onRemoveRun={async () => {}} onResume={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: /Open claude researching run/ }));
    expect(onSelectRun).toHaveBeenCalledWith("r1");
  });
});

it("announces the Notice heading politely and keeps one primary button", () => {
  spine();
  expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
  expect(screen.getByRole("status")).toHaveTextContent("Your turn: review the research");
  expect(document.querySelectorAll(".btn.primary")).toHaveLength(1);
  expect(screen.getByRole("button", { name: "Plan it" })).toBeEnabled();
});

it("approve plan always confirms with what gets pinned", async () => {
  const p = spine(withPlan(view, "plan-review"), [], { activeTab: "plan", planDoc });
  fireEvent.click(screen.getByRole("button", { name: "Approve plan" }));
  expect(p.onStage).not.toHaveBeenCalled();
  const dialog = screen.getByRole("alertdialog");
  expect(dialog).toHaveTextContent("plan.md rev 4f2c19a");
  expect(dialog).toHaveTextContent("0 (0 yours)");
  expect(dialog).toHaveTextContent("The stage becomes approved");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(p.onStage).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Approve plan" }));
  fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Approve plan" }));
  await vi.waitFor(() => expect(p.onStage).toHaveBeenCalledWith("approved"));
});

it("offers milestone approval after a step-by-step turn with an editable commit message", async () => {
  const changes = vi.spyOn(api, "changes").mockResolvedValue({ tool: "difftool", scope: "step", base: "abc", label: "since milestone 0", step_available: true, stat: [{ path: "a.rs", added: 9, deleted: 1 }, { path: "b.rs", added: 3, deleted: 0 }], review: null });
  vi.spyOn(api, "milestone").mockResolvedValue({ subject: "Milestone 1: add the route", dirty: true, milestone: 1, pending: true, live: true });
  const run = { ...view.runs[0], stage: "implementing" as const, task: "implement", status: "idle" as const, implementation_mode: "step-by-step" as const, milestone_pending: true };
  const p = spine({ ...view, state: { ...view.state, stage: "implementing" }, runs: [run] });
  const message = await screen.findByDisplayValue("Milestone 1: add the route");
  expect(changes).toHaveBeenCalledWith("repo/x", "step");
  expect(await screen.findByText("2 files")).toBeInTheDocument();
  expect(screen.getByText("+12")).toBeInTheDocument();
  expect(screen.getByText("−1")).toBeInTheDocument();
  fireEvent.change(message, { target: { value: "Milestone 1: guest home route" } });
  fireEvent.click(screen.getByText("Commit milestone 1 and continue"));
  expect(p.onApproveMilestone).toHaveBeenCalledWith(run, true, "Milestone 1: guest home route");
  expect(screen.queryByText("Accept and finish")).toBeNull();

  fireEvent.click(screen.getByLabelText("commit this milestone"));
  expect(screen.queryByLabelText("Commit subject")).toBeNull();
  fireEvent.click(screen.getByText("Approve milestone 1 and continue"));
  expect(p.onApproveMilestone).toHaveBeenLastCalledWith(run, false, "Milestone 1: guest home route");
});

it("shows the milestone commit's progress instead of the approve form while it runs", () => {
  vi.spyOn(api, "milestone").mockResolvedValue({ subject: "Milestone 1: add the route", dirty: true, milestone: 1, pending: true, live: true });
  const run = { ...view.runs[0], stage: "implementing" as const, task: "implement", status: "idle" as const, implementation_mode: "step-by-step" as const, milestone_pending: true };
  const commit = { id: "c1", scope: { kind: "milestone" as const, run_id: run.id }, phase: "hooks" as const, started_at: new Date(Date.now() - 12_000).toISOString(), lines: ["mypy.....running"] };
  const p = spine({ ...view, state: { ...view.state, stage: "implementing" }, runs: [run], commit });
  const progress = screen.getByRole("status", { name: "Committing milestone…" });
  expect(progress).toHaveTextContent("mypy.....running");
  expect(progress).toHaveTextContent("running hooks · 12s");
  expect(screen.queryByText(/milestone 1 and continue/)).toBeNull();
  fireEvent.click(screen.getByText("Cancel commit"));
  expect(p.onCancelCommit).toHaveBeenCalled();
});

it("offers approval for a stopped scoped milestone with an adaptive checkpoint", async () => {
  vi.spyOn(api, "milestone").mockResolvedValue({ subject: "Milestone 1: phase", dirty: true, milestone: 1, pending: true, live: false });
  const run = { ...view.runs[0], stage: "implementing" as const, task: "implement", status: "stopped" as const, implementation_mode: "all-at-once" as const, milestone_key: "Phase 1", milestone_pending: true };
  spine({ ...view, state: { ...view.state, stage: "implementing" }, runs: [run] });
  await screen.findByDisplayValue("Milestone 1: phase");
  expect(screen.getByText("Commit milestone 1 and continue")).toBeEnabled();
});

it("resumes the milestone review of a stopped step-by-step run", () => {
  const run = { ...view.runs[0], stage: "implementing" as const, task: "implement", status: "stopped" as const, implementation_mode: "step-by-step" as const, milestone_pending: true };
  const p = spine({ ...view, state: { ...view.state, stage: "implementing" }, runs: [run] });
  fireEvent.click(screen.getByRole("button", { name: "Resume milestone review" }));
  expect(p.onContinueMilestone).toHaveBeenCalledWith(run);
});

it("marks skipped steps and finishes every FIG when done", () => {
  spine({ ...view, state: { ...view.state, stage: "approved" }, docs: [{ ...view.docs[0], exists: false }, view.docs[1]] }, []);
  const states = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem").map((li) => li.querySelector(".fig-state")?.textContent);
  expect(states).toEqual(["skipped", "skipped", "current", "pending", "pending"]);
});

describe("first run in a brand-new session", () => {
  it("keeps Start research as the one primary and puts Copy and Skip in the Ask agent menu", async () => {
    const writeText = vi.fn(async () => {});
    Object.assign(navigator, { clipboard: { writeText } });
    const p = spine({ ...view, state: { ...view.state, stage: "new" }, runs: [] }, [], { prompt: "Research repo/x" });
    expect(screen.getByText("Start research runs a hosted agent from this page.")).toBeInTheDocument();
    expect(screen.getByText("Or copy the prompt into any agent in your terminal.")).toBeInTheDocument();
    const start = screen.getByRole("button", { name: "Start research" });
    expect(screen.queryByRole("button", { name: /Copy/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Skip to/ })).toBeNull();
    fireEvent.click(start);
    expect(p.onStartRun).toHaveBeenCalledWith("research");
    fireEvent.click(screen.getByRole("button", { name: "Ask agent" }));
    const menu = screen.getByRole("dialog", { name: "Ask agent" });
    expect(within(menu).getByRole("button", { name: "Skip to build" })).toBeInTheDocument();
    fireEvent.click(within(menu).getByRole("button", { name: "Copy the research prompt" }));
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith("Research repo/x"));
  });

  it("hides Copy prompt until the prompt has loaded", () => {
    spine({ ...view, state: { ...view.state, stage: "new" }, runs: [] }, []);
    fireEvent.click(screen.getByRole("button", { name: "Ask agent" }));
    expect(screen.queryByRole("button", { name: /Copy/ })).toBeNull();
  });
});

describe("disabled primaries carry a reason", () => {
  it("names the reason under a disabled Notice primary", () => {
    spine(withPlan(view, "plan-review"), [], { activeTab: "plan", planDoc, busy: true });
    const button = screen.getByRole("button", { name: "Approve plan" });
    expect(button).toBeDisabled();
    expect(button).toHaveAccessibleDescription("Working…");
  });

  it("asks for a commit subject when it is empty", async () => {
    vi.spyOn(api, "changes").mockResolvedValue({ tool: "difftool", scope: "step", base: "abc", label: "since milestone 0", step_available: true, stat: [], review: null });
    vi.spyOn(api, "milestone").mockResolvedValue({ subject: "Milestone 1", dirty: true, milestone: 1, pending: true, live: true });
    const run = { ...view.runs[0], stage: "implementing" as const, task: "implement", status: "idle" as const, implementation_mode: "step-by-step" as const, milestone_pending: true };
    spine({ ...view, state: { ...view.state, stage: "implementing" }, runs: [run] });
    fireEvent.change(await screen.findByDisplayValue("Milestone 1"), { target: { value: " " } });
    const button = screen.getByRole("button", { name: "Commit milestone 1 and continue" });
    expect(button).toBeDisabled();
    expect(button).toHaveAccessibleDescription("Write a commit subject, or untick commit");
  });
});

it("a opens the approval confirmation and never approves directly", async () => {
  const p = spine(withPlan(view, "plan-review"), [], { activeTab: "plan", planDoc });
  fireEvent.keyDown(window, { key: "a" });
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog).toHaveTextContent("Approve the plan");
  expect(p.onStage).not.toHaveBeenCalled();
});

it("a does nothing when the primary is not a confirmed approval", () => {
  spine();
  fireEvent.keyDown(window, { key: "a" });
  expect(screen.queryByRole("alertdialog")).toBeNull();
});

it("keeps the evidence and summary before the primary in reading order behind a toggle", () => {
  spine(withPlan(view, "plan-review"), [], { activeTab: "plan", planDoc });
  const toggle = screen.getByRole("button", { name: "Show evidence and actions" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  const detail = document.getElementById(toggle.getAttribute("aria-controls") ?? "") as HTMLElement;
  const primary = screen.getByRole("button", { name: "Approve plan" });
  expect(detail.compareDocumentPosition(primary) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  fireEvent.click(toggle);
  expect(screen.getByRole("button", { name: "Show less" })).toHaveAttribute("aria-expanded", "true");
});

it("the Notice owns a pending plan revision and accepts it through the same confirmation", async () => {
  vi.spyOn(api, "changes").mockResolvedValue({ tool: "difftool", scope: "step", base: "abc", label: "since milestone 0", step_available: true, stat: [], review: null });
  vi.spyOn(api, "milestone").mockResolvedValue({ subject: "M1", dirty: true, milestone: 1, pending: true, live: true });
  const run = { ...view.runs[0], stage: "implementing" as const, task: "implement", status: "idle" as const, implementation_mode: "step-by-step" as const, milestone_pending: true };
  const v = withPlan(view, "implementing");
  const p = spine({ ...v, state: { ...v.state, plan_revision_pending: true }, runs: [run] }, [], { planDoc });
  expect(screen.getByRole("status")).toHaveTextContent("Your turn: accept the plan revision");
  expect(screen.getByText(/Accept the revision, then commit milestone 1 and continue/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Commit milestone/ })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Accept plan revision" }));
  const dialog = screen.getByRole("alertdialog");
  expect(dialog).toHaveTextContent("plan.md rev 4f2c19a");
  fireEvent.click(within(dialog).getByRole("button", { name: "Accept plan revision" }));
  await vi.waitFor(() => expect(p.onStage).toHaveBeenCalledWith("approved"));
});

it("the FIG strip switches tabs and the Inbox link counts sessions waiting on you", () => {
  const p = spine(withPlan(view, "plan-review"), [], { activeTab: "plan", planDoc, onTab: vi.fn(), waiting: 3 });
  expect(screen.getByRole("link", { name: /Inbox · 3 waiting on you/ })).toBeInTheDocument();
  const plan = screen.getByRole("button", { name: /^FIG\. 2 Plan/ });
  expect(plan).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: /^FIG\. 1 Research/ }));
  fireEvent.click(screen.getByRole("button", { name: /^FIG\. 4 Review/ }));
  expect(p.onTab).toHaveBeenNthCalledWith(1, "research");
  expect(p.onTab).toHaveBeenNthCalledWith(2, "changes");
});

it("hides the waiting count when there is none", () => {
  spine(view, [], { waiting: null });
  expect(screen.getByRole("link", { name: "Inbox" })).toBeInTheDocument();
});
