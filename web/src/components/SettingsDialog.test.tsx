import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import type { WorktreeDirSetting } from "../types";
import { SettingsDialog } from "./SettingsDialog";

const setting: WorktreeDirSetting = {
  key: "plantool.worktreeDir",
  default: ".worktree/{slug}",
  global: null,
  repo: { root: "/code/app", value: null },
  effective: ".worktree/{slug}",
  example: "/code/app/.worktree/<slug>",
};

beforeEach(() => {
  vi.spyOn(api, "repos").mockResolvedValue({ repos: [{ root: "/code/app", repo_slug: "app", branch: "main", base: "main", sessions: 1, last_used: "2026-01-01T00:00:00Z" }] });
});

it("previews and saves the global worktree location", async () => {
  const get = vi.spyOn(api, "worktreeDir").mockImplementation(async (_repo, preview) =>
    preview === "../{repo}-worktrees/{slug}" ? { ...setting, example: "/code/app-worktrees/<slug>" } : setting,
  );
  const set = vi.spyOn(api, "setWorktreeDir").mockResolvedValue({ ...setting, global: "../{repo}-worktrees/{slug}" });
  render(<SettingsDialog onClose={() => {}} />);

  const input = await screen.findByPlaceholderText(".worktree/{slug}");
  expect(await screen.findByText("/code/app/.worktree/<slug>")).toBeInTheDocument();
  fireEvent.change(input, { target: { value: "../{repo}-worktrees/{slug}" } });
  expect(await screen.findByText("/code/app-worktrees/<slug>")).toBeInTheDocument();
  expect(get).toHaveBeenCalledWith("/code/app", "../{repo}-worktrees/{slug}");

  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(set).toHaveBeenCalledWith("global", "../{repo}-worktrees/{slug}", "/code/app"));
  expect(await screen.findByText("Saved")).toBeInTheDocument();
});

it("saves a per-repository override", async () => {
  vi.spyOn(api, "worktreeDir").mockResolvedValue({ ...setting, global: "~/wt/{slug}" });
  const set = vi.spyOn(api, "setWorktreeDir").mockResolvedValue({ ...setting, repo: { root: "/code/app", value: "trees" } });
  render(<SettingsDialog onClose={() => {}} />);

  const select = await screen.findByRole("combobox");
  await screen.findByRole("option", { name: "/code/app only" });
  fireEvent.change(select, { target: { value: "/code/app" } });
  const input = await screen.findByPlaceholderText("~/wt/{slug}");
  fireEvent.change(input, { target: { value: "trees" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(set).toHaveBeenCalledWith("repo", "trees", "/code/app"));
});
