use serde::{Deserialize, Serialize};

use crate::{loop_contract::validation, prelude::Result};

/// Natural-language source intent that led to research or design work.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionSourceIntent {
	summary: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	user_utterance: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_issue_identifier: Option<String>,
}
#[allow(dead_code)]
impl DecisionSourceIntent {
	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn source_issue_identifier(&self) -> Option<&str> {
		self.source_issue_identifier.as_deref()
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("decision contract source_intent.summary", &self.summary)?;
		validation::validate_optional(
			"decision contract source_intent.user_utterance",
			self.user_utterance.as_deref(),
		)?;

		validation::validate_optional(
			"decision contract source_intent.source_issue_identifier",
			self.source_issue_identifier.as_deref(),
		)
	}
}
