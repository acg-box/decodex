use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	lane_authority::{IntakeAuthority, IntakeAuthorityKind, LaneCommand},
	orchestrator,
	prelude::Result,
	recovery::{self, AdoptValidation, RecoveryContext},
	state::{ReviewLifecycleHandoffInput, ReviewLifecycleTransitionInput},
};

pub(in crate::recovery::review_handoff_apply::adopt) fn write_adopt_local_state(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	handoff_input: ReviewLifecycleHandoffInput<'_>,
	transition_input: ReviewLifecycleTransitionInput<'_>,
) -> Result<()> {
	let worktree_path = validation.worktree_path.to_string_lossy().to_string();
	let attestation = orchestrator::attest_issue_project_binding(
		&context.state_store,
		&context.config,
		&validation.issue,
	)?;
	let lane_id = attestation.lane_id().clone();
	if context
		.state_store
		.lane(&lane_id)?
		.and_then(|lane| lane.intake_authority_id().map(ToOwned::to_owned))
		.is_none()
	{
		let evidence_fingerprint = recovery_adoption_evidence_fingerprint(validation);
		let now = OffsetDateTime::now_utc();
		let accepted_at = now.format(&Rfc3339)?;
		let program_id = format!("recovery-adopt-{}", validation.run_id);
		let authority = IntakeAuthority::new(
			&format!("intake-authority-{program_id}"),
			context.config.service_id(),
			attestation.project().clone(),
			&format!("plan-{program_id}"),
			&program_id,
			"operator",
			"review_handoff_adopt",
			&format!("recovery-adopt:{}", validation.run_id),
			&accepted_at,
			now.unix_timestamp(),
			IntakeAuthorityKind::RecoveryAdoption {
				recovery_request_id: validation.run_id.clone(),
				evidence_fingerprint,
			},
		)?;
		let authority = context.state_store.persist_intake_authority(authority)?;
		context.state_store.apply_lane_command(
			lane_id.clone(),
			attestation.binding_fingerprint(),
			LaneCommand::Admit { intake_authority_id: authority.authority_id().to_owned() },
		)?;
	}
	if let Some(lane) = context.state_store.lane(&lane_id)?
		&& lane.phase() == crate::lane_authority::LanePhase::WaitingReview
	{
		if lane.claim_run_id() == Some(validation.run_id.as_str())
			&& lane.branch_name() == Some(validation.branch_name.as_str())
			&& lane.worktree_path().map(|path| path.as_path())
				== Some(validation.worktree_path.as_path())
		{
			return Ok(());
		}
		crate::prelude::eyre::bail!(
			"Recovery adoption conflicts with an existing waiting-review lane."
		);
	}
	if !context.state_store.try_acquire_registered_lease(
		context.config.service_id(),
		&validation.issue.id,
		&validation.run_id,
		&validation.issue.state.name,
	)? {
		crate::prelude::eyre::bail!(
			"Recovery adoption could not acquire the canonical lane claim."
		);
	}

	context
		.state_store
		.upsert_claimed_worktree(
			context.config.service_id(),
			&validation.issue.id,
			&validation.branch_name,
			&worktree_path,
		)
		.and_then(|()| {
			context.state_store.record_lane_run_attempt(
				context.config.service_id(),
				&validation.run_id,
				&validation.issue.id,
				validation.attempt_number,
				"starting",
			)
		})
		.and_then(|()| {
			context
				.state_store
				.apply_lane_command(
					lane_id.clone(),
					attestation.binding_fingerprint(),
					LaneCommand::BeginRun,
				)
				.map(|_| ())
		})
		.and_then(|()| {
			context.state_store.record_review_lifecycle_handoff(
				context.config.service_id(),
				&validation.issue.id,
				handoff_input,
			)
		})
		.and_then(|()| {
			context.state_store.record_review_lifecycle_transition(
				context.config.service_id(),
				&validation.issue.id,
				transition_input,
			)
		})
		.and_then(|()| {
			context
				.state_store
				.apply_lane_command(
					lane_id,
					attestation.binding_fingerprint(),
					LaneCommand::BeginReview,
				)
				.map(|_| ())
		})
}

