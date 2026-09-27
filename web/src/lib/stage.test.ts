import { STAGES } from "../types";
import { tabForStage } from "./stage";

test("each stage maps to the tab that shows its work", () => {
  expect(STAGES.map((s) => [s, tabForStage(s)])).toEqual([
    ["new", "research"],
    ["researching", "research"],
    ["research-review", "research"],
    ["planning", "plan"],
    ["plan-review", "plan"],
    ["approved", "changes"],
    ["implementing", "changes"],
    ["implementation-review", "changes"],
    ["done", "changes"],
  ]);
});
