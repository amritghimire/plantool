import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import { AskAgentDialog } from "./AskAgentDialog";

afterEach(() => vi.restoreAllMocks());

it("uploads an attachment and sends its path with the request", async () => {
  vi.spyOn(api, "providers").mockResolvedValue({ providers: [{ id: "claude", available: true, models: [] }] });
  const upload = vi.spyOn(api, "uploadAttachment").mockResolvedValue({ name: "notes.txt", path: "/session/attachments/notes.txt" });
  const start = vi.spyOn(api, "startRun").mockResolvedValue({ run: { id: "new" } as never, prompt: "" });
  const onStarted = vi.fn();
  render(<AskAgentDialog sessionKey="repo/change" runs={[]} onClose={() => {}} onStarted={onStarted} />);
  fireEvent.change(screen.getByLabelText("Your request"), { target: { value: "Review this" } });
  const file = new File(["context"], "notes.txt", { type: "text/plain" });
  fireEvent.change(document.querySelector('input[type="file"]')!, { target: { files: [file] } });
  await screen.findByText("Uploaded");
  fireEvent.click(screen.getByRole("button", { name: "Start and send" }));
  await waitFor(() => expect(onStarted).toHaveBeenCalledWith("new"));
  expect(upload).toHaveBeenCalledWith("repo/change", file);
  expect(start).toHaveBeenCalledWith("repo/change", expect.objectContaining({ provider: "claude", stage: "assist", prompt: "Review this\n\nAttached file: /session/attachments/notes.txt" }));
});

it("keeps the new session draft when an earlier request completes", async () => {
  vi.spyOn(api, "providers").mockResolvedValue({ providers: [{ id: "claude", available: true, models: [] }] });
  let resolve!: (value: { run: never; prompt: string }) => void;
  const pending = new Promise<{ run: never; prompt: string }>((yes) => { resolve = yes; });
  const start = vi.spyOn(api, "startRun").mockReturnValue(pending);
  const close = vi.fn();
  const started = vi.fn();
  const { rerender } = render(<AskAgentDialog sessionKey="repo/first" runs={[]} onClose={close} onStarted={started} />);
  fireEvent.change(screen.getByLabelText("Your request"), { target: { value: "Old request" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Start and send" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Start and send" }));
  rerender(<AskAgentDialog sessionKey="repo/second" runs={[]} onClose={close} onStarted={started} />);
  fireEvent.change(screen.getByLabelText("Your request"), { target: { value: "New draft" } });
  await act(async () => resolve({ run: { id: "old" } as never, prompt: "" }));
  expect(screen.getByLabelText("Your request")).toHaveValue("New draft");
  expect(close).not.toHaveBeenCalled();
  expect(started).not.toHaveBeenCalled();
  expect(start).toHaveBeenCalledWith("repo/first", expect.objectContaining({ prompt: "Old request" }));
});
