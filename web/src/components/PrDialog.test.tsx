import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import { PrDialog } from "./PrDialog";

const preview = { head: "fix-it", base: "main", repository: "owner/repo", push_remote: "origin", dirty: true, changed_files: ["src/a.ts"], live_run: false, commits_ahead: 1, existing: null };

beforeEach(() => {
  vi.spyOn(api, "prCommitDraft").mockResolvedValue({ message: "Fix it\n\nSession: fix-it", exists: false });
});
afterEach(() => vi.restoreAllMocks());

it("defaults to draft and saves edits before create", async () => {
  vi.spyOn(api, "prDraft").mockResolvedValue({ title: "Fix it", body: "This fixes the issue.", exists: true });
  vi.spyOn(api, "prPreview").mockResolvedValue(preview);
  const save = vi.spyOn(api, "savePrDraft").mockResolvedValue({ saved: true });
  const create = vi.spyOn(api, "createPr").mockResolvedValue({ pull_request: { number: 7, url: "https://github.com/owner/repo/pull/7", state: "OPEN", draft: true, updated_at: "now" }, session: {} as never });
  render(<PrDialog sessionKey="repo/x" commit={null} onCancelCommit={() => {}} onClose={() => {}} onDraftAgent={() => {}} onSaved={() => {}} />);
  expect(await screen.findByText(/will not be in the PR/)).toBeInTheDocument();
  await screen.findByDisplayValue("This fixes the issue.");
  expect(screen.getByLabelText("Draft", { selector: "input" })).toBeChecked();
  fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Fix the session view" } });
  const button = screen.getByRole("button", { name: "Create PR" });
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
  await waitFor(() => expect(create).toHaveBeenCalledWith("repo/x", { head: "fix-it", repository: "owner/repo", title: "Fix the session view", body: "This fixes the issue.", draft: true }));
  expect(save.mock.invocationCallOrder[0]).toBeLessThan(create.mock.invocationCallOrder[0]);
});

it("says why Create PR is disabled when only uncommitted changes exist", async () => {
  vi.spyOn(api, "prDraft").mockResolvedValue({ title: "Fix it", body: "Body", exists: true });
  vi.spyOn(api, "prPreview").mockResolvedValue({ ...preview, commits_ahead: 0 });
  render(<PrDialog sessionKey="repo/x" commit={null} onCancelCommit={() => {}} onClose={() => {}} onDraftAgent={() => {}} onSaved={() => {}} />);
  expect(await screen.findByText("Commit your changes first")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Create PR" })).toBeDisabled();
});

it("commits with the edited suggested message and reloads the checks", async () => {
  vi.spyOn(api, "prDraft").mockResolvedValue({ title: "Fix it", body: "Body", exists: true });
  const previewCall = vi.spyOn(api, "prPreview").mockResolvedValueOnce({ ...preview, commits_ahead: 0 }).mockResolvedValue({ ...preview, dirty: false, changed_files: [], commits_ahead: 1 });
  const commit = vi.spyOn(api, "prCommit").mockResolvedValue({ sha: "abcdef1234567890", rewritten: [] });
  render(<PrDialog sessionKey="repo/x" commit={null} onCancelCommit={() => {}} onClose={() => {}} onDraftAgent={() => {}} onSaved={() => {}} />);
  const message = await screen.findByDisplayValue(/Session: fix-it/);
  fireEvent.change(message, { target: { value: "Fix the session view" } });
  fireEvent.click(screen.getByRole("button", { name: "Commit all changes" }));
  await waitFor(() => expect(commit).toHaveBeenCalledWith("repo/x", "Fix the session view"));
  expect(await screen.findByText(/Committed abcdef123456/)).toBeInTheDocument();
  expect(previewCall).toHaveBeenCalledTimes(2);
  await waitFor(() => expect(screen.getByRole("button", { name: "Create PR" })).toBeEnabled());
});

it("keeps creation disabled while GitHub is unavailable", async () => {
  vi.spyOn(api, "prDraft").mockResolvedValue({ title: "Fix it", body: "Body", exists: true });
  vi.spyOn(api, "prPreview").mockRejectedValue(new Error("gh is signed out"));
  render(<PrDialog sessionKey="repo/x" commit={null} onCancelCommit={() => {}} onClose={() => {}} onDraftAgent={() => {}} onSaved={() => {}} />);
  expect(await screen.findByText("gh is signed out")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Create PR" })).toBeDisabled();
});

it("shows the running commit's output and cancels it", async () => {
  vi.spyOn(api, "prDraft").mockResolvedValue({ title: "Fix it", body: "Body", exists: true });
  vi.spyOn(api, "prPreview").mockResolvedValue({ ...preview, commits_ahead: 0 });
  const onCancelCommit = vi.fn();
  const commit = { id: "c1", scope: { kind: "pr" as const }, phase: "hooks" as const, started_at: new Date().toISOString(), lines: ["uv-lock....Passed", "ruff....Passed"] };
  render(<PrDialog sessionKey="repo/x" commit={commit} onCancelCommit={onCancelCommit} onClose={() => {}} onDraftAgent={() => {}} onSaved={() => {}} />);
  const progress = await screen.findByRole("status", { name: "Committing…" });
  expect(progress).toHaveTextContent("ruff....Passed");
  expect(screen.queryByText("Commit all changes")).toBeNull();
  fireEvent.click(screen.getByText("Cancel commit"));
  expect(onCancelCommit).toHaveBeenCalled();
});
