import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../api";
import { AttachmentComposer, useAttachmentDraft } from "./AttachmentComposer";

function Harness({ session = "repo/x", run = "r", onSend = async () => {} }: { session?: string; run?: string; onSend?: (session: string, run: string, text: string) => Promise<void> }) {
  const composer = useAttachmentDraft(session, run);
  return <><AttachmentComposer composer={composer} label="Message" placeholder="Message" onSend={() => void composer.send((text) => onSend(session, run, text))} /><button disabled={!composer.canSend} onClick={() => void composer.send((text) => onSend(session, run, text))}>Send</button></>;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const image = () => new File(["png data"], "image.png", { type: "image/png" });
const pick = (files: File[]) => fireEvent.change(screen.getByLabelText("Attach files"), { target: { files } });
beforeEach(() => {
  vi.spyOn(api, "uploadAttachment").mockImplementation(async (_key, file) => ({ name: file.name, path: `/session/attachments/${file.name}` }));
  URL.createObjectURL = vi.fn(() => "blob:preview");
  URL.revokeObjectURL = vi.fn();
});
afterEach(() => { vi.restoreAllMocks(); sessionStorage.clear(); });

it("accepts multiple picker files and sends attachment-only messages", async () => {
  const send = vi.fn(async () => {});
  render(<Harness run="picker" onSend={send} />);
  pick([image(), new File(["notes"], "notes.txt")]);
  expect(screen.getByAltText("Preview of image.png")).toBeInTheDocument();
  await waitFor(() => expect(screen.getAllByText("Uploaded")).toHaveLength(2));
  expect(screen.getByText("8 bytes")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Send"));
  await waitFor(() => expect(send).toHaveBeenCalledWith("repo/x", "picker", "Attached file: /session/attachments/image.png\n\nAttached file: /session/attachments/notes.txt"));
  await waitFor(() => expect(screen.queryByLabelText("Attachments")).toBeNull());
  expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:preview");
});

it("preserves ordinary paste, uploads clipboard images and accepts drop", async () => {
  render(<Harness run="paste" />);
  const box = screen.getByLabelText("Message");
  expect(fireEvent.paste(box, { clipboardData: { files: [], getData: () => "ordinary text" } })).toBe(true);
  expect(api.uploadAttachment).not.toHaveBeenCalled();
  expect(fireEvent.paste(box, { clipboardData: { files: [image()], getData: () => "" } })).toBe(false);
  fireEvent.drop(box.closest(".attachment-composer")!, { dataTransfer: { files: [new File(["drop"], "dropped.txt")], types: ["Files"] } });
  await waitFor(() => expect(screen.getAllByText("Uploaded")).toHaveLength(2));
});

it("retries failed uploads and reuses successful uploads after a send failure", async () => {
  vi.mocked(api.uploadAttachment).mockRejectedValueOnce(new Error("Upload interrupted"));
  const send = vi.fn().mockRejectedValueOnce(new Error("Send interrupted")).mockResolvedValue(undefined);
  render(<Harness run="retry" onSend={send} />);
  pick([image()]);
  expect(await screen.findByRole("alert")).toHaveTextContent("Upload interrupted");
  expect(screen.getByText("Send")).toBeDisabled();
  fireEvent.click(screen.getByText("Retry image.png"));
  await screen.findByText("Uploaded");
  fireEvent.click(screen.getByText("Send"));
  expect(await screen.findByRole("alert")).toHaveTextContent("Send interrupted");
  expect(screen.getByAltText("Preview of image.png")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Send"));
  await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
  expect(api.uploadAttachment).toHaveBeenCalledTimes(2);
});

it("rejects empty or oversized files before upload and allows their removal", () => {
  render(<Harness run="limits" />);
  pick([new File([], "empty"), new File([new Uint8Array(10 * 1024 * 1024 + 1)], "large")]);
  expect(screen.getAllByRole("alert")).toHaveLength(2);
  expect(api.uploadAttachment).not.toHaveBeenCalled();
  expect(screen.getByText("Send")).toBeDisabled();
  fireEvent.click(screen.getByLabelText("Remove empty"));
  fireEvent.click(screen.getByLabelText("Remove large"));
  expect(screen.queryByLabelText("Attachments")).toBeNull();
});

it("isolates pending uploads and text when the session and run change", async () => {
  const upload = deferred<{ name: string; path: string }>();
  vi.mocked(api.uploadAttachment).mockReturnValueOnce(upload.promise);
  const send = vi.fn(async () => {});
  const { rerender } = render(<Harness session="repo/a" run="same-id" onSend={send} />);
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "First draft" } });
  pick([image()]);
  rerender(<Harness session="repo/b" run="same-id" onSend={send} />);
  expect(screen.getByLabelText("Message")).toHaveValue("");
  expect(screen.queryByLabelText("Attachments")).toBeNull();
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "Second draft" } });
  await act(async () => upload.resolve({ name: "image.png", path: "/a/attachments/image.png" }));
  expect(screen.getByLabelText("Message")).toHaveValue("Second draft");
  expect(screen.queryByLabelText("Attachments")).toBeNull();
  rerender(<Harness session="repo/a" run="same-id" onSend={send} />);
  expect(screen.getByLabelText("Message")).toHaveValue("First draft");
  fireEvent.click(screen.getByText("Send"));
  await waitFor(() => expect(send).toHaveBeenCalledWith("repo/a", "same-id", "First draft\n\nAttached file: /a/attachments/image.png"));
});

it("does not clear a new run's draft when an old send completes", async () => {
  const sending = deferred<void>();
  const send = vi.fn(() => sending.promise);
  const { rerender } = render(<Harness run="old" onSend={send} />);
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "Old message" } });
  fireEvent.click(screen.getByText("Send"));
  rerender(<Harness run="new" onSend={send} />);
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "New draft" } });
  await act(async () => sending.resolve());
  expect(screen.getByLabelText("Message")).toHaveValue("New draft");
  expect(send).toHaveBeenCalledWith("repo/x", "old", "Old message");
});

it("clears only the sent snapshot and keeps edits and new files made during sending", async () => {
  const sending = deferred<void>();
  render(<Harness run="snapshot" onSend={() => sending.promise} />);
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "Sent message" } });
  pick([new File(["a"], "first.txt")]);
  await screen.findByText("Uploaded");
  fireEvent.click(screen.getByText("Send"));
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "Later message" } });
  pick([new File(["b"], "later.txt")]);
  await waitFor(() => expect(screen.getAllByText("Uploaded")).toHaveLength(2));
  await act(async () => sending.resolve());
  expect(screen.getByLabelText("Message")).toHaveValue("Later message");
  expect(screen.queryByText("first.txt")).toBeNull();
  expect(screen.getByText("later.txt")).toBeInTheDocument();
});

it("does not restore a removed pending file and revokes previews on unmount", async () => {
  const upload = deferred<{ name: string; path: string }>();
  vi.mocked(api.uploadAttachment).mockReturnValueOnce(upload.promise);
  const { unmount } = render(<Harness run="remove" />);
  pick([image()]);
  fireEvent.click(screen.getByLabelText("Remove image.png"));
  await act(async () => upload.resolve({ name: "image.png", path: "/session/attachments/image.png" }));
  expect(screen.queryByLabelText("Attachments")).toBeNull();
  pick([image()]);
  await screen.findByText("Uploaded");
  unmount();
  expect(URL.revokeObjectURL).toHaveBeenCalledTimes(2);
});
