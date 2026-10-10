import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Composer } from "./Composer";

it("keeps typed feedback and announces failed submission", async () => {
  const onSubmit = vi.fn().mockRejectedValueOnce(new Error("Unable to save the comment at /a/long/path"));
  render(<Composer placeholder="Feedback" showType onSubmit={onSubmit} />);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Explain this decision" } });
  fireEvent.change(screen.getByLabelText("Feedback type"), { target: { value: "blocker" } });
  fireEvent.click(screen.getByRole("button", { name: "Comment" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Unable to save");
  expect(screen.getByRole("textbox")).toHaveValue("Explain this decision");
  expect(onSubmit).toHaveBeenCalledWith("Explain this decision", "blocker");
  onSubmit.mockResolvedValue(undefined);
  fireEvent.click(screen.getByRole("button", { name: "Comment" }));
  await waitFor(() => expect(screen.getByRole("textbox")).toHaveValue(""));
});
