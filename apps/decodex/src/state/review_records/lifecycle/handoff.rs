#[cfg(test)]
use crate::state::{ReviewLifecycleHandoffFixture, runtime_records::ReviewLifecycleKey};
use crate::{
	orchestrator::{
		PostReviewLifecycleFacts, RuntimeReviewGateState,
		kernel::lifecycle::{
			LifecycleDecisionInput, LifecycleEvidenceKind, LifecycleOutcome,
			PreviousLifecycleAuthority, decide_lifecycle_transition,
		},
	},
	prelude::Result,
	state::{ReviewLifecycleHandoffInput, StateStore, runtime_row_parsers},
};

impl StateStore {
	/// Create or replace the retained review lifecycle authority for one handoff lane.
	pub(crate) fn record_review_lifecycle_handoff(
		&self,
		project_id: &str,
		issue_id: &str,
		input: ReviewLifecycleHandoffInput<'_>,
	) -> Result<()> {
		record_handoff_lifecycle_authority(self, project_id, issue_id, &input)
	}

	/// Create or replace the retained review handoff projection for one issue lane.
	#[cfg(test)]
	pub(crate) fn upsert_review_lifecycle_handoff_fixture(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewLifecycleHandoffFixture,
	) -> Result<()> {
		record_handoff_lifecycle_authority(
			self,
			project_id,
			issue_id,
			&ReviewLifecycleHandoffInput {
				run_id: marker.run_id(),
				attempt_number: marker.attempt_number(),
				branch_name: marker.branch_name(),
				pr_url: marker.pr_url(),
				base_ref_name: marker.target_base_ref_name().unwrap_or_default(),
				head_ref_name: marker.pr_head_ref_name(),
				head_sha: marker.pr_head_oid(),
			},
		)?;
		let now = runtime_row_parsers::timestamp_parts();
		let key = ReviewLifecycleKey::new(project_id, issue_id, marker.branch_name());
		let mut state = self.lock()?;
		let Some(record) = state.review_lifecycle_records.get_mut(&key) else {
			return Ok(());
		};
		let same_handoff_projection = record.run_id == marker.run_id()
			&& record.attempt_number == marker.attempt_number()
			&& record.pr_url == marker.pr_url()
			&& record.target_base_ref_name.as_deref() == marker.target_base_ref_name()
			&& record.pr_head_ref_name == marker.pr_head_ref_name()
			&& record.pr_head_oid == marker.pr_head_oid();

		record.run_id = marker.run_id().to_owned();
		record.attempt_number = marker.attempt_number();
		record.pr_url = marker.pr_url().to_owned();
		record.target_base_ref_name = marker.target_base_ref_name().map(str::to_owned);
		record.pr_head_ref_name = marker.pr_head_ref_name().to_owned();
		record.pr_head_oid = marker.pr_head_oid().to_owned();

		if !same_handoff_projection {
			record.head_sha = marker.pr_head_oid().to_owned();
			record.phase = String::from("request_pending");
			record.request_comment_database_id = None;
			record.request_created_at_unix_epoch = None;
			record.request_description_thumbs_up_count = None;
			record.request_retry_count = 0;
			record.external_round_count = 0;
			record.auto_merge_enabled_at_unix_epoch = None;
			record.landing_state = String::from("not_started");
			record.closeout_state = String::from("not_started");
			record.repair_attempt_count = 0;
			record.base_branch = marker.target_base_ref_name().map(str::to_owned);
			record.validated_head_sha = marker.pr_head_oid().to_owned();
			record.merge_commit = None;
			record.cleanup_state = String::from("not_started");
		}

		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		self.persist_runtime_state_locked(&state)
	}

	/// Read the retained review handoff projection for one issue branch.
	#[cfg(test)]
	pub(crate) fn review_lifecycle_handoff_fixture(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
	) -> Result<Option<ReviewLifecycleHandoffFixture>> {
		Ok(self.review_lifecycle_record(project_id, issue_id, branch_name)?.map(|record| {
			ReviewLifecycleHandoffFixture {
				run_id: record.run_id().to_owned(),
				attempt_number: record.attempt_number(),
				branch_name: record.branch_name().to_owned(),
				pr_url: record.pr_url().to_owned(),
				target_base_ref_name: record.target_base_ref_name().map(str::to_owned),
				pr_head_ref_name: record.pr_head_ref_name().to_owned(),
				pr_head_oid: record.pr_head_oid().to_owned(),
			}
		}))
	}
}

fn record_handoff_lifecycle_authority(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	input: &ReviewLifecycleHandoffInput<'_>,
) -> Result<()> {
	let previous_record =
		state_store.review_lifecycle_record(project_id, issue_id, input.branch_name)?;
	let previous = previous_record.as_ref().map(|record| PreviousLifecycleAuthority {
		sequence: record.sequence(),
		next_state: record.next_state(),
	});
	let decided_at = runtime_row_parsers::timestamp_parts().text;
	let idempotency_key = format!(
		"{project_id}:{issue_id}:{}:{}:{}",
		input.branch_name,
		LifecycleEvidenceKind::Handoff.as_str(),
		input.head_sha
	);
	let facts = PostReviewLifecycleFacts {
		project_id: project_id.to_owned(),
		issue_id: issue_id.to_owned(),
		pr_url: input.pr_url.to_owned(),
		base_branch: Some(input.base_ref_name.to_owned()),
		head_branch: input.branch_name.to_owned(),
		validated_head_sha: input.head_sha.to_owned(),
		worktree_path: String::new(),
		review_level: String::new(),
		review_gate_state: RuntimeReviewGateState::NotRequired,
		phase: String::from("request_pending"),
		landing_state: String::from("not_started"),
		closeout_state: String::from("not_started"),
		source_evidence_refs: vec![format!(
			"review_lifecycle_handoff:{}:{}:{}",
			input.run_id, input.attempt_number, input.head_sha
		)],
	};
	let decision = decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous,
		evidence_kind: LifecycleEvidenceKind::Handoff,
		outcome: LifecycleOutcome::Intent,
		merge_commit: None,
		cleanup_state: Some("not_started"),
		authority: "review_lifecycle_runtime",
		actor: "state_adapter",
		idempotency_key: &idempotency_key,
		correlation_id: input.run_id,
		causation_id: Some("review_lifecycle_handoff"),
		decided_at: &decided_at,
	});

	state_store.record_lifecycle_decision(input.run_id, input.attempt_number, &decision)?;

	Ok(())
}
