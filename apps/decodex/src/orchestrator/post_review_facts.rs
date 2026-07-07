use std::{path::Path, process::Command};

use crate::{
	config::ReviewLevel,
	orchestrator::PullRequestReviewState,
	prelude::{Result, eyre},
	state::{self, ReviewCheckpointArtifactLookup, ReviewLifecycleRecord, StateStore},
};

const REVIEW_CHECKPOINT_PHASE_PRIORITY: [&str; 2] = ["repair", "handoff"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeReviewGateState {
	NotRequired,
	WorktreeHeadMissing,
	Pending,
	Clean,
	Findings,
	NeedsArchitectureReview,
	Blocked,
	Unknown(String),
}
impl RuntimeReviewGateState {
	pub(crate) fn as_str(&self) -> &str {
		match self {
			Self::NotRequired => "not_required",
			Self::WorktreeHeadMissing => "worktree_head_missing",
			Self::Pending => "pending",
			Self::Clean => "clean",
			Self::Findings => "findings",
			Self::NeedsArchitectureReview => "needs_architecture_review",
			Self::Blocked => "blocked",
			Self::Unknown(status) => status.as_str(),
		}
	}

	pub(crate) fn from_checkpoint(
		review_level: ReviewLevel,
		validated_head_sha: Option<&str>,
		checkpoint_status: Option<&str>,
	) -> Self {
		if !review_level.requires_review_checkpoint() {
			return Self::NotRequired;
		}
		if validated_head_sha.is_none_or(str::is_empty) {
			return Self::WorktreeHeadMissing;
		}
		match checkpoint_status {
			None => Self::Pending,
			Some("clean") => Self::Clean,
			Some("findings") => Self::Findings,
			Some("needs_architecture_review") => Self::NeedsArchitectureReview,
			Some("blocked") => Self::Blocked,
			Some(status) => Self::Unknown(status.to_owned()),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PostReviewLifecycleFacts {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) pr_url: String,
	pub(crate) base_branch: Option<String>,
	pub(crate) head_branch: String,
	pub(crate) validated_head_sha: String,
	pub(crate) worktree_path: String,
	pub(crate) review_level: String,
	pub(crate) review_gate_state: RuntimeReviewGateState,
	pub(crate) phase: String,
	pub(crate) landing_state: String,
	pub(crate) closeout_state: String,
	pub(crate) source_evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeReviewCheckpointStatus {
	pub(crate) phase: &'static str,
	pub(crate) status: String,
	pub(crate) updated_at: String,
	pub(crate) updated_at_unix: i64,
}

pub(crate) struct PostReviewLifecycleFactsInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) review_lifecycle: Option<&'a ReviewLifecycleRecord>,
	pub(crate) review_state: &'a PullRequestReviewState,
	pub(crate) worktree_path: &'a Path,
	pub(crate) review_level: ReviewLevel,
	pub(crate) phase: &'a str,
	pub(crate) landing_state: Option<&'a str>,
	pub(crate) closeout_state: Option<&'a str>,
	pub(crate) validated_head_sha: Option<&'a str>,
	pub(crate) review_checkpoint_phase: Option<&'a str>,
	pub(crate) review_checkpoint_status: Option<&'a str>,
}

pub(crate) fn build_post_review_lifecycle_facts(
	input: PostReviewLifecycleFactsInput<'_>,
) -> PostReviewLifecycleFacts {
	let validated_head_sha =
		input.validated_head_sha.unwrap_or(input.review_state.head_ref_oid.as_str()).to_owned();
	let mut source_evidence_refs =
		vec![format!("pr_readback:{}:{}", input.review_state.url, input.review_state.head_ref_oid)];
	if let Some(lifecycle) = input.review_lifecycle {
		source_evidence_refs.push(format!(
			"review_lifecycle:{}:{}:{}",
			lifecycle.run_id(),
			lifecycle.attempt_number(),
			lifecycle.pr_head_oid()
		));
	}
	if input.review_checkpoint_status.is_some() {
		source_evidence_refs.push(format!(
			"review_checkpoint:{}:{}:{}",
			input.review_level.as_str(),
			input.review_checkpoint_phase.unwrap_or("unknown"),
			validated_head_sha
		));
	}

	PostReviewLifecycleFacts {
		project_id: input.project_id.to_owned(),
		issue_id: input.issue_id.to_owned(),
		pr_url: input.review_state.url.clone(),
		base_branch: input
			.review_lifecycle
			.and_then(ReviewLifecycleRecord::target_base_ref_name)
			.map(str::to_owned),
		head_branch: input.review_state.head_ref_name.clone(),
		validated_head_sha: validated_head_sha.clone(),
		worktree_path: input.worktree_path.display().to_string(),
		review_level: input.review_level.as_str().to_owned(),
		review_gate_state: RuntimeReviewGateState::from_checkpoint(
			input.review_level,
			input.validated_head_sha,
			input.review_checkpoint_status,
		),
		phase: input.phase.to_owned(),
		landing_state: input.landing_state.unwrap_or("not_started").to_owned(),
		closeout_state: input.closeout_state.unwrap_or("not_started").to_owned(),
		source_evidence_refs,
	}
}

pub(crate) fn runtime_review_checkpoint_status_for_head(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	review_level: ReviewLevel,
	head_sha: &str,
) -> Result<Option<RuntimeReviewCheckpointStatus>> {
	latest_runtime_review_checkpoint_status(
		REVIEW_CHECKPOINT_PHASE_PRIORITY
			.into_iter()
			.map(|phase| {
				runtime_review_checkpoint_status_for_head_phase(
					state_store,
					project_id,
					issue_id,
					review_level,
					head_sha,
					phase,
				)
			})
			.collect::<Result<Vec<_>>>()?,
	)
}

pub(crate) fn runtime_review_checkpoint_status_for_head_phase(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	review_level: ReviewLevel,
	head_sha: &str,
	phase: &'static str,
) -> Result<Option<RuntimeReviewCheckpointStatus>> {
	Ok(state_store
		.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
			project_id,
			issue_id,
			phase,
			review_level: review_level.as_str(),
			head_sha,
		})?
		.map(|checkpoint| RuntimeReviewCheckpointStatus {
			phase,
			status: checkpoint.status().to_owned(),
			updated_at: checkpoint.updated_at().to_owned(),
			updated_at_unix: checkpoint.updated_at_unix(),
		}))
}

