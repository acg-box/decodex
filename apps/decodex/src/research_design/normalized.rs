mod generated_id;
mod input;
mod text;
mod validation;

pub(super) use self::{
	input::NormalizedResearchDesignInput,
	text::{normalize_optional_text, normalize_required_text, normalize_text_list},
};

use crate::research_design::ResearchDesignOutcome;

pub(super) fn default_feedback(outcome: ResearchDesignOutcome) -> &'static str {
	match outcome {
		ResearchDesignOutcome::DecisionReady => {
			"Decision-ready research/design output is stored as a latent contract until promotion."
		},
		ResearchDesignOutcome::NotDecisionReady => {
			"Research/design output is not decision-ready and must not become implementation work."
		},
		ResearchDesignOutcome::Blocked => {
			"Research/design output is blocked; resolve blockers before promotion."
		},
		ResearchDesignOutcome::NeedsHumanDecision => {
			"Research/design output needs an explicit human decision before execution authority exists."
		},
	}
}
