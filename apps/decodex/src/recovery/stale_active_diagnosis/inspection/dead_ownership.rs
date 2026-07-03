use crate::{
	prelude::Result,
	recovery::{
		process_liveness::StaleActiveProcessLiveness,
		stale_active_diagnosis::inspection::inputs::{
			StaleActiveDeadLocalClaims, StaleActiveDeadOwnershipInput,
		},
	},
	state::{ProjectRunStatus, RunActivityMarker, StateStore},
};

pub(super) fn record_recoverable_dead_leased_ownership(
	input: StaleActiveDeadOwnershipInput<'_>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let Some(latest_run) = input.latest_run else {
		return;
	};

	if !(input.run_lease
		&& stale_active_dead_marker_matches_run(input.marker, input.marker_liveness, latest_run))
	{
		return;
	}

	let Ok(local_claims) = stale_active_dead_local_claims_for_run(
		input.project_id,
		input.state_store,
		input.issue_keys,
		latest_run,
	) else {
		blockers.push(String::from("active_shared_claim_unknown"));
		evidence.push(String::from("active_shared_claim_error:dead_local_claim_inspection_failed"));

		return;
	};

	if local_claims.matching_claim_count == 0 {
		return;
	}
	if local_claims.incompatible_claim_present {
		evidence.push(String::from("stale_active_claim_identity_mismatch_present"));

		return;
	}

	blockers.retain(|blocker| blocker != "run_lease_present");
	evidence.push(String::from("stale_run_lease_present"));

	if input.active_shared_claim {
		blockers.retain(|blocker| blocker != "active_shared_claim_present");
		evidence.push(String::from("stale_active_shared_claim_present"));
	}
}

fn stale_active_dead_marker_matches_run(
	marker: Option<&RunActivityMarker>,
	marker_liveness: StaleActiveProcessLiveness,
	run: &ProjectRunStatus,
) -> bool {
	marker_liveness == StaleActiveProcessLiveness::NotAlive
		&& marker.is_some_and(|marker| {
			marker.run_id() == run.run_id() && marker.attempt_number() == run.attempt_number()
		})
}

fn stale_active_dead_local_claims_for_run(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
	run: &ProjectRunStatus,
) -> Result<StaleActiveDeadLocalClaims> {
	let mut claims = StaleActiveDeadLocalClaims::default();

	for issue_key in issue_keys {
		let local_claim_matches =
			state_store.lease_for_issue(issue_key)?.as_ref().is_some_and(|lease| {
				lease.project_id() == project_id && lease.run_id() == run.run_id()
			});
		let active_claim =
			state_store.issue_has_active_shared_claim_read_only(project_id, issue_key)?;
		let external_claim =
			state_store.issue_has_external_shared_claim_read_only(project_id, issue_key)?;

		if local_claim_matches {
			claims.matching_claim_count += 1;
		}
		if external_claim || (active_claim && !local_claim_matches) {
			claims.incompatible_claim_present = true;
		}
	}

	Ok(claims)
}
