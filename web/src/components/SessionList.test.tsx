import { fireEvent, render, screen } from "@testing-library/react";
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

it("shows the workspace for sessions with and without a worktree", async () => {
  vi.spyOn(api, "sessions").mockResolvedValue([
    base,
    { ...base, key: "repo/two", url_path: "/s/repo/two", session: { ...base.session, slug: "two", title: "Two", worktree: "/repo/.worktree/two" } },
  ]);

  render(<MemoryRouter><SessionList /></MemoryRouter>);

  expect(await screen.findByText(".", { selector: ".workspace-path" })).toBeInTheDocument();
  expect(screen.getByText(/\.worktree\/two/, { selector: ".workspace-path" })).toBeInTheDocument();
});

it("refreshes run status when the window regains focus", async () => {
  const waiting: SessionView = { ...base, runs: [{ id: "r1", provider: "claude", provider_session_id: null, stage: "researching", cwd: "/repo", status: "waiting", model: null, started_at: "2026-01-01T00:00:00Z", ended_at: null, error: null, seq: 1 }] };
  const sessions = vi.spyOn(api, "sessions").mockResolvedValueOnce([base]).mockResolvedValue([waiting]);
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  await screen.findByText("One");
  fireEvent.focus(window);
  expect(await screen.findByText("Needs input")).toBeInTheDocument();
  expect(sessions).toHaveBeenCalledTimes(2);
});

it("filters by attention and keeps PR and drop actions separate", async () => {
  const needs = { ...base, key: "repo/review", url_path: "/s/repo/review", session: { ...base.session, slug: "review", title: "Review" }, state: { ...base.state, stage: "plan-review" as const } };
  const done = { ...base, key: "repo/done", url_path: "/s/repo/done", session: { ...base.session, slug: "done", title: "Done", pull_request: { number: 12, url: "https://github.com/o/r/pull/12", state: "OPEN", draft: false, updated_at: base.session.created_at } }, state: { ...base.state, stage: "done" as const } };
  vi.spyOn(api, "sessions").mockResolvedValue([base, needs, done]);
  const remove = vi.spyOn(api, "removeSession").mockResolvedValue({ removed: needs.key, worktree_removed: false });
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  expect(await screen.findByRole("link", { name: "Open PR #12" })).toHaveAttribute("href", "https://github.com/o/r/pull/12");
  fireEvent.click(screen.getByRole("button", { name: /Needs you/ }));
  expect(screen.getByRole("link", { name: "Open Review" })).toBeInTheDocument();
  expect(screen.queryByRole("link", { name: "Open Done" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Drop Review" }));
  const dialog = screen.getByRole("dialog", { name: "Drop repo/review?" });
  expect(dialog).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Drop Review" }));
  fireEvent.click(screen.getByRole("button", { name: "Drop session" }));
  await vi.waitFor(() => expect(remove).toHaveBeenCalledWith("repo/review", false));
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

it("puts sessions that need attention before recently updated work", async () => {
  const recent = { ...base, key: "repo/recent", url_path: "/s/repo/recent", session: { ...base.session, slug: "recent", title: "Recent" }, state: { ...base.state, updated_at: "2026-02-01T00:00:00Z" } };
  const review = { ...base, key: "repo/review", url_path: "/s/repo/review", session: { ...base.session, slug: "review", title: "Needs review" }, state: { ...base.state, stage: "plan-review" as const } };
  vi.spyOn(api, "sessions").mockResolvedValue([recent, review]);
  render(<MemoryRouter><SessionList /></MemoryRouter>);
  await screen.findByRole("link", { name: "Open Needs review" });
  expect(screen.getAllByRole("link", { name: /^Open (Needs review|Recent)$/ }).map((link) => link.getAttribute("aria-label"))).toEqual(["Open Needs review", "Open Recent"]);
});