fn recovery_adoption_evidence_fingerprint(validation: &AdoptValidation) -> String {
	let mut digest = Sha256::new();
	for value in [
		validation.issue.id.as_str(),
		validation.run_id.as_str(),
		validation.branch_name.as_str(),
		validation.local_head_oid.as_str(),
		recovery::landing_url(&validation.landing_state),
	] {
		digest.update((value.len() as u64).to_be_bytes());
		digest.update(value.as_bytes());
	}
	format!(
		"sha256:{}",
		digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>()
	)
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use tempfile::TempDir;

	use super::*;
	use crate::{
		lane_authority::{IntakeAuthorityKind, LanePhase},
		recovery::{RecoveryRuntimeMutationPolicy, tests as recovery_tests},
	};

	#[test]
	fn adoption_creates_typed_authority_and_waiting_review_lane() {
		let temp_dir = TempDir::new().expect("tempdir");
		let context = recovery_tests::sample_recovery_context(
			&temp_dir,
			RecoveryRuntimeMutationPolicy::ReadOnly,
		);
		let branch = "x/pubfi-pub-718";
		let head = "1123456789abcdef0123456789abcdef01234567";
		let mut issue = recovery_tests::sample_issue("In Review");
		issue.team.id = String::from("team-test");
		let validation = AdoptValidation {
			issue,
			branch_name: branch.to_owned(),
			worktree_path: Path::new("/tmp/PUB-718").to_path_buf(),
			run_id: String::from("pub-718-manual-adopt-2-1123456789ab"),
			attempt_number: 2,
			landing_state: recovery_tests::sample_landing_state(
				"https://github.com/hack-ink/pubfi-mono-v2/pull/14",
				branch,
				head,
			),
			local_head_oid: head.to_owned(),
			worktree_path_for_event: Some(String::from(".worktrees/PUB-718")),
			active_label_present: true,
			success_state_transition: None,
			previous_worktree_mapping: None,
		};
		let handoff = ReviewLifecycleHandoffInput {
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
			branch_name: branch,
			pr_url: recovery::landing_url(&validation.landing_state),
			base_ref_name: &validation.landing_state.base_ref_name,
			head_ref_name: &validation.landing_state.head_ref_name,
			head_sha: head,
		};
		let transition = ReviewLifecycleTransitionInput {
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
			branch_name: branch,
			pr_url: recovery::landing_url(&validation.landing_state),
			head_sha: head,
			phase: "rebound",
			request_comment_database_id: None,
			request_created_at_unix_epoch: None,
			request_description_thumbs_up_count: None,
			request_retry_count: 0,
			external_round_count: 0,
			auto_merge_enabled_at_unix_epoch: None,
		};

		write_adopt_local_state(&context, &validation, handoff, transition)
			.expect("adoption state");

		let lane_id =
			crate::lane_authority::LaneId::new("pubfi", &validation.issue.id).expect("lane id");
		let lane = context.state_store.lane(&lane_id).expect("lane read").expect("lane");
		assert_eq!(lane.phase(), LanePhase::WaitingReview);
		assert_eq!(lane.claim_run_id(), Some(validation.run_id.as_str()));
		let settled_epoch = lane.epoch();
		write_adopt_local_state(&context, &validation, handoff, transition)
			.expect("exact adoption retry");
		assert_eq!(
			context
				.state_store
				.lane(&lane_id)
				.expect("retry lane read")
				.expect("retry lane")
				.epoch(),
			settled_epoch,
			"exact retry must be a no-op",
		);
		let authority = context
			.state_store
			.intake_authority("pubfi", lane.intake_authority_id().expect("authority id"))
			.expect("authority read")
			.expect("authority");
		assert!(matches!(authority.authority(), IntakeAuthorityKind::RecoveryAdoption { .. }));
		let attempt = context
			.state_store
			.run_attempt(&validation.run_id)
			.expect("attempt read")
			.expect("attempt");
		assert_eq!(attempt.project_id(), Some("pubfi"));
	}
}
