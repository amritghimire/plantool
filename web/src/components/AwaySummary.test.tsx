import { fireEvent, render, screen } from "@testing-library/react";
import { AwaySummary } from "./AwaySummary";
import type { State } from "../types";
beforeEach(() => {
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => values.set(key, value) });
});
afterEach(() => vi.unstubAllGlobals());
const state: State = { stage: "new", comments: [], docs: {}, seq: 4, review: null, updated_at: "", activity: [{ seq: 4, at: "2026-10-09T12:00:00Z", type: "step", summary: "Plan ready" }] };
it("hides empty activity and marks unseen updates", () => {
  window.localStorage.setItem("plantool.seen:activity-a", "3");
  const { rerender } = render(<AwaySummary sessionKey="activity-a" state={state} />);
  expect(screen.getByText(/1 updates/)).toBeInTheDocument();
  fireEvent.click(screen.getByText("Mark updates seen"));
  expect(screen.queryByText(/Since you were away/)).toBeNull();
  rerender(<AwaySummary sessionKey="activity-a" state={{ ...state, seq: 5, activity: [...state.activity!, { seq: 5, at: "2026-10-09T12:01:00Z", type: "step", summary: "Build ready" }] }} />);
  expect(screen.getByText(/1 updates/)).toBeInTheDocument();
});
it("reads the new session's seen marker", () => {
  window.localStorage.setItem("plantool.seen:activity-b", "1");
  window.localStorage.setItem("plantool.seen:activity-c", "4");
  const { rerender } = render(<AwaySummary sessionKey="activity-b" state={state} />);
  expect(screen.getByText(/1 updates/)).toBeInTheDocument();
  rerender(<AwaySummary sessionKey="activity-c" state={state} />);
  expect(screen.queryByText(/Since you were away/)).toBeNull();
});
