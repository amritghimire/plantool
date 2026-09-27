import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ChangesTab } from "./ChangesTab";
import { api } from "../api";
import type { ChangesResponse } from "../types";

const base: ChangesResponse = {
  tool: "difftool",
  scope: "step",
  base: "abc123",
  label: "since milestone 1 was approved",
  step_available: true,
  stat: [{ path: "src/auth.rs", added: 9, deleted: 1 }],
  review: null,
};

test("switches scope and expands a file to show its diff", async () => {
  const changes = vi.spyOn(api, "changes").mockImplementation(async (_key, scope) => (scope === "all" ? { ...base, scope: "all", label: "in the whole implementation" } : base));
  const file = vi.spyOn(api, "changesFile").mockResolvedValue({ path: "src/auth.rs", diff: "diff --git a/src/auth.rs b/src/auth.rs\n@@ -1,2 +1,3 @@\n context\n-old\n+new\n", truncated: false });
  render(<ChangesTab sessionKey="repo/x" nonce={0} />);
  await screen.findByText("src/auth.rs");
  expect(screen.getByText(/since milestone 1 was approved/)).toBeTruthy();

  fireEvent.click(screen.getByText("src/auth.rs"));
  await waitFor(() => expect(file).toHaveBeenCalledWith("repo/x", "src/auth.rs", "step"));
  const added = await screen.findByText("+new");
  expect(added.className).toContain("add");
  expect(screen.getByText("-old").className).toContain("del");
  expect(screen.getByText("@@ -1,2 +1,3 @@").className).toContain("hunk");

  fireEvent.click(screen.getByText("Whole implementation"));
  await screen.findByText(/in the whole implementation/);
  expect(changes).toHaveBeenLastCalledWith("repo/x", "all");
  expect(screen.queryByText("+new")).toBeNull();
});

test("hides the scope switch outside step-by-step implementation", async () => {
  vi.spyOn(api, "changes").mockResolvedValue({ ...base, scope: "all", step_available: false, label: "against main" });
  render(<ChangesTab sessionKey="repo/y" nonce={0} />);
  await screen.findByText(/against main/);
  expect(screen.queryByText("This milestone")).toBeNull();
});
