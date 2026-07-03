//! Tracker issue metadata hydration for operator status rows.

use std::{
	collections::{BTreeSet, HashMap, HashSet},
	time::Instant,
};

use crate::{
	commit_message,
	config::ServiceConfig,
	orchestrator::{
		self, OperatorHistoryLaneStatus, OperatorIssueDisplayMetadata, OperatorRunStatus,
		OperatorStatusSnapshot, RunIssueMetadataHydration, TrackerObserverOutcome,
		status_run_projection,
	},
	prelude::Result,
	tracker::{self, IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

pub(super) fn hydrate_operator_run_rows_from_tracker<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	snapshot: &mut OperatorStatusSnapshot,
	hydration: RunIssueMetadataHydration,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> TrackerObserverOutcome
where
	T: IssueTracker,
{
	let issue_ids =
		operator_snapshot_run_issue_ids(snapshot, hydration, stale_terminal_local_issue_ids);

	if issue_ids.is_empty() {
		return TrackerObserverOutcome::Ok;
	}

	match tracker.refresh_issues(&issue_ids) {
		Ok(issues) => {
			let active_label = tracker::automation_active_label(project.service_id());
			let needs_attention_label =
				workflow.frontmatter().tracker().needs_attention_label().to_owned();
			let metadata_by_issue_id = issues
				.into_iter()
				.map(|issue| {
					let active_label_present = issue.has_label(&active_label);
					let needs_attention_label_present = issue.has_label(&needs_attention_label);

					(
						issue.id,
						OperatorIssueDisplayMetadata {
							issue_identifier: issue.identifier,
							title: Some(issue.title),
							author: issue.author,
							issue_state: Some(issue.state.name),
							active_label_present: Some(active_label_present),
							needs_attention_label_present: Some(needs_attention_label_present),
						},
					)
				})
				.collect::<HashMap<_, _>>();

			hydrate_operator_snapshot_run_rows(snapshot, &metadata_by_issue_id);

			if let Err(error) = hydrate_missing_current_lane_tracker_metadata(
				tracker,
				snapshot,
				&active_label,
				&needs_attention_label,
			) {
				if let Some(backoff) = orchestrator::tracker_connector_backoff(
					&error,
					Instant::now(),
					"run_issue_identifier_metadata",
				) {
					return TrackerObserverOutcome::Backoff(backoff);
				}

				let _ = error;

				tracing::warn!(
					project_id = project.service_id(),
					"Skipped missing-issue current lane classification; sensitive tracker details were withheld."
				);

				return TrackerObserverOutcome::Unavailable;
			}

			TrackerObserverOutcome::Ok
		},
		Err(error)
			if issue_ids.iter().any(|issue_id| {
				tracker::issue_lookup_missing_error_for_candidate(&error, issue_id)
			}) =>
		{
			let active_label = tracker::automation_active_label(project.service_id());
			let needs_attention_label =
				workflow.frontmatter().tracker().needs_attention_label().to_owned();

			if let Err(error) = hydrate_missing_current_lane_tracker_metadata(
				tracker,
				snapshot,
				&active_label,
				&needs_attention_label,
			) {
				if let Some(backoff) = orchestrator::tracker_connector_backoff(
					&error,
					Instant::now(),
					"run_issue_identifier_metadata",
				) {
					return TrackerObserverOutcome::Backoff(backoff);
				}

				let _ = error;

				tracing::warn!(
					project_id = project.service_id(),
					"Skipped missing-issue current lane classification; sensitive tracker details were withheld."
				);

				return TrackerObserverOutcome::Unavailable;
			}

			TrackerObserverOutcome::Ok
		},
		Err(error) => {
			if let Some(backoff) = orchestrator::tracker_connector_backoff(
				&error,
				Instant::now(),
				"run_issue_metadata",
			) {
				return TrackerObserverOutcome::Backoff(backoff);
			}

			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Skipped tracker issue metadata hydration for operator run rows; sensitive tracker details were withheld."
			);

			TrackerObserverOutcome::Unavailable
		},
	}
}

pub(super) fn operator_run_is_stale_terminal_local_residue(
	run: &OperatorRunStatus,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> bool {
	operator_run_is_terminal_unleased_identifier(run)
		&& stale_terminal_local_issue_ids.contains(run.issue_id.trim())
}

pub(super) fn operator_run_tracker_issue_identifier_selector(
	run: &OperatorRunStatus,
) -> Option<String> {
	run.issue_identifier
		.as_ref()
		.filter(|identifier| commit_message::looks_like_issue_identifier(identifier))
		.map(|identifier| identifier.to_ascii_uppercase())
		.or_else(|| {
			status_run_projection::operator_run_issue_identifier_from_fields(
				&run.run_id,
				run.branch_name.as_deref(),
				run.worktree_path.as_deref(),
			)
		})
		.or_else(|| {
			commit_message::looks_like_issue_identifier(&run.issue_id)
				.then(|| run.issue_id.to_ascii_uppercase())
		})
}

pub(super) fn fill_missing_history_lane_issue_metadata(
	lane: &mut OperatorHistoryLaneStatus,
	metadata: &OperatorIssueDisplayMetadata,
) {
	if lane.issue_identifier.as_ref().is_none_or(|identifier| identifier.trim().is_empty())
		&& !metadata.issue_identifier.trim().is_empty()
	{
		lane.issue_identifier = Some(metadata.issue_identifier.clone());
		lane.issue_key = metadata.issue_identifier.clone();
	}
	if lane.title.as_ref().is_none_or(|title| title.trim().is_empty())
		&& let Some(title) = metadata.title.as_ref().filter(|title| !title.trim().is_empty())
	{
		lane.title = Some(title.clone());
	}
	if lane.author.as_ref().is_none_or(|author| author.trim().is_empty())
		&& let Some(author) = metadata.author.as_ref().filter(|author| !author.trim().is_empty())
	{
		lane.author = Some(author.clone());
	}
}

pub(super) fn fill_missing_run_issue_metadata(
	run: &mut OperatorRunStatus,
	metadata: &OperatorIssueDisplayMetadata,
) {
	if run.issue_identifier.as_ref().is_none_or(|identifier| identifier.trim().is_empty())
		&& !metadata.issue_identifier.trim().is_empty()
	{
		run.issue_identifier = Some(metadata.issue_identifier.clone());
	}
	if run.title.as_ref().is_none_or(|title| title.trim().is_empty())
		&& let Some(title) = metadata.title.as_ref().filter(|title| !title.trim().is_empty())
	{
		run.title = Some(title.clone());
	}
	if run.author.as_ref().is_none_or(|author| author.trim().is_empty())
		&& let Some(author) = metadata.author.as_ref().filter(|author| !author.trim().is_empty())
	{
		run.author = Some(author.clone());
	}
	if run.issue_state.as_ref().is_none_or(|issue_state| issue_state.trim().is_empty())
		&& let Some(issue_state) =
			metadata.issue_state.as_ref().filter(|issue_state| !issue_state.trim().is_empty())
	{
		run.issue_state = Some(issue_state.clone());
	}
	if run.active_label_present.is_none() {
		run.active_label_present = metadata.active_label_present;
	}
	if run.needs_attention_label_present.is_none() {
		run.needs_attention_label_present = metadata.needs_attention_label_present;
	}
}

fn operator_snapshot_run_issue_ids(
	snapshot: &OperatorStatusSnapshot,
	hydration: RunIssueMetadataHydration,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> Vec<String> {
	let mut issue_ids = BTreeSet::new();

	for run in &snapshot.current_lanes {
		append_operator_run_issue_id(&mut issue_ids, run, stale_terminal_local_issue_ids);
	}

	if matches!(hydration, RunIssueMetadataHydration::AllRows) {
		for run in &snapshot.recent_runs {
			append_operator_run_issue_id(&mut issue_ids, run, stale_terminal_local_issue_ids);
		}
		for lane in &snapshot.history_lanes {
			append_operator_run_issue_id(
				&mut issue_ids,
				&lane.latest_run,
				stale_terminal_local_issue_ids,
			);

			for attempt in &lane.attempts {
				append_operator_run_issue_id(
					&mut issue_ids,
					attempt,
					stale_terminal_local_issue_ids,
				);
			}
		}
	}

	issue_ids.into_iter().collect()
}

fn append_operator_run_issue_id(
	issue_ids: &mut BTreeSet<String>,
	run: &OperatorRunStatus,
	stale_terminal_local_issue_ids: &HashSet<String>,
) {
	if operator_run_is_stale_terminal_local_residue(run, stale_terminal_local_issue_ids) {
		return;
	}

	let issue_id = run.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		issue_ids.insert(issue_id.to_owned());
	}
}

fn operator_run_is_terminal_unleased_identifier(run: &OperatorRunStatus) -> bool {
	!run.run_lease
		&& orchestrator::looks_like_tracker_issue_identifier_key(&run.issue_id)
		&& orchestrator::local_run_attempt_status_is_terminal(&run.attempt_status)
}

fn hydrate_operator_snapshot_run_rows(
	snapshot: &mut OperatorStatusSnapshot,
	metadata_by_issue_id: &HashMap<String, OperatorIssueDisplayMetadata>,
) {
	for run in snapshot.current_lanes.iter_mut().chain(snapshot.recent_runs.iter_mut()) {
		hydrate_operator_run_row_from_issue_metadata(run, metadata_by_issue_id);
	}
	for lane in &mut snapshot.history_lanes {
		hydrate_history_lane_from_issue_metadata(lane, metadata_by_issue_id);
	}
}

fn hydrate_missing_current_lane_tracker_metadata<T>(
	tracker: &T,
	snapshot: &mut OperatorStatusSnapshot,
	active_label: &str,
	needs_attention_label: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	let mut metadata_by_issue_id = HashMap::new();
	let mut missing_rows = Vec::new();

	for run in &snapshot.current_lanes {
		if run.issue_state.is_some() {
			continue;
		}

		let Some(selector) = operator_run_tracker_issue_identifier_selector(run) else {
			continue;
		};

		match tracker.get_issue_by_identifier(&selector) {
			Ok(Some(issue)) => {
				metadata_by_issue_id.insert(
					run.issue_id.clone(),
					operator_issue_display_metadata(&issue, active_label, needs_attention_label),
				);
			},
			Ok(None) => missing_rows.push((run.run_id.clone(), run.issue_id.clone(), selector)),
			Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, &selector) => {
				missing_rows.push((run.run_id.clone(), run.issue_id.clone(), selector));
			},
			Err(error) => return Err(error),
		}
	}

	if !metadata_by_issue_id.is_empty() {
		hydrate_operator_snapshot_run_rows(snapshot, &metadata_by_issue_id);
	}

	for (run_id, issue_id, selector) in missing_rows {
		orchestrator::mark_operator_run_tracker_issue_missing(
			snapshot, &run_id, &issue_id, &selector,
		);
	}

	Ok(())
}

fn operator_issue_display_metadata(
	issue: &TrackerIssue,
	active_label: &str,
	needs_attention_label: &str,
) -> OperatorIssueDisplayMetadata {
	OperatorIssueDisplayMetadata {
		issue_identifier: issue.identifier.clone(),
		title: Some(issue.title.clone()),
		author: issue.author.clone(),
		issue_state: Some(issue.state.name.clone()),
		active_label_present: Some(issue.has_label(active_label)),
		needs_attention_label_present: Some(issue.has_label(needs_attention_label)),
	}
}

fn hydrate_history_lane_from_issue_metadata(
	lane: &mut OperatorHistoryLaneStatus,
	metadata_by_issue_id: &HashMap<String, OperatorIssueDisplayMetadata>,
) {
	if let Some(metadata) = metadata_by_issue_id.get(&lane.issue_id) {
		apply_history_lane_issue_metadata(lane, metadata);
	}

	hydrate_operator_run_row_from_issue_metadata(&mut lane.latest_run, metadata_by_issue_id);

	for attempt in &mut lane.attempts {
		hydrate_operator_run_row_from_issue_metadata(attempt, metadata_by_issue_id);
	}
}

fn hydrate_operator_run_row_from_issue_metadata(
	run: &mut OperatorRunStatus,
	metadata_by_issue_id: &HashMap<String, OperatorIssueDisplayMetadata>,
) {
	if let Some(metadata) = metadata_by_issue_id.get(&run.issue_id) {
		apply_run_issue_metadata(run, metadata);
	}
}

fn apply_history_lane_issue_metadata(
	lane: &mut OperatorHistoryLaneStatus,
	metadata: &OperatorIssueDisplayMetadata,
) {
	if !metadata.issue_identifier.trim().is_empty() {
		lane.issue_identifier = Some(metadata.issue_identifier.clone());
		lane.issue_key = metadata.issue_identifier.clone();
	}

	if let Some(title) = metadata.title.as_ref().filter(|title| !title.trim().is_empty()) {
		lane.title = Some(title.clone());
	}
	if let Some(author) = metadata.author.as_ref().filter(|author| !author.trim().is_empty()) {
		lane.author = Some(author.clone());
	}
	if let Some(issue_state) =
		metadata.issue_state.as_ref().filter(|issue_state| !issue_state.trim().is_empty())
	{
		lane.issue_state = Some(issue_state.clone());
	}
	if let Some(active_label_present) = metadata.active_label_present {
		lane.active_label_present = Some(active_label_present);
	}
	if let Some(needs_attention_label_present) = metadata.needs_attention_label_present {
		lane.needs_attention_label_present = Some(needs_attention_label_present);
	}
}

fn apply_run_issue_metadata(run: &mut OperatorRunStatus, metadata: &OperatorIssueDisplayMetadata) {
	if !metadata.issue_identifier.trim().is_empty() {
		run.issue_identifier = Some(metadata.issue_identifier.clone());
	}

	if let Some(title) = metadata.title.as_ref().filter(|title| !title.trim().is_empty()) {
		run.title = Some(title.clone());
	}
	if let Some(author) = metadata.author.as_ref().filter(|author| !author.trim().is_empty()) {
		run.author = Some(author.clone());
	}
	if let Some(issue_state) =
		metadata.issue_state.as_ref().filter(|issue_state| !issue_state.trim().is_empty())
	{
		run.issue_state = Some(issue_state.clone());
	}
	if let Some(active_label_present) = metadata.active_label_present {
		run.active_label_present = Some(active_label_present);
	}
	if let Some(needs_attention_label_present) = metadata.needs_attention_label_present {
		run.needs_attention_label_present = Some(needs_attention_label_present);
	}
}
