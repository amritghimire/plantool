import { fireEvent, render, screen, within } from "@testing-library/react";
import { Drawer, drawerFrom } from "./Drawer";
import { CommentsDrawer } from "./CommentsDrawer";
import { AgentDrawer } from "./AgentDrawer";
import { DetailsDrawer } from "./DetailsDrawer";
import type { Comment, DocResponse, SessionView } from "../types";

const now = new Date().toISOString();
const view: SessionView = {
  key: "repo/x",
  url_path: "/s/repo/x",
  dir: "/d",
  session: { repo_slug: "repo", slug: "x", title: "x", repo: { root: "/r", common_dir: "/r/.git", branch: "main" }, worktree: null, base: "main", mirror: false, created_at: now },
  state: { stage: "plan-review", comments: [], docs: {}, seq: 3, review: null, updated_at: "" },
  docs: [],
  runs: [{ id: "r1", provider: "claude", provider_session_id: null, stage: "researching", cwd: "/r", status: "idle", model: null, started_at: now, ended_at: null, error: null, seq: 1 }],
  open_comments: 1,
};
const question: Comment = { id: "q1", doc: "research", anchor: { line: 4, text: "Keep, hide, or remove Storage.", outdated: false }, body: "Keep, hide, or remove Storage?", kind: "agent", author: "agent", parent: null, resolved: false, created_at: now, updated_at: null, seq: 2 };
const actions = () => ({ reply: vi.fn(async () => {}), resolve: vi.fn(async () => {}), edit: vi.fn(async () => {}), remove: vi.fn(async () => {}) });