pub(crate) fn latest_runtime_review_checkpoint_status(
	checkpoints: Vec<Option<RuntimeReviewCheckpointStatus>>,
) -> Result<Option<RuntimeReviewCheckpointStatus>> {
	Ok(checkpoints.into_iter().flatten().max_by(|left, right| {
		(left.updated_at_unix, left.updated_at.as_str(), left.phase == "handoff").cmp(&(
			right.updated_at_unix,
			right.updated_at.as_str(),
			right.phase == "handoff",
		))
	}))
}

pub(crate) fn worktree_has_review_blocking_changes(worktree_path: &Path) -> Result<bool> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["status", "--porcelain=v1", "--untracked-files=all"])
		.output()?;
	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		eyre::bail!(
			"Failed to inspect review-blocking worktree status in `{}`: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let status = String::from_utf8(output.stdout)?;
	Ok(status
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.any(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line)))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::orchestrator::PullRequestReviewState;
	use crate::state::{
		ReviewHandoffMarker, ReviewLifecycleRecord, ReviewPolicyCheckpointInput, StateStore,
	};

	#[test]
	fn post_review_lifecycle_facts_preserve_lineage_and_runtime_gate() {
		let review_state = PullRequestReviewState {
			url: String::from("https://github.com/hack-ink/decodex/pull/173"),
			state: String::from("OPEN"),
			is_draft: false,
			review_decision: Some(String::from("APPROVED")),
			merge_commit_allowed: true,
			pending_review_requests: 0,
			mergeable: String::from("MERGEABLE"),
			merge_state_status: String::from("CLEAN"),
			head_ref_name: String::from("x/pub-101"),
			head_ref_oid: String::from("head-sha"),
			merge_commit_oid: None,
			head_repository_name: None,
			head_repository_owner: None,
			status_check_rollup_state: Some(String::from("SUCCESS")),
			unresolved_review_threads: 0,
			issue_description_external_review_thumbs_up_count: 0,
			issue_comments: Vec::new(),
			reviews: Vec::new(),
		};
		let handoff = ReviewHandoffMarker::new(
			"run-1",
			2,
			"x/pub-101",
			"https://github.com/hack-ink/decodex/pull/173",
			"main",
			"x/pub-101",
			"head-sha",
		);

		let facts = build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			review_lifecycle: Some(&ReviewLifecycleRecord::from_test_review_markers(
				&handoff, None,
			)),
			review_state: &review_state,
			worktree_path: Path::new("/tmp/pubfi"),
			review_level: ReviewLevel::Standard,
			phase: "request_pending",
			landing_state: None,
			closeout_state: None,
			validated_head_sha: Some("head-sha"),
			review_checkpoint_phase: Some("handoff"),
			review_checkpoint_status: Some("clean"),
		});

		assert_eq!(facts.project_id, "pubfi");
		assert_eq!(facts.issue_id, "PUB-101");
		assert_eq!(facts.base_branch.as_deref(), Some("main"));
		assert_eq!(facts.head_branch, "x/pub-101");
		assert_eq!(facts.validated_head_sha, "head-sha");
		assert_eq!(facts.review_level, "standard");
		assert_eq!(facts.review_gate_state, RuntimeReviewGateState::Clean);
		assert_eq!(
			facts.source_evidence_refs,
			vec![
				String::from("pr_readback:https://github.com/hack-ink/decodex/pull/173:head-sha"),
				String::from("review_lifecycle:run-1:2:head-sha"),
				String::from("review_checkpoint:standard:handoff:head-sha"),
			]
		);
	}

	#[test]
	fn runtime_review_checkpoint_status_for_head_prefers_current_same_head_handoff_artifact() {
		let state_store = StateStore::open_in_memory().expect("state store should open");

		state_store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: "pubfi",
				issue_id: "PUB-101",
				run_id: "run-1:runtime-review:repair:old",
				attempt_number: 1,
				phase: "repair",
				review_level: "standard",
				status: "findings",
				head_sha: "head-sha",
				nonclean_rounds: 1,
				details_json: "{}",
			})
			.expect("repair checkpoint should persist");
		state_store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: "pubfi",
				issue_id: "PUB-101",
				run_id: "run-1:runtime-review:handoff:new",
				attempt_number: 1,
				phase: "handoff",
				review_level: "standard",
				status: "clean",
				head_sha: "head-sha",
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("handoff checkpoint should persist");

		let checkpoint = runtime_review_checkpoint_status_for_head(
			&state_store,
			"pubfi",
			"PUB-101",
			ReviewLevel::Standard,
			"head-sha",
		)
		.expect("checkpoint lookup should succeed")
		.expect("checkpoint should exist");

		assert_eq!(checkpoint.phase, "handoff");
		assert_eq!(checkpoint.status, "clean");
	}
}
