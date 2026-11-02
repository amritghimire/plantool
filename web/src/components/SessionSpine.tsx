import { useEffect, useId, useState } from "react";
import { Link } from "react-router-dom";
import * as Popover from "@radix-ui/react-popover";
import {
  ArrowLeftIcon,
  BookOpenIcon,
  CaretDownIcon,
  GitBranchIcon,
} from "@phosphor-icons/react";
import { api } from "../api";
import type { Comment, DocResponse, Run, SessionView, Stage } from "../types";
import { useConfirmation } from "../lib/confirmation";
import { cn } from "../lib/cn";
import {
  STEPS,
  noticeFor,
  openBlockers,
  openThreads,
  planProgress,
  resumeStageFor,
  stepOf,
  type NoticeAction,
} from "../lib/notice";
import type { RunStage } from "./StartRunDialog";
import { CommitProgress } from "./CommitProgress";
import { DecisionSummary } from "./DecisionSummary";
import { changeTotals } from "./ChangesTab";
import { ThemeToggle } from "./ThemeToggle";
import { CopyButton } from "./CopyButton";
import { useShortcuts } from "../lib/shortcuts";

export interface RunRequest {
  resumeId?: string;
  initialMode?: "step-by-step";
  initialPrompt?: string;
}

export interface SessionSpineProps {
  view: SessionView;
  comments: Comment[];
  activeTab: string;
  planDoc: DocResponse | null;
  prompt?: string | null;
  busy: boolean;
  onStartRun: (stage: RunStage, request?: RunRequest) => void;
  onStage: (s: Stage, overrideReason?: string) => Promise<void>;
  onJump: (c: Comment) => void;
  onNavigate: (line: number) => void;
  onAskAgent: () => void;
  onPr: () => void;
  onOpenRun: (id: string) => void;
  onOpenChanges: () => void;
  onContinueMilestone: (run: Run) => void;
  onApproveMilestone: (run: Run, commit: boolean, message: string) => void;
  onCancelCommit: () => void;
  onDetails: () => void;
  onSettings: () => void;
  onTab?: (tab: string) => void;
  waiting?: number | null;
}

const FIG_TAB = ["research", "plan", "changes", "changes", "changes"];

const CONFIRMED_APPROVALS = new Set([
  "accept-revision",
  "approve-plan",
  "approve-override",
  "accept",
]);

const short = (sha: string | null | undefined) =>
  sha ? sha.slice(0, 7) : "unknown";

