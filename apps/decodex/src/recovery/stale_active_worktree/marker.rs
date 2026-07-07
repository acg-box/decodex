use crate::{
	recovery::process_liveness::{self, StaleActiveProcessLiveness},
	state::RunActivityMarker,
};

pub(in crate::recovery::stale_active_worktree) fn inspect_stale_active_activity_marker(
	marker: Option<&RunActivityMarker>,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if let Some(marker) = marker {
		inspect_process_liveness(marker_liveness, evidence, blockers);
		inspect_marker_progress(marker, marker_liveness, evidence, blockers);
		inspect_marker_protocol_events(marker, marker_liveness, evidence, blockers);
		inspect_marker_activity(marker, marker_liveness, evidence, blockers);
		inspect_marker_child_agent_activity(marker, marker_liveness, evidence, blockers);
		inspect_marker_protocol_summary(marker, marker_liveness, evidence, blockers);
		inspect_marker_thread_status(marker, marker_liveness, evidence, blockers);
	} else {
		evidence.push(String::from("activity_marker_missing"));
	}
}

fn inspect_process_liveness(
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	match marker_liveness {
		StaleActiveProcessLiveness::Alive => blockers.push(String::from("process_alive")),
		StaleActiveProcessLiveness::NotAlive => evidence.push(String::from("process_not_alive")),
		StaleActiveProcessLiveness::Unknown => {
			blockers.push(String::from("process_liveness_unknown"))
		},
	}
}

fn inspect_marker_progress(
	marker: &RunActivityMarker,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if marker.last_progress_unix_epoch().is_some() {
		push_stale_or_live(
			marker_liveness,
			"stale_activity_marker_progress_present",
			"activity_marker_progress_present",
			evidence,
			blockers,
		);
	} else {
		evidence.push(String::from("activity_marker_progress_missing"));
	}
}

fn inspect_marker_protocol_events(
	marker: &RunActivityMarker,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if marker.event_count() > 0 || marker.last_event_type().is_some() {
		push_stale_or_live(
			marker_liveness,
			"stale_protocol_event_marker_present",
			"protocol_event_marker_present",
			evidence,
			blockers,
		);
	} else {
		evidence.push(String::from("protocol_event_marker_missing"));
	}
}

fn inspect_marker_activity(
	marker: &RunActivityMarker,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if marker.last_protocol_activity_unix_epoch().is_some() {
		push_stale_or_live(
			marker_liveness,
			"stale_activity_marker_protocol_activity_present",
			"activity_marker_protocol_activity_present",
			evidence,
			blockers,
		);
	} else {
		evidence.push(String::from("activity_marker_protocol_activity_missing"));
	}
}

fn inspect_marker_child_agent_activity(
	marker: &RunActivityMarker,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if marker.child_agent_activity().is_some() {
		push_stale_or_live(
			marker_liveness,
			"stale_activity_marker_child_agent_activity_present",
			"activity_marker_child_agent_activity_present",
			evidence,
			blockers,
		);
	} else {
		evidence.push(String::from("activity_marker_child_agent_activity_missing"));
	}
}

fn inspect_marker_protocol_summary(
	marker: &RunActivityMarker,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if marker.protocol_activity().is_some() {
		push_stale_or_live(
			marker_liveness,
			"stale_activity_marker_protocol_activity_summary_present",
			"activity_marker_protocol_activity_summary_present",
			evidence,
			blockers,
		);
	} else {
		evidence.push(String::from("activity_marker_protocol_activity_summary_missing"));
	}
}

fn inspect_marker_thread_status(
	marker: &RunActivityMarker,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if process_liveness::stale_active_marker_thread_active(marker) {
		push_stale_or_live(
			marker_liveness,
			"stale_activity_marker_thread_active",
			"activity_marker_thread_active",
			evidence,
			blockers,
		);
	} else {
		evidence.push(String::from("activity_marker_thread_inactive"));
	}
}

fn push_stale_or_live(
	marker_liveness: StaleActiveProcessLiveness,
	stale_evidence: &str,
	live_blocker: &str,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if marker_liveness == StaleActiveProcessLiveness::NotAlive {
		evidence.push(stale_evidence.to_owned());
	} else {
		blockers.push(live_blocker.to_owned());
	}
}
