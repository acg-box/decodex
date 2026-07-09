use serde::{Deserialize, Serialize};

use crate::orchestrator::PostReviewLifecycleFacts;

pub(crate) const LIFECYCLE_AUTHORITY_SCHEMA_VERSION: &str = "decodex/lifecycle-authority-record/1";
pub(crate) const LIFECYCLE_EVENT_SCHEMA_VERSION: &str = "decodex/lifecycle-event/1";
pub(crate) const LIFECYCLE_EVENT_TYPE: &str = "lifecycle_event";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleEvidenceKind {
	Handoff,
	ReviewWait,
	ReviewRepair,
	LandingIntent,
	LandingReadback,
	CloseoutCompletion,
}
impl LifecycleEvidenceKind {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Handoff => "handoff",
			Self::ReviewWait => "review_wait",
			Self::ReviewRepair => "review_repair",
			Self::LandingIntent => "landing_intent",
			Self::LandingReadback => "landing_readback",
			Self::CloseoutCompletion => "closeout_completion",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleOutcome {
	Intent,
	Succeeded,
	Failed,
	NeedsManualAttention,
}
impl LifecycleOutcome {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Intent => "intent",
			Self::Succeeded => "succeeded",
			Self::Failed => "failed",
			Self::NeedsManualAttention => "manual_attention_required",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleDecisionInput<'a> {
	pub(crate) facts: &'a PostReviewLifecycleFacts,
	pub(crate) previous: Option<PreviousLifecycleAuthority<'a>>,
	pub(crate) evidence_kind: LifecycleEvidenceKind,
	pub(crate) outcome: LifecycleOutcome,
	pub(crate) merge_commit: Option<&'a str>,
	pub(crate) cleanup_state: Option<&'a str>,
	pub(crate) authority: &'a str,
	pub(crate) actor: &'a str,
	pub(crate) idempotency_key: &'a str,
	pub(crate) correlation_id: &'a str,
	pub(crate) causation_id: Option<&'a str>,
	pub(crate) decided_at: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PreviousLifecycleAuthority<'a> {
	pub(crate) sequence: i64,
	pub(crate) next_state: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct LifecycleAuthorityRecord {
	pub(crate) schema_version: String,
	pub(crate) project_id: String,
	pub(crate) service_id: String,
	pub(crate) issue_id: String,
	pub(crate) subject_id: String,
	pub(crate) sequence: i64,
	pub(crate) phase: String,
	pub(crate) transition: String,
	pub(crate) previous_state: String,
	pub(crate) next_state: String,
	pub(crate) next_action: String,
	pub(crate) review_level: String,
	pub(crate) review_gate_state: String,
	pub(crate) pr_url: String,
	pub(crate) base_branch: Option<String>,
	pub(crate) head_branch: String,
	pub(crate) validated_head_sha: String,
	pub(crate) worktree_path: String,
	pub(crate) merge_commit: Option<String>,
	pub(crate) cleanup_state: String,
	pub(crate) authority: String,
	pub(crate) actor: String,
	pub(crate) source_evidence_refs: Vec<String>,
	pub(crate) idempotency_key: String,
	pub(crate) correlation_id: String,
	pub(crate) causation_id: Option<String>,
	pub(crate) decided_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct LifecycleEventEnvelope {
	pub(crate) schema_version: String,
	pub(crate) event_type: String,
	pub(crate) subject_id: String,
	pub(crate) sequence: i64,
	pub(crate) idempotency_key: String,
	pub(crate) correlation_id: String,
	pub(crate) causation_id: Option<String>,
	pub(crate) authority_record: LifecycleAuthorityRecord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleDecision {
	pub(crate) authority_record: LifecycleAuthorityRecord,
	pub(crate) authority_record_envelope: LifecycleEventEnvelope,
}

pub(crate) fn decide_lifecycle_transition(input: LifecycleDecisionInput<'_>) -> LifecycleDecision {
	let previous_state = input
		.previous
		.map(|record| record.next_state.to_owned())
		.unwrap_or_else(|| String::from("none"));
	let sequence = input.previous.map(|record| record.sequence + 1).unwrap_or(1);
	let transition = lifecycle_transition(input.evidence_kind, input.outcome);
	let next_state = lifecycle_next_state(input.evidence_kind, input.outcome);
	let phase = lifecycle_phase(input.evidence_kind, input.outcome, &input.facts.phase);
	let next_action =
		lifecycle_next_action(input.evidence_kind, input.outcome, &next_state, &phase);
	let mut source_evidence_refs = input.facts.source_evidence_refs.clone();

	source_evidence_refs.push(format!(
		"lifecycle_evidence:{}:{}",
		input.evidence_kind.as_str(),
		input.outcome.as_str()
	));

	let authority_record = LifecycleAuthorityRecord {
		schema_version: LIFECYCLE_AUTHORITY_SCHEMA_VERSION.to_owned(),
		project_id: input.facts.project_id.clone(),
		service_id: input.facts.project_id.clone(),
		issue_id: input.facts.issue_id.clone(),
		subject_id: lifecycle_subject_id(input.facts),
		sequence,
		phase,
		transition,
		previous_state,
		next_state,
		next_action,
		review_level: input.facts.review_level.clone(),
		review_gate_state: input.facts.review_gate_state.as_str().to_owned(),
		pr_url: input.facts.pr_url.clone(),
		base_branch: input.facts.base_branch.clone(),
		head_branch: input.facts.head_branch.clone(),
		validated_head_sha: input.facts.validated_head_sha.clone(),
		worktree_path: input.facts.worktree_path.clone(),
		merge_commit: input.merge_commit.map(str::to_owned),
		cleanup_state: input.cleanup_state.unwrap_or("not_started").to_owned(),
		authority: input.authority.to_owned(),
		actor: input.actor.to_owned(),
		source_evidence_refs,
		idempotency_key: input.idempotency_key.to_owned(),
		correlation_id: input.correlation_id.to_owned(),
		causation_id: input.causation_id.map(str::to_owned),
		decided_at: input.decided_at.to_owned(),
	};
	let authority_record_envelope = LifecycleEventEnvelope {
		schema_version: LIFECYCLE_EVENT_SCHEMA_VERSION.to_owned(),
		event_type: String::from("lifecycle_authority_recorded"),
		subject_id: authority_record.subject_id.clone(),
		sequence,
		idempotency_key: authority_record.idempotency_key.clone(),
		correlation_id: authority_record.correlation_id.clone(),
		causation_id: authority_record.causation_id.clone(),
		authority_record: authority_record.clone(),
	};

	LifecycleDecision { authority_record, authority_record_envelope }
}

fn lifecycle_subject_id(facts: &PostReviewLifecycleFacts) -> String {
	format!("{}:{}:{}", facts.project_id, facts.issue_id, facts.head_branch)
}

fn lifecycle_transition(kind: LifecycleEvidenceKind, outcome: LifecycleOutcome) -> String {
	match (kind, outcome) {
		(LifecycleEvidenceKind::Handoff, _) => "review_handoff_recorded",
		(LifecycleEvidenceKind::ReviewWait, LifecycleOutcome::Intent) => "review_wait_recorded",
		(LifecycleEvidenceKind::ReviewRepair, LifecycleOutcome::Intent) => "review_repair_required",
		(LifecycleEvidenceKind::LandingIntent, _) => "landing_started",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::Succeeded) => "landed",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::Failed) => "landing_failed",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::NeedsManualAttention) =>
			"manual_attention_required",
		(LifecycleEvidenceKind::CloseoutCompletion, LifecycleOutcome::Succeeded) =>
			"closeout_completed",
		(LifecycleEvidenceKind::CloseoutCompletion, LifecycleOutcome::Failed) => "closeout_failed",
		(_, LifecycleOutcome::NeedsManualAttention) => "manual_attention_required",
		(_, LifecycleOutcome::Intent) => "intent_recorded",
		(_, LifecycleOutcome::Succeeded) => "completed",
		(_, LifecycleOutcome::Failed) => "failed",
	}
	.to_owned()
}

fn lifecycle_next_state(kind: LifecycleEvidenceKind, outcome: LifecycleOutcome) -> String {
	match (kind, outcome) {
		(LifecycleEvidenceKind::Handoff, _) => "review_pending",
		(LifecycleEvidenceKind::ReviewWait, LifecycleOutcome::Intent) => "review_waiting",
		(LifecycleEvidenceKind::ReviewRepair, LifecycleOutcome::Intent) => "repair_required",
		(LifecycleEvidenceKind::LandingIntent, _) => "landing_started",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::Succeeded) => "landed",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::Failed) => "landing_failed",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::NeedsManualAttention) =>
			"manual_attention_required",
		(LifecycleEvidenceKind::CloseoutCompletion, LifecycleOutcome::Succeeded) => "closed",
		(LifecycleEvidenceKind::CloseoutCompletion, LifecycleOutcome::Failed) => "closeout_failed",
		(_, LifecycleOutcome::NeedsManualAttention) => "manual_attention_required",
		(_, LifecycleOutcome::Intent) => "intent_recorded",
		(_, LifecycleOutcome::Succeeded) => "completed",
		(_, LifecycleOutcome::Failed) => "failed",
	}
	.to_owned()
}

