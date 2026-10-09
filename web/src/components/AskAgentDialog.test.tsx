import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  fireEvent.click(screen.getByRole("button", { name: "Start and send" }));
  await waitFor(() => expect(onStarted).toHaveBeenCalledWith("new"));
  expect(upload).toHaveBeenCalledWith("repo/change", file);
  expect(start).toHaveBeenCalledWith("repo/change", expect.objectContaining({ provider: "claude", stage: "assist", prompt: "Review this\n\nAttached file: /session/attachments/notes.txt" }));
});
