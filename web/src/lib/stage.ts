import type { Stage } from "../types";

export function tabForStage(stage: Stage): "research" | "plan" | "changes" {
  switch (stage) {
    case "new":
    case "researching":
    case "research-review":
      return "research";
    case "planning":
    case "plan-review":
      return "plan";
    default:
      return "changes";
  }
}
