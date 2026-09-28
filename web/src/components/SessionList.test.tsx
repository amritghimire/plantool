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

  expect(await screen.findByText("/repo", { selector: ".workspace-path" })).toBeInTheDocument();
  expect(screen.getByText("/repo/.worktree/two", { selector: ".workspace-path" })).toBeInTheDocument();
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
