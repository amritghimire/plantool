import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import { PrDialog } from "./PrDialog";

const preview = { head: "fix-it", base: "main", repository: "owner/repo", push_remote: "origin", dirty: true, commits_ahead: 1, existing: null };

afterEach(() => vi.restoreAllMocks());

it("requires an explicit PR mode and saves edits before create", async () => {
  vi.spyOn(api, "prDraft").mockResolvedValue({ title: "Fix it", body: "This fixes the issue.", exists: true });
  vi.spyOn(api, "prPreview").mockResolvedValue(preview);
  const save = vi.spyOn(api, "savePrDraft").mockResolvedValue({ saved: true });
  const create = vi.spyOn(api, "createPr").mockResolvedValue({ pull_request: { number: 7, url: "https://github.com/owner/repo/pull/7", state: "OPEN", draft: true, updated_at: "now" }, session: {} as never });
  render(<PrDialog sessionKey="repo/x" onClose={() => {}} onDraftAgent={() => {}} onSaved={() => {}} />);
  expect(await screen.findByText(/Uncommitted changes will not appear/)).toBeInTheDocument();
  await screen.findByDisplayValue("This fixes the issue.");
  const button = screen.getByRole("button", { name: "Create PR" });
  expect(button).toBeDisabled();
  fireEvent.click(screen.getByLabelText("Draft", { selector: "input" }));
  fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Fix the session view" } });
  fireEvent.click(button);
  await waitFor(() => expect(create).toHaveBeenCalledWith("repo/x", { head: "fix-it", repository: "owner/repo", title: "Fix the session view", body: "This fixes the issue.", draft: true }));
  expect(save.mock.invocationCallOrder[0]).toBeLessThan(create.mock.invocationCallOrder[0]);
});

it("keeps creation disabled while GitHub is unavailable", async () => {
  vi.spyOn(api, "prDraft").mockResolvedValue({ title: "Fix it", body: "Body", exists: true });
  vi.spyOn(api, "prPreview").mockRejectedValue(new Error("gh is signed out"));
  render(<PrDialog sessionKey="repo/x" onClose={() => {}} onDraftAgent={() => {}} onSaved={() => {}} />);
  expect(await screen.findByText("gh is signed out")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Create PR" })).toBeDisabled();
});
