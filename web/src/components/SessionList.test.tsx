import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { api } from "../api";
import type { SessionView } from "../types";
import { SessionList } from "./SessionList";

const base: SessionView = {
  key: "repo/one",
  url_path: "/s/repo/one",
  dir: "/sessions/one",
  session: {
    repo_slug: "repo", slug: "one", title: "One", repo: { root: "/repo", common_dir: "/repo/.git", branch: "main" },
    worktree: null, base: "main", mirror: false, created_at: "2026-01-01T00:00:00Z",
  },
  state: { stage: "new", comments: [], docs: {}, seq: 0, review: null, updated_at: "2026-01-01T00:00:00Z" },
  docs: [], runs: [], open_comments: 0,
};

it("keeps the worktree path out of the row", async () => {
  vi.spyOn(api, "sessions").mockResolvedValue([
    { ...base, session: { ...base.session, worktree: "/repo/.worktree/one" } },
  ]);
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  await screen.findByRole("link", { name: "Open One" });
  expect(screen.queryByText(/\.worktree\/one/)).toBeNull();
  expect(screen.getByRole("heading", { level: 1, name: "Sessions" })).toBeInTheDocument();
  expect(screen.queryByText(/confidently approve/)).toBeNull();
});

it("refreshes run status when the window regains focus", async () => {
  const waiting: SessionView = { ...base, runs: [{ id: "r1", provider: "claude", provider_session_id: null, stage: "researching", cwd: "/repo", status: "waiting", model: null, started_at: "2026-01-01T00:00:00Z", ended_at: null, error: null, seq: 1 }] };
  const sessions = vi.spyOn(api, "sessions").mockResolvedValueOnce([base]).mockResolvedValue([waiting]);
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  await screen.findByText("One");
  fireEvent.focus(window);
  expect(await screen.findByText("Agent needs your answer", { selector: ".row-reason" })).toBeInTheDocument();
  expect(sessions).toHaveBeenCalledTimes(2);
});

it("drops a session from the row menu and keeps the PR link separate", async () => {
  const needs = { ...base, key: "repo/review", url_path: "/s/repo/review", session: { ...base.session, slug: "review", title: "Review" }, state: { ...base.state, stage: "plan-review" as const } };
  const done = { ...base, key: "repo/done", url_path: "/s/repo/done", session: { ...base.session, slug: "done", title: "Done", pull_request: { number: 12, url: "https://github.com/o/r/pull/12", state: "OPEN", draft: false, updated_at: base.session.created_at } }, state: { ...base.state, stage: "done" as const } };
  vi.spyOn(api, "sessions").mockResolvedValue([base, needs, done]);
  const remove = vi.spyOn(api, "removeSession").mockResolvedValue({ removed: needs.key, worktree_removed: false });
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  expect(await screen.findByRole("link", { name: "Open PR #12" })).toHaveAttribute("href", "https://github.com/o/r/pull/12");
  expect(screen.queryByRole("button", { name: /^Drop/ })).toBeNull();
  const openMenu = () => {
    fireEvent.pointerDown(screen.getByRole("button", { name: "More for Review" }), { button: 0, ctrlKey: false });
    fireEvent.click(screen.getByRole("button", { name: "More for Review" }));
  };
  openMenu();
  fireEvent.click(await screen.findByRole("button", { name: "Drop session…" }));
  const dialog = screen.getByRole("alertdialog", { name: "Drop repo/review?" });
  expect(dialog).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(screen.queryByRole("alertdialog")).toBeNull();
  openMenu();
  fireEvent.click(await screen.findByRole("button", { name: "Drop session…" }));
  fireEvent.click(screen.getByRole("button", { name: "Drop session" }));
  await vi.waitFor(() => expect(remove).toHaveBeenCalledWith("repo/review", false));
});

it("disables dropping while a run is live", async () => {
  const live: SessionView = { ...base, runs: [{ id: "r1", provider: "claude", provider_session_id: null, stage: "researching", cwd: "/repo", status: "running", model: null, started_at: "2026-01-01T00:00:00Z", ended_at: null, error: null, seq: 1 }] };
  vi.spyOn(api, "sessions").mockResolvedValue([live]);
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  const trigger = await screen.findByRole("button", { name: "More for One" });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.click(trigger);
  expect(await screen.findByRole("button", { name: "Drop session…" })).toBeDisabled();
  expect(screen.getByText(/Stop the live run/)).toBeInTheDocument();
});

it("filters by repository through the URL", async () => {
  const other = { ...base, key: "other/x", url_path: "/s/other/x", session: { ...base.session, repo_slug: "other", slug: "x", title: "Elsewhere" } };
  vi.spyOn(api, "sessions").mockResolvedValue([base, other]);
  render(<MemoryRouter initialEntries={["/?repo=other"]}><SessionList /></MemoryRouter>);
  expect(await screen.findByRole("link", { name: "Open Elsewhere" })).toBeInTheDocument();
  expect(screen.queryByRole("link", { name: "Open One" })).toBeNull();
  fireEvent.change(screen.getByRole("combobox", { name: "Repository" }), { target: { value: "" } });
  expect(screen.getByRole("link", { name: "Open One" })).toBeInTheDocument();
});

