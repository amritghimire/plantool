import { fireEvent, render, screen } from "@testing-library/react";
import { Handoff } from "./Handoff";
it("announces handoff changes and exposes one action", () => {
  const action = vi.fn();
  const { rerender } = render(<Handoff step="Build" label="Agent working" action="View agent run" onAction={action} />);
  expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
  rerender(<Handoff step="Build" label="Agent needs an answer" action="Answer agent" onAction={action} />);
  expect(screen.getByRole("status")).toHaveTextContent("Agent needs an answer");
  fireEvent.click(screen.getByRole("button", { name: "Answer agent" }));
  expect(action).toHaveBeenCalledOnce();
});