export function SessionSpine(p: SessionSpineProps) {
  const { confirm, confirmWithReason, dialog } = useConfirmation();
  const { view } = p;
  const s = view.session;
  const stage = view.state.stage;
  const planSha =
    p.planDoc?.sha ?? view.docs.find((d) => d.kind === "plan")?.sha ?? null;
  const notice = noticeFor(view, {
    comments: p.comments,
    activeTab: p.activeTab,
    planSha,
  });
  const open = openThreads(p.comments);
  const openHuman = open.filter((c) => c.kind === "human").length;
  const blockers = openBlockers(p.comments);
  const progress = planProgress(view);
  const step = stepOf(view);
  const idx = STEPS.indexOf(step);

  const [commitMilestone, setCommitMilestone] = useState(true);
  const [commitMessage, setCommitMessage] = useState("");
  const approvalRun =
    notice.primary?.id === "approve-milestone"
      ? (notice.primary.run ?? null)
      : null;
  const approvalCount = approvalRun?.milestones_approved ?? 0;
  useEffect(() => {
    if (!approvalRun) return;
    let cancelled = false;
    setCommitMessage("");
    api
      .milestone(view.key, approvalRun.id)
      .then((m) => {
        if (!cancelled) setCommitMessage(m.subject);
      })
      .catch(() => {
        if (!cancelled) setCommitMessage(`Milestone ${approvalCount + 1}`);
      });
    return () => {
      cancelled = true;
    };
  }, [view.key, approvalRun?.id, approvalCount, planSha]);

  const [milestoneDiff, setMilestoneDiff] = useState<{
    files: number;
    added: number;
    deleted: number;
    label: string;
  } | null>(null);
  useEffect(() => {
    if (!approvalRun) return;
    let cancelled = false;
    setMilestoneDiff(null);
    api
      .changes(view.key, "step")
      .then((r) => {
        if (!cancelled)
          setMilestoneDiff({
            files: r.stat.length,
            label: r.label,
            ...changeTotals(r),
          });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [view.key, approvalRun?.id, approvalCount]);

  const commentsField = {
    label: "Open comments",
    value: `${open.length} (${openHuman} yours)`,
  };
  const approve = async (override: boolean) => {
    const fields = [
      { label: "Pins", value: `plan.md rev ${short(planSha)}` },
      commentsField,
      {
        label: "Next",
        value: "The stage becomes approved. Start the build from the Notice.",
      },
    ];
    if (override) {
      const reason = await confirmWithReason(
        `${blockers.length} open ${blockers.length === 1 ? "blocker stops" : "blockers stop"} ordinary approval. Your reason is recorded with the approval.`,
        {
          title: "Approve the plan with an override",
          confirmLabel: "Approve with recorded override",
          fields,
          reasonLabel: "Why are you overriding the blockers?",
        },
      );
      if (reason) await p.onStage("approved", reason);
      return;
    }
    if (
      await confirm("You are allowing this revision of the plan.", false, {
        title: "Approve the plan",
        confirmLabel: "Approve plan",
        fields,
      })
    )
      await p.onStage("approved");
  };
  const accept = async () => {
    if (
      await confirm(
        "You are accepting the implementation as finished.",
        false,
        {
          title: "Accept the implementation",
          confirmLabel: "Accept and finish",
          fields: [
            {
              label: "Plan",
              value: `plan.md rev ${short(view.state.approved?.sha ?? planSha)}`,
            },
            ...(progress.total
              ? [
                  {
                    label: "Tasks",
                    value: `${progress.done} / ${progress.total} done`,
                  },
                ]
              : []),
            commentsField,
            { label: "Next", value: "The session moves to Done." },
          ],
        },
      )
    )
      await p.onStage("done");
  };
  const acceptRevision = async () => {
    if (
      await confirm(
        "You are accepting the revised plan for the rest of the build.",
        false,
        {
          title: "Accept the plan revision",
          confirmLabel: "Accept plan revision",
          fields: [
            { label: "Pins", value: `plan.md rev ${short(planSha)}` },
            {
              label: "Next",
              value: "The next milestone builds against this revision.",
            },
          ],
        },
      )
    )
      await p.onStage("approved");
  };
  const act = async (a: NoticeAction) => {
    switch (a.id) {
      case "accept-revision":
        return acceptRevision();
      case "start-research":
        return p.onStartRun("research");
      case "skip-to-plan":
      case "start-plan":
      case "revise-plan":
        return p.onStartRun("plan");
      case "skip-to-build":
        if (
          await confirm(
            "Skip planning? The stage becomes approved. The agent will write a short task list to plan.md from the brief, then implement phase by phase.",
          )
        )
          await p.onStage("approved");
        return;
      case "approve-plan":
        return approve(false);
      case "approve-override":
        return approve(true);
      case "start-build":
      case "continue-build":
        return p.onStartRun("implement");
      case "start-build-steps":
        return p.onStartRun("implement", { initialMode: "step-by-step" });
      case "approve-milestone":
        if (a.run) p.onApproveMilestone(a.run, commitMilestone, commitMessage);
        return;
      case "resume-milestone":
        if (a.run) p.onContinueMilestone(a.run);
        return;
      case "review-changes":
        return p.onOpenChanges();
      case "accept":
        return accept();
      case "finish-build":
        if (
          await confirm(
            "Finish the build without the agent? The session moves to final review, where you check the changes and accept. Unticked tasks stay unticked; you can still resume a run from Review.",
          )
        )
          await p.onStage("implementation-review");
        return;
      case "open-run":
        if (a.run) p.onOpenRun(a.run.id);
        return;
      case "resume-run":
        if (a.run) p.onStartRun(resumeStageFor(a.run), { resumeId: a.run.id });
        return;
      case "open-details":
        return p.onDetails();
      case "critique":
        return p.onStartRun(
          "critique",
          a.prompt ? { initialPrompt: a.prompt } : undefined,
        );
      case "ask":
        return p.onAskAgent();
      case "create-pr":
        return p.onPr();
    }
  };

  const figState = (i: number): string => {
    if (stage === "done") return "done";
    if (i < idx) {
      const kind = i === 0 ? "research" : i === 1 ? "plan" : null;
      const skipped = kind && !view.docs.find((d) => d.kind === kind)?.exists;
      return skipped ? "skipped" : "done";
    }
    if (STEPS[i] === "Build" && progress.total)
      return `${progress.done} / ${progress.total} tasks`;
    return i === idx ? "current" : "pending";
  };
  const isCurrent = (i: number) =>
    stage === "done" ? i === STEPS.length - 1 : i === idx;
  const currentFig = stage === "done" ? STEPS.length - 1 : idx;
  const activeFig =
    FIG_TAB[currentFig] === p.activeTab
      ? currentFig
      : FIG_TAB.indexOf(p.activeTab);

  const primary = notice.primary;
  const whyId = useId();
  const primaryWhy =
    primary?.disabled ??
    (primary?.id === "approve-milestone" &&
    commitMilestone &&
    commitMessage.trim() === ""
      ? "Write a commit subject, or untick commit"
      : p.busy
        ? "Working…"
        : undefined);
  const primaryDisabled = !!primaryWhy;
  useShortcuts({
    a: () => {
      if (primary && !primaryDisabled && CONFIRMED_APPROVALS.has(primary.id))
        void act(primary);
    },
  });
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
  const showSummary = notice.form === "Notice of allowance" && !!p.planDoc;
  const hasDetail =
    notice.evidence.length > 0 ||
    !!(approvalRun && milestoneDiff) ||
    showSummary ||
    !!notice.guide?.length ||
    !!notice.note;
  const menuPlain = notice.secondary.filter((a) => !a.prompt);
  const menuPresets = notice.secondary.filter((a) => a.prompt);

  return (
    <aside className="spine" aria-label="Session">
      <div className="spine-top">
        <Link to="/" className="wordmark">
          plantool
        </Link>
        <Link to="/" className="spine-back">
          <ArrowLeftIcon size={14} aria-hidden="true" /> Inbox
          {!!p.waiting && <span className="spine-waiting"> · {p.waiting} waiting on you</span>}
        </Link>
      </div>
      <header className="spine-id">
        <p className="spine-title">{s.title}</p>
        <p className="spine-ref">{s.repo_slug}</p>
        <p className="spine-ref">
          <GitBranchIcon size={13} aria-hidden="true" />
          <span>{view.workspace_branch || s.repo.branch}</span>
        </p>
      </header>
      <ol className="figs" aria-label="Steps">
        {STEPS.map((name, i) => (
          <li
            key={name}
            className={cn(isCurrent(i) && "current", i === activeFig && "active")}
            aria-current={isCurrent(i) ? "step" : undefined}
          >
            <button
              type="button"
              className="fig"
              aria-pressed={i === activeFig}
              aria-label={`FIG. ${i + 1} ${name}, ${figState(i)}`}
              onClick={() => p.onTab?.(FIG_TAB[i])}
            >
              <span className="fig-n">FIG. {i + 1}</span>
              <span className="fig-name">{name}</span>
              <span className="fig-state">{figState(i)}</span>
            </button>
          </li>
        ))}
      </ol>
      <section
        className={cn("form-box notice", expanded && "expanded")}
        aria-labelledby="notice-form"
      >
        <header>
          <span id="notice-form">{notice.form}</span>
          <button
            className="notice-more"
            type="button"
            aria-expanded={expanded}
            aria-controls={detailsId}
            aria-label={expanded ? "Show less" : hasDetail ? "Show evidence and actions" : "Show actions"}
            onClick={() => setExpanded((v) => !v)}
          >
            <span className="notice-more-label">
              {expanded ? "Less" : hasDetail ? "Evidence" : "More"}
            </span>
            <CaretDownIcon size={12} aria-hidden="true" />
          </button>
        </header>
        <div className="form-box-body">
          <p className="notice-heading" role="status" aria-live="polite">
            {notice.heading}
          </p>
          <div className="notice-detail" id={detailsId}>
            {(notice.evidence.length > 0 || (approvalRun && milestoneDiff)) && (
              <dl className="notice-fields">
                {notice.evidence.map((f) => (
                  <div key={f.label}>
                    <dt>{f.label}</dt>
                    <dd>
                      {f.href ? (
                        <a href={f.href} target="_blank" rel="noreferrer">
                          {f.value}
                        </a>
                      ) : (
                        f.value
                      )}
                    </dd>
                  </div>
                ))}
                {approvalRun && milestoneDiff && (
                  <div title={milestoneDiff.label}>
                    <dt>Diff</dt>
                    <dd>
                      {milestoneDiff.files} file
                      {milestoneDiff.files === 1 ? "" : "s"}{" "}
                      <span className="added">+{milestoneDiff.added}</span>{" "}
                      <span className="deleted">−{milestoneDiff.deleted}</span>
                    </dd>
                  </div>
                )}
              </dl>
            )}
            {showSummary && p.planDoc && (
              <DecisionSummary doc={p.planDoc} onNavigate={p.onNavigate} />
            )}
            {notice.guide?.map((line) => (
              <p key={line} className="notice-guide">
                {line}
              </p>
            ))}
            {notice.note && <p className="notice-note">{notice.note}</p>}
          </div>
          {notice.blockedBy && (
            <div className="notice-blocker">
              <span>{notice.blockedBy.text}</span>
              <button
                className="link"
                type="button"
                onClick={() =>
                  notice.blockedBy && p.onJump(notice.blockedBy.comment)
                }
              >
                Review blocker
              </button>
            </div>
          )}
          {notice.milestone?.committing && view.commit && (
            <CommitProgress
              job={view.commit}
              label="Committing milestone…"
              onCancel={p.onCancelCommit}
            />
          )}
          {primary?.id === "approve-milestone" && (
            <>
              <label className="check-row small">
                <input
                  type="checkbox"
                  checked={commitMilestone}
                  onChange={(e) => setCommitMilestone(e.target.checked)}
                  disabled={p.busy}
                />
                commit this milestone
              </label>
              {commitMilestone && (
                <label className="commit-subject">
                  <span>Commit subject</span>
                  <input
                    className="commit-message"
                    value={commitMessage}
                    onChange={(e) => setCommitMessage(e.target.value)}
                    disabled={p.busy}
                    placeholder="Milestone 1: what changed"
                  />
                </label>
              )}
            </>
          )}
          <div className="notice-actions">
            {primary && (
              <button
                className="btn primary notice-primary"
                type="button"
                disabled={primaryDisabled}
                aria-describedby={primaryWhy ? whyId : undefined}
                onClick={() => void act(primary)}
              >
                {primary.id === "approve-milestone" && !commitMilestone
                  ? primary.label.replace(/^Commit/, "Approve")
                  : primary.label}
              </button>
            )}
            {primaryWhy && (
              <p className="notice-note notice-why" id={whyId}>
                {primaryWhy}
              </p>
            )}
            {notice.alternates.length > 0 && (
              <div className="notice-alternates">
                {notice.alternates.map((a) => (
                  <button
                    key={`${a.id}:${a.run?.id ?? ""}`}
                    className="link"
                    type="button"
                    disabled={p.busy}
                    onClick={() => void act(a)}
                  >
                    {a.label}
                  </button>
                ))}
              </div>
            )}
            <Popover.Root>
              <Popover.Trigger asChild>
                <button
                  className="btn notice-ask"
                  type="button"
                  disabled={p.busy}
                >
                  Ask agent
                  <CaretDownIcon size={14} aria-hidden="true" />
                </button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content
                  className="menu"
                  align="start"
                  sideOffset={4}
                  collisionPadding={12}
                  aria-label="Ask agent"
                >
                  {notice.copyPrompt && p.prompt && (
                    <CopyButton
                      text={p.prompt}
                      label="Copy the research prompt"
                      className="menu-item"
                    />
                  )}
                  {menuPlain.map((a) => (
                    <Popover.Close asChild key={`${a.id}:${a.label}`}>
                      <button
                        className="menu-item"
                        type="button"
                        onClick={() => void act(a)}
                      >
                        {a.label}
                      </button>
                    </Popover.Close>
                  ))}
                  {menuPresets.length > 0 && (
                    <div
                      className="menu-group"
                      role="group"
                      aria-label="Help me review this plan"
                    >
                      <span className="menu-label">
                        Help me review this plan
                      </span>
                      {menuPresets.map((a) => (
                        <Popover.Close asChild key={a.label}>
                          <button
                            className="menu-item"
                            type="button"
                            onClick={() => void act(a)}
                          >
                            {a.label}
                          </button>
                        </Popover.Close>
                      ))}
                    </div>
                  )}
                </Popover.Content>
              </Popover.Portal>
            </Popover.Root>
          </div>
        </div>
      </section>
      <footer className="spine-foot">
        <button className="link" type="button" onClick={p.onDetails}>
          Session details
        </button>
        <button className="link" type="button" onClick={p.onSettings}>
          Settings
        </button>
        <span className="spacer" />
        <a
          className="spine-icon"
          href="https://github.com/amritghimire/plantool#readme"
          target="_blank"
          rel="noreferrer"
          aria-label="Documentation"
          title="Documentation"
        >
          <BookOpenIcon size={16} aria-hidden="true" />
        </a>
        <ThemeToggle className="spine-icon" size={16} />
      </footer>
      {dialog}
    </aside>
  );
}
