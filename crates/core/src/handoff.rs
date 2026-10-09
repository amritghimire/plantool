use crate::{Run, RunStatus, Stage, State};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Handoff {
    pub label: &'static str,
    pub action: &'static str,
    pub step: &'static str,
}

pub fn handoff(state: &State, runs: &[Run]) -> Handoff {
    let step = match state.stage {
        Stage::New | Stage::Researching | Stage::ResearchReview => "Research",
        Stage::Planning | Stage::PlanReview => "Plan",
        Stage::Approved | Stage::Implementing => "Build",
        Stage::ImplementationReview => "Review",
        Stage::Done => "Done",
    };
    if state.pause_reason.is_some() {
        return Handoff {
            label: "Agent needs an answer",
            action: "Review checkpoint",
            step,
        };
    }
    let latest = runs.iter().max_by(|a, b| a.started_at.cmp(&b.started_at));
    if latest.is_some_and(|r| {
        r.milestone_pending && matches!(r.status, RunStatus::Idle | RunStatus::Stopped)
    }) {
        return Handoff {
            label: "Owner reviewing",
            action: "Review milestone",
            step,
        };
    }
    let (label, action) = match latest.map(|r| r.status) {
        Some(RunStatus::Waiting) => ("Agent needs an answer", "Answer agent"),
        Some(RunStatus::Running | RunStatus::Starting) => ("Agent working", "View agent run"),
        Some(RunStatus::Failed) if state.stage != Stage::Done => {
            ("Recovery needed", "Resume agent run")
        }
        Some(RunStatus::Stopped)
            if matches!(
                state.stage,
                Stage::Researching | Stage::Planning | Stage::Implementing
            ) =>
        {
            ("Recovery needed", "Resume agent run")
        }
        _ => match state.stage {
            Stage::PlanReview => ("Owner approving", "Review plan"),
            Stage::ResearchReview | Stage::ImplementationReview => {
                ("Owner reviewing", "Review comments")
            }
            Stage::Done => ("Review complete", "View result"),
            _ => ("Owner choosing next step", "Start agent run"),
        },
    };
    Handoff {
        label,
        action,
        step,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_handoffs() {
        let mut state = State {
            stage: Stage::PlanReview,
            ..State::default()
        };
        assert_eq!(handoff(&state, &[]).label, "Owner approving");
        assert_eq!(handoff(&state, &[]).step, "Plan");
        state.stage = Stage::ImplementationReview;
        assert_eq!(handoff(&state, &[]).label, "Owner reviewing");
    }
    #[test]
    fn agent_and_recovery_handoffs() {
        let mut state = State {
            stage: Stage::Implementing,
            ..State::default()
        };
        let mut run: Run =
            serde_json::from_str(include_str!("../tests/fixtures/v0.0.14/run.json")).unwrap();
        for (status, label) in [
            (RunStatus::Running, "Agent working"),
            (RunStatus::Starting, "Agent working"),
            (RunStatus::Waiting, "Agent needs an answer"),
            (RunStatus::Failed, "Recovery needed"),
            (RunStatus::Stopped, "Recovery needed"),
        ] {
            run.status = status;
            assert_eq!(handoff(&state, &[run.clone()]).label, label);
        }
        run.status = RunStatus::Stopped;
        run.milestone_pending = true;
        assert_eq!(handoff(&state, &[run]).action, "Review milestone");
        state.pause_reason = Some("Public API change".into());
        assert_eq!(handoff(&state, &[]).action, "Review checkpoint");
        state.pause_reason = None;
        state.stage = Stage::Done;
        assert_eq!(handoff(&state, &[]).step, "Done");
    }
}
