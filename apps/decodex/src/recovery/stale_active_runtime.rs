//! Run and control-channel evidence inspection for stale-active recovery.

use std::collections::HashSet;

use crate::{
	prelude::Result,
	recovery::{
		context::RecoveryRuntimeMutationPolicy, process_liveness::StaleActiveProcessLiveness,
	},
	state::{ProjectRunStatus, RUN_CONTROL_CHANNEL_STATUS_ACTIVE, StateStore},
};

pub(super) fn stale_active_runs(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<Vec<ProjectRunStatus>> {
	let mut runs = if listing_mode.allows_runtime_writes() {
		let mut runs = Vec::new();
		let mut seen_run_ids = HashSet::new();

		for issue_key in issue_keys {
			for run in state_store.list_project_issue_runs(project_id, issue_key)? {
				if seen_run_ids.insert(run.run_id().to_owned()) {
					runs.push(run);
				}
			}
		}

		runs
	} else {
		let (leased_runs, recent_runs) =
			state_store.list_project_runs_read_only(project_id, usize::MAX)?;
		let issue_key_set = issue_keys.iter().map(String::as_str).collect::<HashSet<_>>();

		leased_runs
			.into_iter()
			.chain(recent_runs)
			.filter(|run| issue_key_set.contains(run.issue_id()))
			.collect()
	};

	runs.sort_by(|left, right| {
		left.attempt_number()
			.cmp(&right.attempt_number())
			.then_with(|| left.run_id().cmp(right.run_id()))
	});

	Ok(runs)
}

pub(super) fn latest_stale_active_run(runs: &[ProjectRunStatus]) -> Option<&ProjectRunStatus> {
	runs.iter().max_by(|left, right| {
		left.attempt_number()
			.cmp(&right.attempt_number())
			.then_with(|| left.run_id().cmp(right.run_id()))
	})
}

pub(super) fn inspect_stale_active_run_evidence(
	runs: &[ProjectRunStatus],
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if runs.is_empty() {
		evidence.push(String::from("run_attempt_missing"));
		evidence.push(String::from("protocol_event_evidence_missing"));
		evidence.push(String::from("child_agent_activity_missing"));
		evidence.push(String::from("protocol_activity_missing"));
		evidence.push(String::from("thread_reference_missing"));

		return;
	}

	evidence.push(String::from("run_attempt_present"));

	if runs.iter().any(|run| {
		run.event_count() > 0 || run.last_event_type().is_some() || run.last_event_at().is_some()
	}) {
		if marker_liveness == StaleActiveProcessLiveness::NotAlive {
			evidence.push(String::from("stale_protocol_event_evidence_present"));
		} else {
			blockers.push(String::from("protocol_event_evidence_present"));
		}
	} else {
		evidence.push(String::from("protocol_event_evidence_missing"));
	}
	if runs.iter().any(|run| run.child_agent_activity().is_some()) {
		if marker_liveness == StaleActiveProcessLiveness::NotAlive {
			evidence.push(String::from("stale_child_agent_activity_present"));
		} else {
			blockers.push(String::from("child_agent_activity_present"));
		}
	} else {
		evidence.push(String::from("child_agent_activity_missing"));
	}
	if runs.iter().any(|run| run.protocol_activity().is_some()) {
		if marker_liveness == StaleActiveProcessLiveness::NotAlive {
			evidence.push(String::from("stale_protocol_activity_present"));
		} else {
			blockers.push(String::from("protocol_activity_present"));
		}
	} else {
		evidence.push(String::from("protocol_activity_missing"));
	}
	if runs.iter().any(|run| run.thread_id().is_some() || run.turn_id().is_some()) {
		evidence.push(String::from("stale_thread_reference_present"));
	} else {
		evidence.push(String::from("thread_reference_missing"));
	}
}

pub(super) fn inspect_stale_active_control_channel(
	run: Option<&ProjectRunStatus>,
	runs: &[ProjectRunStatus],
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> String {
	let mut active_channel_present = false;

	for run in runs {
		let Some(channel) = run.control_channel() else {
			continue;
		};

		if channel.status() != RUN_CONTROL_CHANNEL_STATUS_ACTIVE {
			continue;
		}

		match channel.channel_path().try_exists() {
			Ok(true) => active_channel_present = true,
			Ok(false) => {},
			Err(error) => {
				blockers.push(String::from("active_control_channel_unknown"));
				evidence.push(format!("control_channel_status_error:{}", error));
			},
		}
	}

	if active_channel_present {
		if marker_liveness == StaleActiveProcessLiveness::NotAlive {
			evidence.push(String::from("stale_active_control_channel_present"));
		} else {
			blockers.push(String::from("active_control_channel_present"));
		}
	}

	let Some(channel) = run.and_then(ProjectRunStatus::control_channel) else {
		if !active_channel_present {
			evidence.push(String::from("control_channel_missing"));
		}

		return String::from("missing");
	};

	if !active_channel_present {
		evidence.push(String::from("control_channel_inactive_or_file_missing"));
	}

	format!("{}:{}", channel.transport(), channel.status())
}
