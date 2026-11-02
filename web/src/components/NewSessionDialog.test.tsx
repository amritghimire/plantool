import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { api } from "../api";
import { NewSessionDialog } from "./NewSessionDialog";

afterEach(() => vi.restoreAllMocks());

it("suggests a session ID from the title until the ID is edited", () => {
  vi.spyOn(api, "repos").mockResolvedValue({ repos: [] });
  render(<MemoryRouter><NewSessionDialog onClose={() => {}} /></MemoryRouter>);
  const title = screen.getByPlaceholderText("Fix the login timeout");
  const slug = screen.getByPlaceholderText("fix-login-timeout");
  fireEvent.change(title, { target: { value: "Fix login timeout" } });
  expect(slug).toHaveValue("fix-login-timeout");
  fireEvent.change(slug, { target: { value: "auth-timeout" } });
  fireEvent.change(title, { target: { value: "Fix login timeout today" } });
  expect(slug).toHaveValue("auth-timeout");
});

const provider = (id: string, available: boolean) => ({ id, available, models: [] });

it("shows which agents are ready without blocking Create", async () => {
  vi.spyOn(api, "repos").mockResolvedValue({ repos: [{ root: "/repo", branch: "main", sessions: 0, last_used: "2026-01-01T00:00:00Z" }] as never });
  vi.spyOn(api, "providers").mockResolvedValue({ providers: [provider("claude", true), provider("codex", false), provider("copilot", false), provider("ollama", true)] });
  render(<MemoryRouter><NewSessionDialog onClose={() => {}} /></MemoryRouter>);
  const list = await screen.findByRole("list", { name: "Agent providers" });
  expect(list).toHaveTextContent("Claude Code ready");
  expect(list).toHaveTextContent("Codex not ready");
  expect(list).toHaveTextContent("Ollama via OpenCode ready");
  expect(screen.queryByText(/No agent can run from the browser/)).toBeNull();
  fireEvent.change(screen.getByPlaceholderText("Fix the login timeout"), { target: { value: "Fix it" } });
  expect(screen.getByRole("button", { name: "Create session" })).toBeEnabled();
});

it("keeps the copy-prompt path open and shows setup when no agent is ready", async () => {
  vi.spyOn(api, "repos").mockResolvedValue({ repos: [] });
  vi.spyOn(api, "providers").mockResolvedValue({ providers: [provider("claude", false), provider("codex", false)] });
  render(<MemoryRouter><NewSessionDialog onClose={() => {}} /></MemoryRouter>);
  expect(await screen.findByText(/No agent can run from the browser yet. You can still create the session and paste its prompt/)).toBeInTheDocument();
  expect(screen.getByText("Agent setup")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Copy setup command" })).toHaveLength(2);
});

it("says why Create is disabled for every missing field", async () => {
  vi.spyOn(api, "repos").mockResolvedValue({ repos: [] });
  vi.spyOn(api, "providers").mockResolvedValue({ providers: [] });
  render(<MemoryRouter><NewSessionDialog onClose={() => {}} /></MemoryRouter>);
  const create = screen.getByRole("button", { name: "Create session" });
  expect(create).toHaveAccessibleDescription("Loading repositories…");
  await vi.waitFor(() => expect(create).toHaveAccessibleDescription("Enter a repository path"));
  fireEvent.change(screen.getByPlaceholderText("/absolute/path/to/a/git/checkout"), { target: { value: "/repo" } });
  expect(create).toHaveAccessibleDescription("Add a title");
  fireEvent.change(screen.getByPlaceholderText("fix-login-timeout"), { target: { value: "-bad" } });
  expect(create).toHaveAccessibleDescription("Fix the session ID");
  expect(create).toBeDisabled();
  fireEvent.change(screen.getByPlaceholderText("fix-login-timeout"), { target: { value: "good" } });
  expect(create).toBeEnabled();
  expect(create).not.toHaveAttribute("aria-describedby");
});
