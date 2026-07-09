use super::terminal_lifecycle_authority_must_not_reenter_review;
#[cfg(test)]
use crate::state::{ReviewLifecycleHandoffFixture, ReviewLifecycleTransitionFixture};
use crate::{
	orchestrator::{
		PostReviewLifecycleFacts, RuntimeReviewGateState,
		kernel::lifecycle::{
			LifecycleDecisionInput, LifecycleEvidenceKind, LifecycleOutcome,
			PreviousLifecycleAuthority, decide_lifecycle_transition,
		},
	},
	prelude::Result,
	state::{
		ReviewLifecycleTransitionInput, StateStore, runtime_records::ReviewLifecycleKey,
		runtime_row_parsers,
	},
};

impl StateStore {
	pub(crate) fn record_review_lifecycle_transition(
		&self,
		project_id: &str,
		issue_id: &str,
		input: ReviewLifecycleTransitionInput<'_>,
	) -> Result<()> {
		if self
			.review_lifecycle_record(project_id, issue_id, input.branch_name)?
			.is_some_and(|record| terminal_lifecycle_authority_must_not_reenter_review(&record))
		{
			return Ok(());
		}

		record_orchestration_lifecycle_authority(self, project_id, issue_id, &input)?;
		let now = runtime_row_parsers::timestamp_parts();
		let key = ReviewLifecycleKey::new(project_id, issue_id, input.branch_name);
		let mut state = self.lock()?;
		let Some(record) = state.review_lifecycle_records.get_mut(&key) else {
			return Ok(());
		};

		record.run_id = input.run_id.to_owned();
		record.attempt_number = input.attempt_number;
		record.pr_url = input.pr_url.to_owned();
		record.head_sha = input.head_sha.to_owned();
		record.phase = input.phase.to_owned();
		record.validated_head_sha = input.head_sha.to_owned();
		record.request_comment_database_id = input.request_comment_database_id;
		record.request_created_at_unix_epoch = input.request_created_at_unix_epoch;
		record.request_description_thumbs_up_count = input.request_description_thumbs_up_count;
		record.request_retry_count = input.request_retry_count;
		record.external_round_count = input.external_round_count;
		record.auto_merge_enabled_at_unix_epoch = input.auto_merge_enabled_at_unix_epoch;
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		self.persist_runtime_state_locked(&state)
	}

	/// Create or replace the retained review orchestration projection for one issue lane.
	#[cfg(test)]
	pub(crate) fn upsert_review_lifecycle_transition_fixture(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewLifecycleTransitionFixture,
	) -> Result<()> {
		self.record_review_lifecycle_transition(
			project_id,
			issue_id,
			ReviewLifecycleTransitionInput {
				run_id: marker.run_id(),
				attempt_number: marker.attempt_number(),
				branch_name: marker.branch_name(),
				pr_url: marker.pr_url(),
				head_sha: marker.head_sha(),
				phase: marker.phase(),
				request_comment_database_id: marker.request_comment_database_id(),
				request_created_at_unix_epoch: marker.request_created_at_unix_epoch(),
				request_description_thumbs_up_count: marker.request_description_thumbs_up_count(),
				request_retry_count: marker.request_retry_count(),
				external_round_count: marker.external_round_count(),
				auto_merge_enabled_at_unix_epoch: marker.auto_merge_enabled_at_unix_epoch(),
			},
		)
	}

	/// Read retained review orchestration for the current handoff identity.
	#[cfg(test)]
	pub(crate) fn review_lifecycle_transition_fixture(
		&self,
		project_id: &str,
		issue_id: &str,
		review_handoff: &ReviewLifecycleHandoffFixture,
	) -> Result<Option<ReviewLifecycleTransitionFixture>> {
		let Some(record) =
			self.review_lifecycle_record(project_id, issue_id, review_handoff.branch_name())?
		else {
			return Ok(None);
		};

		if record.run_id() != review_handoff.run_id()
			|| record.attempt_number() != review_handoff.attempt_number()
			|| record.branch_name() != review_handoff.branch_name()
			|| record.pr_url() != review_handoff.pr_url()
		{
			return Ok(None);
		}

		Ok(Some(ReviewLifecycleTransitionFixture::new(
			record.run_id().to_owned(),
			record.attempt_number(),
			record.branch_name().to_owned(),
			record.pr_url().to_owned(),
			record.head_sha().to_owned(),
			record.phase().to_owned(),
			record.request_comment_database_id(),
			record.request_created_at_unix_epoch(),
			record.request_description_thumbs_up_count(),
			record.request_retry_count(),
			record.external_round_count(),
			record.auto_merge_enabled_at_unix_epoch(),
		)))
	}
}

fn record_orchestration_lifecycle_authority(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	input: &ReviewLifecycleTransitionInput<'_>,
) -> Result<()> {
	let previous_record =
		state_store.review_lifecycle_record(project_id, issue_id, input.branch_name)?;
	let previous = previous_record.as_ref().map(|record| PreviousLifecycleAuthority {
		sequence: record.sequence(),
		next_state: record.next_state(),
	});
	let base_branch = previous_record
		.as_ref()
		.and_then(|record| record.target_base_ref_name())
		.map(str::to_owned);
	let decided_at = runtime_row_parsers::timestamp_parts().text;
	let evidence_kind = lifecycle_evidence_kind_for_phase(input.phase);
	let idempotency_key = format!(
		"{project_id}:{issue_id}:{}:{}:{}:{}:{}:{}",
		input.branch_name,
		evidence_kind.as_str(),
		input.phase,
		input.head_sha,
		input.request_retry_count,
		input.external_round_count
	);
	let facts = PostReviewLifecycleFacts {
		project_id: project_id.to_owned(),
		issue_id: issue_id.to_owned(),
		pr_url: input.pr_url.to_owned(),
		base_branch,
		head_branch: input.branch_name.to_owned(),
		validated_head_sha: input.head_sha.to_owned(),
		worktree_path: String::new(),
		review_level: String::new(),
		review_gate_state: RuntimeReviewGateState::NotRequired,
		phase: input.phase.to_owned(),
		landing_state: String::from("not_started"),
		closeout_state: String::from("not_started"),
		source_evidence_refs: vec![format!(
			"review_orchestration:{}:{}:{}:{}",
			input.run_id, input.attempt_number, input.phase, input.head_sha
		)],
	};
	let decision = decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous,
		evidence_kind,
		outcome: LifecycleOutcome::Intent,
		merge_commit: None,
		cleanup_state: Some("not_started"),
		authority: "review_lifecycle_runtime",
		actor: "state_adapter",
		idempotency_key: &idempotency_key,
		correlation_id: input.run_id,
		causation_id: Some(input.phase),
		decided_at: &decided_at,
	});

	state_store.record_lifecycle_decision(input.run_id, input.attempt_number, &decision)?;

	Ok(())
}

fn lifecycle_evidence_kind_for_phase(phase: &str) -> LifecycleEvidenceKind {
	match phase {
		"repair_required" => LifecycleEvidenceKind::ReviewRepair,
		"waiting_for_merge" => LifecycleEvidenceKind::LandingIntent,
		_ => LifecycleEvidenceKind::ReviewWait,
	}
}
