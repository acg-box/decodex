use std::collections::HashMap;

use crate::orchestrator::{
	OperatorHistoryLaneStatus, OperatorIssueDisplayMetadata, OperatorRunStatus,
	OperatorStatusSnapshot,
};

pub(in crate::orchestrator) fn fill_missing_history_lane_issue_metadata(
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

pub(in crate::orchestrator) fn fill_missing_run_issue_metadata(
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

pub(in crate::orchestrator::status::issue_metadata) fn hydrate_operator_snapshot_run_rows(
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
