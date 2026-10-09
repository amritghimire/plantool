use crate::model::Stage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Actor {
    Human,
    Agent,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StageError {
    #[error("only a human can set the stage to {0}")]
    HumanOnly(Stage),
    #[error("an agent cannot move the stage backwards ({from} -> {to})")]
    Backwards { from: Stage, to: Stage },
    #[error(
        "an agent cannot move the stage from {from} to {to}; a human approval is needed first"
    )]
    NeedsApproval { from: Stage, to: Stage },
}

pub fn transition(from: Stage, to: Stage, actor: Actor) -> Result<(), StageError> {
    if actor == Actor::Human {
        return Ok(());
    }
    if matches!(to, Stage::Approved | Stage::Done) {
        return Err(StageError::HumanOnly(to));
    }
    if to.index() < from.index() {
        return Err(StageError::Backwards { from, to });
    }
    let crosses_approval =
        from.index() < Stage::Approved.index() && to.index() > Stage::Approved.index();
    if crosses_approval {
        return Err(StageError::NeedsApproval { from, to });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_can_do_anything() {
        for from in Stage::ALL {
            for to in Stage::ALL {
                assert_eq!(transition(from, to, Actor::Human), Ok(()));
            }
        }
    }

    #[test]
    fn agent_steps_forward_before_approval() {
        assert_eq!(
            transition(Stage::New, Stage::Researching, Actor::Agent),
            Ok(())
        );
        assert_eq!(
            transition(Stage::Researching, Stage::ResearchReview, Actor::Agent),
            Ok(())
        );
        assert_eq!(
            transition(Stage::ResearchReview, Stage::PlanReview, Actor::Agent),
            Ok(())
        );
        assert_eq!(
            transition(Stage::PlanReview, Stage::PlanReview, Actor::Agent),
            Ok(())
        );
    }

    #[test]
    fn agent_steps_forward_after_approval() {
        assert_eq!(
            transition(Stage::Approved, Stage::Implementing, Actor::Agent),
            Ok(())
        );
        assert_eq!(
            transition(
                Stage::Implementing,
                Stage::ImplementationReview,
                Actor::Agent
            ),
            Ok(())
        );
    }

    #[test]
    fn agent_cannot_approve_or_finish() {
        assert_eq!(
            transition(Stage::PlanReview, Stage::Approved, Actor::Agent),
            Err(StageError::HumanOnly(Stage::Approved))
        );
        assert_eq!(
            transition(Stage::ImplementationReview, Stage::Done, Actor::Agent),
            Err(StageError::HumanOnly(Stage::Done))
        );
    }

    #[test]
    fn agent_cannot_skip_approval() {
        assert_eq!(
            transition(Stage::PlanReview, Stage::Implementing, Actor::Agent),
            Err(StageError::NeedsApproval {
                from: Stage::PlanReview,
                to: Stage::Implementing
            })
        );
    }

    #[test]
    fn agent_cannot_go_backwards() {
        assert_eq!(
            transition(Stage::PlanReview, Stage::Researching, Actor::Agent),
            Err(StageError::Backwards {
                from: Stage::PlanReview,
                to: Stage::Researching
            })
        );
    }
}
