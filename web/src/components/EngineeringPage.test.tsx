import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { api } from "../api";
import { EngineeringPage, EngineeringItemPage } from "./EngineeringPage";
import type { EngineeringOverview, EngineeringDetail } from "../engineering";
vi.mock("./GithubSettings", () => ({ GithubSettings: () => null }));
const overview: EngineeringOverview = {
  projects: [
    {
      id: "p",
      name: "owner/alpha",
      checkout: "/alpha",
      visibility: "private",
      app_id: 1,
      installation_id: 2,
      branch: "main",
      synced_at: null,
      error: null,
      scheduler: "project_wait",
    },
    {
      id: "q",
      name: "owner/beta",
      checkout: "/beta",
      visibility: "public",
      app_id: 1,
      installation_id: 2,
      branch: "main",
      synced_at: null,
      error: null,
      scheduler: "project_wait",
    },
  ],
  items: [
    {
      id: "p:1",
      project: "p",
      title: "Alpha task",
      issue: 1,
      state: "backend",
      priority: null,
      pr: null,
      error: null,
      frozen: false,
      step: "backend",
      waiting_gates: 0,
    },
    {
      id: "q:2",
      project: "q",
      title: "Beta task",
      issue: 2,
      state: "plan_gate",
      priority: null,
      pr: null,
      error: null,
      frozen: false,
      step: "plan_gate",
      waiting_gates: 1,
    },
  ],
  active: [
    {
      id: "a",
      item: "p:1",
      project: "p",
      title: "Alpha task",
      role: "backend",
      state: "running",
      route: "codex-sol",
      started_at: "2026-10-24T12:00:00Z",
      provider: "openai",
      model: "gpt-6.1-sol",
    },
  ],
};
beforeEach(() => {
  vi.spyOn(api, "engineering").mockResolvedValue(overview);
  vi.spyOn(api, "workerStatus").mockResolvedValue({
    worker: { available: true, message: "Engineering OS available" },
  });
});
it("switches projects without mixing their items or attempts", async () => {
  render(
    <MemoryRouter initialEntries={["/engineering?project=p"]}>
      <EngineeringPage />
    </MemoryRouter>,
  );
  await screen.findByText("gpt-6.1-sol · openai");
  expect(screen.queryByText("Beta task")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /owner\/beta/ }));
  await screen.findByText("Beta task");
  expect(screen.queryByText("gpt-6.1-sol · openai")).not.toBeInTheDocument();
  expect(screen.getByText("Waiting for plan approval")).toBeInTheDocument();
});
it("shows persisted attempts, real graph history and GitHub gate links", async () => {
  const detail: EngineeringDetail = {
    item: { ...overview.items[0], step: "plan_gate", waiting_gates: 1 },
    repository: "owner/alpha",
    base: "base",
    head: "head",
    attempts: [
      {
        id: "a",
        role: "pm",
        generation: 1,
        state: "completed",
        route: "route",
        started_at: "2026-10-24T12:00:00Z",
        ended_at: null,
        summary: "Keyed plan recorded",
        provider: "ollama",
        model: "model",
        tokens_in: 10,
        tokens_out: 5,
        duration_ms: 3000,
      },
    ],
    gates: [
      {
        id: "g",
        kind: "plan",
        state: "open",
        hash: "abc123",
        sha: null,
        comment: 42,
        at: "now",
      },
    ],
    events: [
      {
        id: 1,
        at: "2026-10-24T12:00:00Z",
        action: "workflow.projected",
        step: "plan_review",
      },
    ],
    history_limit: 200,
  };
  vi.spyOn(api, "engineeringItem").mockResolvedValue(detail);
  render(
    <MemoryRouter initialEntries={["/engineering/items/p%3A1"]}>
      <Routes>
        <Route
          path="/engineering/items/:id"
          element={<EngineeringItemPage />}
        />
      </Routes>
    </MemoryRouter>,
  );
  await screen.findByText("Keyed plan recorded");
  fireEvent.click(screen.getByRole("button", { name: "Graph history" }));
  expect(screen.getByText("CTO plan review")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Approval gates" }));
  expect(
    screen.getByRole("link", { name: /Review on GitHub/ }),
  ).toHaveAttribute(
    "href",
    "https://github.com/owner/alpha/issues/1#issuecomment-42",
  );
  expect(
    screen.queryByRole("button", { name: /approve/i }),
  ).not.toBeInTheDocument();
});
