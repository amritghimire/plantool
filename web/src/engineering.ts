export type EngineeringProject = {
  id: string;
  name: string | null;
  checkout: string;
  visibility: string | null;
  app_id: number | null;
  installation_id: number | null;
  branch: string | null;
  synced_at: string | null;
  error: string | null;
  scheduler: string | null;
  preflight?: { warnings: string[]; advisories?: string[] } | null;
  roles?: { role: string; ready: boolean }[];
};
export type EngineeringItem = {
  id: string;
  project: string;
  title: string;
  issue: number | null;
  state: string;
  priority: string | null;
  pr: number | null;
  error: string | null;
  frozen: boolean;
  step: string;
  waiting_gates: number;
};
export type EngineeringAttempt = {
  id: string;
  role: string;
  generation: number;
  state: string;
  route: string;
  started_at: string;
  ended_at: string | null;
  summary: string | null;
  provider: string | null;
  model: string | null;
  tokens_in: number | null;
  tokens_out: number | null;
  duration_ms: number | null;
};
export type EngineeringOverview = {
  roles?: { role: string; ready: boolean }[];
  projects: EngineeringProject[];
  items: EngineeringItem[];
  active: {
    id: string;
    item: string;
    project: string;
    title: string;
    role: string;
    state: string;
    route: string;
    started_at: string;
    provider: string | null;
    model: string | null;
  }[];
};
export type EngineeringDetail = {
  scan_snapshot?: string|null;
  scan_sources?: {name:string;available:boolean;http_status:number|null;reason:string;next_step:string;truncated:boolean}[];
  notification_delivery?: {pending:number};
  item: EngineeringItem;
  repository: string | null;
  base: string;
  head: string;
  attempts: EngineeringAttempt[];
  gates: {
    id: string;
    kind: string;
    state: string;
    hash: string;
    sha: string | null;
    comment: number | null;
    at: string;
  }[];
  events: { id: number; at: string; action: string; step: string | null }[];
  history_limit: number;
};
export const stepLabel = (step: string) =>
  ({
    closed: "Closed on GitHub",
    owner_question: "Waiting for your answer in the item thread",
    project_wait: "Waiting for project events",
    await_sprint_gate: "Waiting for sprint approval",
    plan_gate: "Waiting for plan approval",
    research_gate: "Waiting for research approval",
    milestone: "Waiting for milestone review",
    pr: "PR review and merge",
    declined_wait: "Declined",
    loop_complete: "Complete",
    research: "CTO research",
    plan: "PM planning",
    plan_review: "CTO plan review",
    scan_devops: "DevOps scan",
    scan_qa: "QA scan",
    scan_pm: "PM finding routes",
    triage: "PM triage",
    discussion: "Team discussion",
    escalated: "Owner attention required",
    backlog: "Backlog",
  })[step] ?? step.replaceAll("_", " ");
