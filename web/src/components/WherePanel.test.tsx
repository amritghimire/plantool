import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import type { SessionView } from "../types";
import { WherePanel } from "./WherePanel";
it("sends changed context and preserves a plain language pause rule", async () => {
  const view = { key: "repo/session", workspace_branch: "main", session: { repo: { root: "/repo" }, base: "main", worktree: null, pause_rule: { mode: "plain-language", rule: "Pause before API changes" } }, state: { proposals: [] } } as unknown as SessionView;
  const updated = vi.fn();
  const save = vi.spyOn(api, "setContext").mockResolvedValue({ view } as Awaited<ReturnType<typeof api.setContext>>);
  render(<WherePanel view={view} onUpdated={updated} />);
  fireEvent.change(screen.getByLabelText("Base"), { target: { value: "HEAD~1" } });
  fireEvent.click(screen.getByText("Apply context change"));
  await waitFor(() => expect(save).toHaveBeenCalledWith("repo/session", expect.objectContaining({ base: "HEAD~1", branch: undefined, workspace: undefined, pause_rule: { mode: "plain-language", rule: "Pause before API changes" } })));
  expect(updated).toHaveBeenCalledWith(view);
});
