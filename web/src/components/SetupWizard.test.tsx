import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { api } from "../api";
import { SetupWizard } from "./SetupWizard";
vi.mock("./WorkerSettings", () => ({
  WorkerSettings: () => <p>Worker controls</p>,
}));
vi.mock("./ModelSettings", () => ({
  ModelSettings: () => <p>Model controls</p>,
}));
vi.mock("./GithubSettings", () => ({ GithubSettings: () => null }));
vi.mock("./OversightSettings", () => ({ OversightSettings: () => null }));
vi.mock("./LoopSettings", () => ({ LoopSettings: () => null }));
beforeEach(() => {
  vi.spyOn(api, "engineering").mockResolvedValue({
    projects: [],
    items: [],
    active: [],
  });
  vi.spyOn(api, "workerStatus").mockResolvedValue({
    worker: { available: true, message: "Ready" },
  });
});
it("resumes the daemon-saved step and saves the next step with its project", async () => {
  let cursor = { step: "worker", project: "p" };
  vi.spyOn(api, "setupProgress").mockImplementation(async () => cursor);
  const save = vi
    .spyOn(api, "saveSetupProgress")
    .mockImplementation(async (p) => (cursor = p as typeof cursor));
  const first = render(
    <MemoryRouter>
      <SetupWizard />
    </MemoryRouter>,
  );
  await screen.findByText("Worker controls");
  expect(screen.getByRole("button", { name: /First run/ })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Try one small issue first");
  expect(save).toHaveBeenCalledWith({ step: "ready", project: "p" });
  first.unmount();
  render(
    <MemoryRouter>
      <SetupWizard />
    </MemoryRouter>,
  );
  await screen.findByText("Try one small issue first");
  expect(screen.getByText("STEP 7 OF 7")).toBeInTheDocument();
});
it("keeps the current step when saving fails instead of silently losing progress", async () => {
  vi.spyOn(api, "setupProgress").mockResolvedValue({
    step: "welcome",
    project: null,
  });
  vi.spyOn(api, "saveSetupProgress").mockRejectedValue(
    new Error("Could not save setup progress"),
  );
  render(
    <MemoryRouter>
      <SetupWizard />
    </MemoryRouter>,
  );
  await screen.findByText("Your engineering team, across projects");
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Could not save setup progress",
    ),
  );
  expect(screen.getByText("STEP 1 OF 7")).toBeInTheDocument();
});

it("does not advance when a role has no verified model route", async () => {
  vi.spyOn(api, "setupProgress").mockResolvedValue({
    step: "models",
    project: null,
  });
  const save = vi
    .spyOn(api, "saveSetupProgress")
    .mockResolvedValue({ step: "github", project: null });
  vi.spyOn(api, "executionStatus").mockResolvedValue({
    execution: { stopped: true },
  } as Awaited<ReturnType<typeof api.executionStatus>>);
  vi.spyOn(api, "verifyModels").mockResolvedValue(
    {} as Awaited<ReturnType<typeof api.verifyModels>>,
  );
  vi.spyOn(api, "engineering").mockResolvedValue({
    projects: [],
    items: [],
    active: [],
    roles: [
      "cto",
      "pm",
      "backend",
      "frontend",
      "devops",
      "qa",
      "reviewer",
      "docs",
      "supervisor",
    ].map((role) => ({ role, ready: role !== "docs" })),
  });
  render(
    <MemoryRouter>
      <SetupWizard />
    </MemoryRouter>,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Verify and continue" }),
  );
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent(
      "No verified route for: docs",
    ),
  );
  expect(save).not.toHaveBeenCalled();
});
