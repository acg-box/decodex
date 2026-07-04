use std::collections::{BTreeMap, BTreeSet};

use serde_json::{self, Value};

use crate::orchestrator::git_ops::RepoGateFailureDiagnostic;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepoGateTrackedRewriteDecision {
	files: Vec<String>,
	pub(super) owned: bool,
	pub(super) decision: &'static str,
	pub(super) reason: &'static str,
	source_error_class: Option<&'static str>,
	source_diagnostic: Option<RepoGateFailureDiagnostic>,
}
impl RepoGateTrackedRewriteDecision {
	pub(super) fn continue_to_commit_capable_phase(files: Vec<String>) -> Self {
		Self {
			files,
			owned: true,
			decision: "continue_to_commit_capable_phase",
			reason: "all rewritten files were already present in the pre-gate implementation diff and the repo gate passed",
			source_error_class: None,
			source_diagnostic: None,
		}
	}

	pub(super) fn require_attention(files: Vec<String>, owned: bool, reason: &'static str) -> Self {
		Self {
			files,
			owned,
			decision: "repo_gate_tracked_rewrites_left",
			reason,
			source_error_class: None,
			source_diagnostic: None,
		}
	}

	pub(super) fn scope_envelope_violation(
		files: Vec<String>,
		source_error_class: Option<&'static str>,
		source_diagnostic: Option<RepoGateFailureDiagnostic>,
	) -> Self {
		Self {
			files,
			owned: false,
			decision: "scope_envelope_violation",
			reason: "one or more repo-gate rewrites were not present in the pre-gate lane diff",
			source_error_class,
			source_diagnostic,
		}
	}

	pub(crate) fn files_display(&self) -> String {
		if self.files.is_empty() {
			String::from("(no tracked files reported)")
		} else {
			self.files.join(", ")
		}
	}

	pub(crate) fn to_json(&self) -> Value {
		serde_json::json!({
			"files": &self.files,
			"owned": self.owned,
			"decision": self.decision,
			"reason": self.reason,
			"sourceErrorClass": self.source_error_class,
			"sourceRepoGateFailure": self.source_diagnostic.as_ref().map(RepoGateFailureDiagnostic::to_json),
		})
	}

	pub(crate) fn is_scope_envelope_violation(&self) -> bool {
		self.decision == "scope_envelope_violation"
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RepoGateCommandOutcome {
	tracked_rewrite_decision: Option<RepoGateTrackedRewriteDecision>,
}
impl RepoGateCommandOutcome {
	pub(super) fn clean() -> Self {
		Self::default()
	}

	pub(super) fn with_tracked_rewrite_decision(decision: RepoGateTrackedRewriteDecision) -> Self {
		Self { tracked_rewrite_decision: Some(decision) }
	}

	pub(crate) fn tracked_rewrite_decision(&self) -> Option<&RepoGateTrackedRewriteDecision> {
		self.tracked_rewrite_decision.as_ref()
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RepoGateTrackedDiffSnapshot {
	pub(super) full_diff: String,
	pub(super) path_diffs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RepoGateScopeEnvelope {
	authorized_paths: BTreeSet<String>,
}
impl RepoGateScopeEnvelope {
	pub(super) fn from_pre_gate_diff(snapshot: &RepoGateTrackedDiffSnapshot) -> Self {
		Self { authorized_paths: snapshot.path_diffs.keys().cloned().collect() }
	}

	pub(super) fn violation_files(
		&self,
		rewritten_files: impl IntoIterator<Item = String>,
	) -> Vec<String> {
		rewritten_files.into_iter().filter(|path| !self.authorized_paths.contains(path)).collect()
	}
}
