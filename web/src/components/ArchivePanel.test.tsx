import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi, it, expect } from "vitest";
import { authenticatedFetch } from "../auth";
import { ArchivePanel } from "./ArchivePanel";
vi.mock("../auth", () => ({ authenticatedFetch: vi.fn() }));
it.each([false, true])("imports with explicit restore-approvals choice: %s", async (restore) => {
  vi.mocked(authenticatedFetch).mockReset().mockResolvedValue({ ok: false, text: async () => "test response" } as Response);
  render(<ArchivePanel sessionKey="repo/item" repo="/repo" />);
  if (restore) fireEvent.click(screen.getByLabelText("Restore recorded approvals with passkey confirmation"));
  const file = new File(["zip"], "session.zip", { type: "application/zip" });
  fireEvent.change(screen.getByLabelText("Import zip"), { target: { files: [file] } });
  await waitFor(() => expect(authenticatedFetch).toHaveBeenCalledOnce());
  const [url, init] = vi.mocked(authenticatedFetch).mock.calls[0];
  expect(new URL(url, "http://localhost").searchParams.get("confirm")).toBe(String(restore));
  expect(init?.method).toBe("POST");
  expect(init?.body).toBe(file);
  expect(await screen.findByRole("alert")).toHaveTextContent("test response");
});
