use std::collections::{BTreeMap, BTreeSet};

use serde_json::{self, Value};
use sha2::{Digest as _, Sha256};

use crate::orchestrator::git_ops::RepoGateFailureDiagnostic;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepoGateTrackedRewriteDecision {
	files: Vec<String>,
	pub(super) owned: bool,
	pub(super) classification: &'static str,
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
			classification: "lane_owned",
			decision: "continue_to_commit_capable_phase",
			reason: "all rewritten files were already present in the pre-gate implementation diff and the repo gate passed",
			source_error_class: None,
			source_diagnostic: None,
		}
	}

	pub(super) fn lane_owned_requires_clean_boundary(files: Vec<String>) -> Self {
		Self {
			files,
			owned: true,
			classification: "lane_owned",
			decision: "repo_gate_tracked_rewrites_left",
			reason: "all rewritten files were pre-gate implementation paths, but this lifecycle boundary requires a clean committed worktree",
			source_error_class: None,
			source_diagnostic: None,
		}
	}

	pub(super) fn lane_external_tracked_rewrite(files: Vec<String>) -> Self {
		Self {
			files,
			owned: false,
			classification: "lane_external_tracked_rewrite",
			decision: "require_scoped_authority",
			reason: "repo gate rewrote tracked files outside the lane diff after passing; stop automatic issue-local repair until scoped authority is explicit",
			source_error_class: None,
			source_diagnostic: None,
		}
	}

	pub(super) fn ambiguous_scope(
		files: Vec<String>,
		source_error_class: Option<&'static str>,
		source_diagnostic: Option<RepoGateFailureDiagnostic>,
	) -> Self {
		Self {
			files,
			owned: false,
			classification: "ambiguous_scope",
			decision: "manual_authority_boundary",
			reason: "repo gate failed after writing files outside the lane diff; Decodex cannot infer generated-artifact or scope semantics",
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
			"fileCount": self.files.len(),
			"sample": self.files.iter().take(12).collect::<Vec<_>>(),
			"rewriteSetHash": self.rewrite_set_hash(),
			"owned": self.owned,
			"classification": self.classification,
			"decision": self.decision,
			"reason": self.reason,
			"sourceErrorClass": self.source_error_class,
			"sourceRepoGateFailure": self.source_diagnostic.as_ref().map(RepoGateFailureDiagnostic::to_json),
		})
	}

	pub(crate) fn is_scope_envelope_violation(&self) -> bool {
		self.classification == "ambiguous_scope"
	}

	pub(crate) fn is_lane_external_tracked_rewrite(&self) -> bool {
		self.classification == "lane_external_tracked_rewrite"
	}

	fn rewrite_set_hash(&self) -> String {
		let mut hasher = Sha256::new();

		for file in &self.files {
			hasher.update(file.as_bytes());
			hasher.update(b"\n");
		}

		hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
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