it("caps recently done and moves focus with j and k", async () => {
  const dones = Array.from({ length: 12 }, (_, i) => ({ ...base, key: `repo/d${i}`, url_path: `/s/repo/d${i}`, session: { ...base.session, slug: `d${i}`, title: `Done ${i}` }, state: { ...base.state, stage: "done" as const } }));
  vi.spyOn(api, "sessions").mockResolvedValue(dones);
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  await screen.findByRole("link", { name: "Open Done 0" });
  expect(screen.getByText("Nothing is waiting on you.")).toBeInTheDocument();
  expect(screen.getAllByRole("link", { name: /^Open Done/ })).toHaveLength(10);
  fireEvent.click(screen.getByRole("button", { name: "Show all 12" }));
  expect(screen.getAllByRole("link", { name: /^Open Done/ })).toHaveLength(12);
  fireEvent.keyDown(window, { key: "j" });
  fireEvent.keyDown(window, { key: "j" });
  expect(screen.getByRole("link", { name: "Open Done 1" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "k" });
  expect(screen.getByRole("link", { name: "Open Done 0" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "/" });
  expect(screen.getByRole("searchbox", { name: "Search sessions" })).toHaveFocus();
});

it("finds a session by its brief and offers a way to clear the search", async () => {
  vi.spyOn(api, "sessions").mockResolvedValue([base, { ...base, key: "repo/two", url_path: "/s/repo/two", session: { ...base.session, slug: "two", title: "Second task", brief: "Repair the login timeout" } }]);
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  await screen.findByRole("link", { name: "Open One" });
  fireEvent.change(screen.getByRole("searchbox", { name: "Search sessions" }), { target: { value: "login" } });
  expect(screen.getByRole("link", { name: "Open Second task" })).toBeInTheDocument();
  expect(screen.queryByRole("link", { name: "Open One" })).toBeNull();
  fireEvent.change(screen.getByRole("searchbox", { name: "Search sessions" }), { target: { value: "missing" } });
  fireEvent.click(screen.getByRole("button", { name: "Clear search and filters" }));
  expect(screen.getByRole("link", { name: "Open One" })).toBeInTheDocument();
});

it("puts sessions waiting on you before recently updated work", async () => {
  const recent = { ...base, key: "repo/recent", url_path: "/s/repo/recent", session: { ...base.session, slug: "recent", title: "Recent" }, state: { ...base.state, updated_at: "2026-02-01T00:00:00Z" } };
  const review = { ...base, key: "repo/review", url_path: "/s/repo/review", session: { ...base.session, slug: "review", title: "Needs review" }, state: { ...base.state, stage: "research-review" as const } };
  vi.spyOn(api, "sessions").mockResolvedValue([recent, review]);
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  await screen.findByRole("link", { name: "Open Needs review" });
  expect(screen.getAllByRole("link", { name: /^Open (Needs review|Recent)$/ }).map((link) => link.getAttribute("href"))).toEqual(["/s/repo/review", "/s/repo/recent"]);
  expect(screen.getByRole("heading", { name: "Waiting on you 1" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Not started or idle 1" })).toBeInTheDocument();
});

it("explains the five FIGs and human-only approval on first run, with one New session action", async () => {
  vi.spyOn(api, "sessions").mockResolvedValue([]);
  vi.spyOn(api, "repos").mockResolvedValue({ repos: [] });
  vi.spyOn(api, "providers").mockResolvedValue({ providers: [] });
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  const panel = await screen.findByRole("region", { name: "No sessions yet" });
  expect(within(panel).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "FIG. 1 Research",
    "FIG. 2 Plan",
    "FIG. 3 Build",
    "FIG. 4 Review",
    "FIG. 5 Done",
  ]);
  expect(panel).toHaveTextContent("You comment on any line");
  expect(panel).toHaveTextContent("Only you can approve, here in the browser. The CLI never can.");
  expect(within(panel).getByRole("link", { name: "How it works" })).toHaveAttribute("href", "https://github.com/amritghimire/plantool#readme");
  expect(screen.getAllByRole("button", { name: "New session" })).toHaveLength(1);
  fireEvent.click(within(panel).getByRole("button", { name: "New session" }));
  expect(await screen.findByRole("button", { name: "Create session" })).toBeInTheDocument();
});

it("shows the no-match state, not the first-run panel, when a search finds nothing", async () => {
  vi.spyOn(api, "sessions").mockResolvedValue([base]);
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  await screen.findByRole("link", { name: "Open One" });
  fireEvent.change(screen.getByRole("searchbox", { name: "Search sessions" }), { target: { value: "zzz" } });
  expect(screen.getByRole("heading", { name: "No matching sessions" })).toBeInTheDocument();
  expect(screen.getByText("zzz")).toBeInTheDocument();
  expect(screen.queryByText("No sessions yet")).toBeNull();
});
