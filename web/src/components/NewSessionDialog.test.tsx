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
