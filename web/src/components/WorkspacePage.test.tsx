import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { api } from "../api";
import { WorkspacePage } from "./WorkspacePage";
const value = {
  projects: [
    { id: "p", name: "Alpha" },
    { id: "q", name: "Beta" },
  ],
  threads: [
    {
      id: "general",
      project: "p",
      item: null,
      channel: "general",
      title: "#general",
      messages: [
        {
          id: "m",
          sender: "cto",
          body: "Alpha context",
          mentions: [],
          at: "2026-10-24T12:00:00Z",
        },
      ],
    },
    {
      id: "item",
      project: "p",
      item: "p:1",
      channel: null,
      title: "Auth contract",
      messages: [
        {
          id: "n",
          sender: "backend",
          body: "Review interface",
          mentions: [],
          at: "2026-10-24T12:00:00Z",
        },
      ],
    },
    {
      id: "beta",
      project: "q",
      item: null,
      channel: "general",
      title: "#general",
      messages: [
        {
          id: "b",
          sender: "pm",
          body: "Beta context",
          mentions: [],
          at: "2026-10-24T12:00:00Z",
        },
      ],
    },
  ],
};
beforeEach(() => {
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  localStorage.clear();
  vi.spyOn(api, "workspace").mockResolvedValue(value);
});
it("scopes search and conversation to the selected project and keeps replies context-only", async () => {
  const reply = vi
    .spyOn(api, "workspaceReply")
    .mockResolvedValue({ id: "r", authority: "context_only" });
  render(
    <MemoryRouter initialEntries={["/workspace?project=p"]}>
      <WorkspacePage />
    </MemoryRouter>,
  );
  await screen.findByText("Alpha context");
  expect(screen.queryByText("Beta context")).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Owner reply · context only"), {
    target: { value: "approved" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send context" }));
  await waitFor(() =>
    expect(reply).toHaveBeenCalledWith(
      expect.objectContaining({ thread: "general", body: "approved" }),
    ),
  );
  expect(
    screen.queryByRole("button", { name: /approve/i }),
  ).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Project"), {
    target: { value: "q" },
  });
  await screen.findByText("Beta context");
  expect(screen.queryByText("Alpha context")).not.toBeInTheDocument();
});
it("searches thread messages and persists local unread markers when opening a thread", async () => {
  render(
    <MemoryRouter initialEntries={["/workspace?project=p"]}>
      <WorkspacePage />
    </MemoryRouter>,
  );
  await screen.findByText("Alpha context");
  await waitFor(() =>
    expect(
      JSON.parse(localStorage.getItem("plantool.workspace.read") ?? "{}").general,
    ).toBe("m"),
  );
  expect(
    within(screen.getByRole("button", { name: /Auth contract/ })).getByLabelText(
      "1 unread messages",
    ),
  ).toBeInTheDocument();
  expect(
    within(screen.getByRole("button", { name: /# general/ })).queryByLabelText(
      /unread messages/,
    ),
  ).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Search this project"), {
    target: { value: "interface" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Auth contract/ }));
  await screen.findByText("Review interface");
  await waitFor(() =>
    expect(
      JSON.parse(localStorage.getItem("plantool.workspace.read") ?? "{}").item,
    ).toBe("n"),
  );
});