describe("Drawer", () => {
  it("is labelled, takes focus and closes on Escape but not while typing", () => {
    const onClose = vi.fn();
    render(<Drawer id="details" onClose={onClose}><input aria-label="field" /></Drawer>);
    const drawer = screen.getByRole("complementary", { name: "Session details" });
    expect(drawer).toHaveFocus();
    fireEvent.keyDown(screen.getByLabelText("field"), { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(drawer, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Close session details" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("only accepts known drawers from the URL", () => {
    expect(drawerFrom("comments")).toBe("comments");
    expect(drawerFrom("nope")).toBeNull();
    expect(drawerFrom(null)).toBeNull();
  });
});

describe("Comments drawer", () => {
  const plan: DocResponse = { kind: "plan", sha: "s", content: "# Plan\n\nFirst.\n\nSecond **bold** line.\n", captured_at: "", path: "/p", lines: 5, headings: [], progress: [] };
  const mine: Comment = { ...question, id: "m1", doc: "plan", anchor: { line: 5, text: "Second", outdated: false }, body: "Use **bold** here", kind: "human", author: "me", type: "blocker", seq: 3 };
  const outdated: Comment = { ...question, id: "o1", doc: "plan", anchor: { line: 3, text: "First.", outdated: true }, body: "old", seq: 4 };
  const orphan: Comment = { ...question, id: "x1", doc: "plan", anchor: { line: 40, text: "gone", outdated: false }, body: "orphaned", seq: 5 };
  function setup(over: { comments?: Comment[]; selected?: string | null } = {}) {
    const a = actions();
    const onSendToRun = vi.fn(async () => {});
    const onSelect = vi.fn();
    render(<CommentsDrawer sessionKey="repo/x" comments={over.comments ?? [question]} docs={{ plan }} author="me" selected={over.selected ?? null} onSelect={onSelect} liveRun={view.runs[0]} reviewPrompt="Act on the review comments" onSendToRun={onSendToRun} actions={a} busy={false} />);
    return { actions: a, onSendToRun, onSelect };
  }
  const chip = (name: RegExp) => screen.getByRole("button", { name });

  it("counts and filters open, blockers, outdated and mine", () => {
    setup({ comments: [question, mine, outdated, orphan, { ...question, id: "r1", resolved: true }] });
    expect(chip(/^Open 4$/)).toHaveAttribute("aria-pressed", "true");
    expect(chip(/^Blockers 1$/)).toBeInTheDocument();
    expect(chip(/^Outdated 2$/)).toBeInTheDocument();
    expect(chip(/^Mine 1$/)).toBeInTheDocument();
    const groups = screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent);
    expect(groups).toEqual(["research.md", "plan.md"]);
    fireEvent.click(chip(/^Outdated/));
    expect(screen.getByText("old")).toBeInTheDocument();
    expect(screen.getByText("orphaned")).toBeInTheDocument();
    expect(screen.queryByText("Keep, hide, or remove Storage?")).toBeNull();
    fireEvent.click(chip(/^Mine/));
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    fireEvent.click(chip(/^Blockers/));
    expect(screen.getByRole("button", { name: /blocker by me on plan line 5/ })).toBeInTheDocument();
  });

  it("starts each thread heading with its reference numeral and fills the selected one", () => {
    setup({ comments: [question, mine, outdated], selected: "m1" });
    const heads = screen.getAllByRole("button", { name: /^\d+, / });
    expect(heads.map((h) => h.querySelector(".cd-ref")?.textContent)).toEqual(["10", "12", "14"]);
    expect(heads[0].firstElementChild).toHaveClass("cd-ref");
    const selected = screen.getByRole("button", { name: /^14, blocker by me/ });
    expect(selected.closest(".cd-row")).toHaveClass("selected");
    expect(selected.querySelector(".cd-ref")).toHaveClass("blocker");
  });

  it("renders the preview as markdown, not raw asterisks", () => {
    setup({ comments: [{ ...mine, body: "Use **bold** here\n\n1. second paragraph" }] });
    const strong = screen.getByText("bold", { selector: "strong" });
    expect(strong.closest(".cd-preview")).not.toBeNull();
    expect(screen.queryByText(/\*\*/)).toBeNull();
    expect(screen.queryByText("second paragraph")).toBeNull();
  });

  it("tells agent and human authors apart", () => {
    setup({ comments: [{ ...question, author: "council" }, mine] });
    expect(screen.getByText("agent", { selector: ".cd-agent" })).toBeInTheDocument();
    expect(document.querySelector(".cd-author.human")).toHaveTextContent("me");
  });

  it("selects a row and shows the full thread for the selected one", async () => {
    const { onSelect, actions, onSendToRun } = setup({ selected: "q1" });
    fireEvent.click(screen.getByRole("button", { name: /question|note by agent/ }));
    expect(onSelect).toHaveBeenCalledWith(question);
    fireEvent.click(screen.getByText("Agreed, go ahead."));
    await vi.waitFor(() => expect(actions.reply).toHaveBeenCalledWith(question, "Agreed, go ahead."));
    fireEvent.click(screen.getByRole("button", { name: "Resolve" }));
    await vi.waitFor(() => expect(actions.resolve).toHaveBeenCalledWith(question, true));
    fireEvent.click(screen.getByTitle("Ask the running agent to answer this thread"));
    await vi.waitFor(() => expect(onSendToRun).toHaveBeenCalled());
    expect((onSendToRun.mock.calls[0] as unknown as [string])[0]).toContain("--parent q1");
  });

  it("sends the review prompt to the live run", async () => {
    const { onSendToRun } = setup();
    fireEvent.click(screen.getByText("Send comments to the running agent"));
    await vi.waitFor(() => expect(onSendToRun).toHaveBeenCalledWith("Act on the review comments"));
  });

  it("keeps resolved conversations behind a Resolved toggle", () => {
    setup({ comments: [{ ...question, resolved: true }] });
    expect(screen.queryByText("Keep, hide, or remove Storage?")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^Resolved 1$/ }));
    expect(screen.getByText("Keep, hide, or remove Storage?")).toBeInTheDocument();
  });
});