fn lifecycle_phase(
	kind: LifecycleEvidenceKind,
	outcome: LifecycleOutcome,
	current_phase: &str,
) -> String {
	match (kind, outcome) {
		(LifecycleEvidenceKind::Handoff, _) => "request_pending",
		(LifecycleEvidenceKind::ReviewWait, _) => current_phase,
		(LifecycleEvidenceKind::ReviewRepair, _) => "repair_required",
		(LifecycleEvidenceKind::LandingIntent, _) => "waiting_for_merge",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::Succeeded) => "landed",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::Failed) => "landing_failed",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::NeedsManualAttention) =>
			"manual_attention_required",
		(LifecycleEvidenceKind::CloseoutCompletion, LifecycleOutcome::Succeeded) => "closed",
		(LifecycleEvidenceKind::CloseoutCompletion, LifecycleOutcome::Failed) => "closeout_failed",
		(_, LifecycleOutcome::NeedsManualAttention) => "manual_attention_required",
		_ => current_phase,
	}
	.to_owned()
}

fn lifecycle_next_action(
	kind: LifecycleEvidenceKind,
	outcome: LifecycleOutcome,
	next_state: &str,
	phase: &str,
) -> String {
	if matches!(kind, LifecycleEvidenceKind::ReviewWait) {
		return match phase {
			"request_pending" => "request_external_review",
			"waiting_for_ack" => "wait_for_external_review_ack",
			"waiting_for_result" => "wait_for_external_review_result",
			"pass_waiting_for_gates" => "wait_for_landing_gates",
			_ => "wait_for_external_review_signal",
		}
		.to_owned();
	}

	match (kind, outcome, next_state) {
		(LifecycleEvidenceKind::Handoff, _, "review_pending") =>
			"wait_for_runtime_review_gate_or_external_review",
		(LifecycleEvidenceKind::ReviewRepair, _, _) => "run_retained_review_repair_adapter",
		(LifecycleEvidenceKind::LandingIntent, _, _) => "poll_landing_readback",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::Succeeded, _) =>
			"run_retained_closeout_adapter",
		(LifecycleEvidenceKind::LandingReadback, LifecycleOutcome::Failed, _) =>
			"repair_landing_failure_or_request_manual_attention",
		(LifecycleEvidenceKind::CloseoutCompletion, LifecycleOutcome::Succeeded, _) => "no_action",
		(_, LifecycleOutcome::NeedsManualAttention, _) => "request_manual_attention",
		_ => "continue_lifecycle",
	}
	.to_owned()
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use crate::{
		config::ReviewLevel,
		orchestrator::{
			self, PostReviewLifecycleFactsInput, PullRequestReviewState,
			kernel::lifecycle::{
				LIFECYCLE_EVENT_SCHEMA_VERSION, LifecycleDecisionInput, LifecycleEvidenceKind,
				LifecycleOutcome,
			},
		},
		state::{ReviewLifecycleHandoffFixture, ReviewLifecycleRecord},
	};

	#[test]
	fn lifecycle_kernel_is_pure_and_emits_authority_record_envelope() {
		let facts =
			orchestrator::build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
				project_id: "pubfi",
				issue_id: "PUB-101",
				review_lifecycle: Some(&ReviewLifecycleRecord::from_test_lifecycle_fixtures(
					&ReviewLifecycleHandoffFixture::new(
						"run-1",
						1,
						"x/pub-101",
						"https://github.com/hack-ink/decodex/pull/101",
						"main",
						"x/pub-101",
						"head-sha",
					),
					None,
				)),
				review_state: &PullRequestReviewState {
					url: String::from("https://github.com/hack-ink/decodex/pull/101"),
					state: String::from("OPEN"),
					is_draft: false,
					review_decision: Some(String::from("APPROVED")),
					merge_commit_allowed: true,
					pending_review_requests: 0,
					mergeable: String::from("MERGEABLE"),
					merge_state_status: String::from("CLEAN"),
					base_ref_oid: Some(String::from("base-sha")),
					head_ref_name: String::from("x/pub-101"),
					head_ref_oid: String::from("head-sha"),
					merge_commit_oid: None,
					head_repository_name: None,
					head_repository_owner: None,
					status_check_rollup_state: Some(String::from("SUCCESS")),
					required_status_contexts: Vec::new(),
					unresolved_review_threads: 0,
					issue_description_external_review_thumbs_up_count: 0,
					issue_comments: Vec::new(),
					reviews: Vec::new(),
				},
				worktree_path: Path::new("/tmp/pubfi"),
				review_level: ReviewLevel::Standard,
				phase: "request_pending",
				landing_state: None,
				closeout_state: None,
				validated_head_sha: Some("head-sha"),
				review_checkpoint_phase: Some("handoff"),
				review_checkpoint_status: Some("clean"),
			});
		let decision = super::decide_lifecycle_transition(LifecycleDecisionInput {
			facts: &facts,
			previous: None,
			evidence_kind: LifecycleEvidenceKind::LandingReadback,
			outcome: LifecycleOutcome::Succeeded,
			merge_commit: Some("merge-sha"),
			cleanup_state: Some("pending"),
			authority: "issue_authority",
			actor: "runtime",
			idempotency_key: "PUB-101:landed:merge-sha",
			correlation_id: "corr-1",
			causation_id: Some("landing-intent-1"),
			decided_at: "2026-07-07T00:00:00Z",
		});

		assert_eq!(decision.authority_record.sequence, 1);
		assert_eq!(decision.authority_record.transition, "landed");
		assert_eq!(decision.authority_record.next_state, "landed");
		assert_eq!(decision.authority_record.merge_commit.as_deref(), Some("merge-sha"));
		assert_eq!(
			decision.authority_record_envelope.schema_version,
			LIFECYCLE_EVENT_SCHEMA_VERSION
		);
		assert_eq!(decision.authority_record_envelope.authority_record, decision.authority_record);
	}
}
