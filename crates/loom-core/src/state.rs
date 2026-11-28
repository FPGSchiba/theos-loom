//! Lifecycle states of epics and stories (umbrella §3.2).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Lifecycle state of an epic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EpicState {
    /// Accepted, not yet sliced into stories.
    Intake,
    /// Being sliced into stories.
    Slicing,
    /// Its stories are being worked.
    Active,
    /// Waiting on something outside the epic.
    Blocked,
    /// All stories merged.
    Done,
}

/// Lifecycle state of a story.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StoryState {
    /// Waiting for its turn.
    Queued,
    /// Being refined into a spec.
    Refining,
    /// Spec approved, ready to implement.
    Refined,
    /// Being implemented.
    Implementing,
    /// Implementation done, under review.
    InReview,
    /// Waiting on something outside the story.
    Blocked,
    /// Merged into the base branch.
    Merged,
}

#[cfg(test)]
mod tests {
    use super::{EpicState, StoryState};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn every_state_name_round_trips() -> TestResult {
        let epics = [
            (EpicState::Intake, "intake"),
            (EpicState::Slicing, "slicing"),
            (EpicState::Active, "active"),
            (EpicState::Blocked, "blocked"),
            (EpicState::Done, "done"),
        ];
        for (state, name) in epics {
            let json = serde_json::to_string(&state)?;
            assert_eq!(json, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<EpicState>(&json)?, state);
        }
        let stories = [
            (StoryState::Queued, "queued"),
            (StoryState::Refining, "refining"),
            (StoryState::Refined, "refined"),
            (StoryState::Implementing, "implementing"),
            (StoryState::InReview, "in-review"),
            (StoryState::Blocked, "blocked"),
            (StoryState::Merged, "merged"),
        ];
        for (state, name) in stories {
            let json = serde_json::to_string(&state)?;
            assert_eq!(json, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<StoryState>(&json)?, state);
        }
        Ok(())
    }
}