describe("Agent drawer", () => {
  beforeEach(() => localStorage.clear());
  it("keeps stopped runs collapsed until opened and shows failures", () => {
    const stopped = { ...view.runs[0], id: "old", status: "failed" as const, provider_session_id: "provider-session" };
    const onResume = vi.fn();
    render(<AgentDrawer onClose={() => {}} sessionKey="repo/x" runs={[view.runs[0], stopped]} selectedRun={null} lines={[]} onSelectRun={() => {}} onRemoveRun={async () => {}} onResume={onResume} />);
    expect(screen.getByRole("button", { name: /Finished runs/ })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: /Open claude researching run, failed/ })).toBeNull();
    expect(screen.getByText("1 failed")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Finished runs/ }));
    expect(screen.getByRole("button", { name: /Open claude researching run, failed/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Resume run" }));
    expect(onResume).toHaveBeenCalledWith(stopped);
  });

  it("draws run history bars as long as each run took, with the reason a run carries", () => {
    const long = { ...view.runs[0], id: "long", status: "stopped" as const, started_at: "2026-01-01T00:00:00Z", ended_at: "2026-01-01T04:00:00Z", idle_stopped: true };
    const short = { ...view.runs[0], id: "short", status: "failed" as const, started_at: "2026-01-02T00:00:00Z", ended_at: "2026-01-02T01:00:00Z", error: "codex exited" };
    const { container } = render(<AgentDrawer onClose={() => {}} sessionKey="repo/bars" runs={[long, short]} selectedRun={null} lines={[]} onSelectRun={() => {}} onRemoveRun={async () => {}} onResume={() => {}} />);
    const fills = [...container.querySelectorAll<HTMLElement>(".run-bar-fill")];
    expect(fills.map((f) => f.style.width)).toEqual(["25%", "100%"]);
    expect(fills[0]).toHaveClass("failed");
    expect(screen.getByText("4h 00m")).toBeInTheDocument();
    expect(screen.getByText("Stopped after sitting idle")).toBeInTheDocument();
    expect(screen.getByText("codex exited")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear finished" })).toBeInTheDocument();
  });

  it("confirms before clearing a run", async () => {
    const stopped = { ...view.runs[0], id: "old", status: "stopped" as const };
    const onRemoveRun = vi.fn(async () => {});
    localStorage.setItem("plantool.finished.repo/x", "open");
    render(<AgentDrawer onClose={() => {}} sessionKey="repo/x" runs={[stopped]} selectedRun={null} lines={[]} onSelectRun={() => {}} onRemoveRun={onRemoveRun} onResume={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Clear run transcript" }));
    expect(onRemoveRun).not.toHaveBeenCalled();
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    await vi.waitFor(() => expect(onRemoveRun).toHaveBeenCalledWith("old"));
  });
});

describe("Details drawer", () => {
  function setup(v: SessionView = view) {
    const p = { onBrief: vi.fn(async () => {}), onStage: vi.fn(async () => {}), onStartRun: vi.fn(), onRefreshPr: vi.fn(), onUpdated: vi.fn(), onWorkspace: vi.fn(), onDelete: vi.fn(async () => {}) };
    render(<DetailsDrawer view={v} busy={false} {...p} />);
    return p;
  }
  it("shows the session's active workspace", () => {
    setup();
    expect(screen.getByText("/r", { selector: ".workspace-path" })).toBeInTheDocument();
  });

  it("shows the worktree as the active workspace", () => {
    setup({ ...view, session: { ...view.session, worktree: "/r/.worktree/x" } });
    expect(screen.getByText("/r/.worktree/x", { selector: ".workspace-path" })).toBeInTheDocument();
  });

  it("asks before setting the stage", async () => {
    const p = setup();
    fireEvent.change(screen.getByLabelText("Set stage"), { target: { value: "done" } });
    const dialog = screen.getByRole("alertdialog");
    expect(dialog).toHaveTextContent("Plan review");
    expect(p.onStage).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Set to Done" }));
    await vi.waitFor(() => expect(p.onStage).toHaveBeenCalledWith("done"));
  });

  it("confirms session deletion once and makes worktree removal optional", () => {
    const p = setup({ ...view, session: { ...view.session, worktree: "/worktrees/x" } });
    fireEvent.click(screen.getByRole("button", { name: "Drop session" }));
    expect(screen.getByRole("alertdialog", { name: "Drop repo/x?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Drop session" }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Also remove worktree/ }));
    fireEvent.click(screen.getAllByRole("button", { name: "Drop session" }).at(-1)!);
    expect(p.onDelete).toHaveBeenCalledWith(true);
  });

  it("applies and dismisses agent workspace proposals", () => {
    setup({ ...view, state: { ...view.state, proposals: [{ id: "p1", kind: "base", old: "main", new: "dev", reason: "Rebased" }] } });
    expect(screen.getByText("Agent proposes base")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Apply proposal" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dismiss" })).toBeInTheDocument();
  });
});
