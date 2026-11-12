//! Keep observed tool calls when the native store has not recorded their completion.
use std::collections::{BTreeMap, BTreeSet};

use decodex_database::SqliteStore;
use decodex_protocol::{AgentActivityDto, AgentTimelineContent};

use super::ProjectionError;

pub(super) async fn read<'a>(
	store: &SqliteStore,
	work: &str,
	thread: &str,
	contents: impl Iterator<Item = &'a AgentTimelineContent>,
) -> Result<Vec<AgentActivityDto>, ProjectionError> {
	let contents = contents.collect::<Vec<_>>();
	let turns: BTreeSet<_> = contents
		.iter()
		.filter_map(|content| match content {
			AgentTimelineContent::Item { turn_id, .. }
			| AgentTimelineContent::TurnBoundary { turn_id, .. } => Some(turn_id.to_string()),
			_ => None,
		})
		.collect();
	if turns.is_empty() {
		return Ok(vec![]);
	}
	let rows = store
		.read_agent_activity_observations(work.into(), thread.into(), turns.into_iter().collect())
		.await
		.map_err(|_| ProjectionError::Malformed)?;
	if rows.len() > 256 {
		return Err(ProjectionError::Capacity);
	}
	Ok(project(contents.into_iter(), &rows))
}

fn project<'a>(
	contents: impl Iterator<Item = &'a AgentTimelineContent>,
	rows: &[String],
) -> Vec<AgentActivityDto> {
	let mut native = BTreeSet::new();
	let mut completed = BTreeSet::new();
	for content in contents {
		match content {
			AgentTimelineContent::Item { turn_id, item_id, .. } => {
				native.insert((turn_id.as_str(), item_id.as_str()));
			},
			AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } => {
				completed.insert(turn_id.as_str());
			},
			_ => {},
		}
	}
	let mut observed = BTreeMap::new();
	for row in rows {
		let Ok(activity) = serde_json::from_str::<AgentActivityDto>(row) else { continue };
		if matches!(activity.kind.as_str(), "reasoning" | "plan")
			|| native.contains(&(activity.turn_id.as_str(), activity.item_id.as_str()))
		{
			continue;
		}
		observed.insert((activity.turn_id.clone(), activity.item_id.clone()), activity);
	}
	let mut result = observed.into_values().collect::<Vec<_>>();
	for activity in &mut result {
		if completed.contains(activity.turn_id.as_str()) && activity.status == "running" {
			activity.status = "unreported".into();
			activity.detail = "Result not reported".into();
		}
	}
	result.sort_by_key(|activity| activity.native_timestamp_ms);
	result
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn missing_completion_keeps_the_call_without_inventing_success_or_duplicate_steps() {
		let row = |id, status| {
			json!({"turn_id":"turn","item_id":id,"kind":"commandExecution",
			"status":status,"label":"Reading files","detail":"","duration_ms":null})
			.to_string()
		};
		let items = vec![
			AgentTimelineContent::Item {
				turn_id: "turn".into(),
				item_id: "native".into(),
				kind: "commandExecution".into(),
				phase: None,
				text: String::new(),
				truncated: false,
				activity: None,
				attachments: vec![],
				collaboration: None,
			},
			AgentTimelineContent::TurnBoundary {
				turn_id: "turn".into(),
				completed: true,
				status: Some("completed".into()),
				duration_ms: None,
				usage_summary: None,
				usage: None,
				timing: None,
				error: None,
			},
		];
		let activities = project(
			items.iter(),
			&[
				row("missing", "running"),
				row("native", "running"),
				row("finished", "running"),
				row("finished", "completed"),
			],
		);
		assert_eq!(activities.len(), 2);
		assert_eq!(
			activities.iter().find(|a| a.item_id == "missing").unwrap().status,
			"unreported"
		);
		assert_eq!(
			activities.iter().find(|a| a.item_id == "finished").unwrap().status,
			"completed"
		);
	}
}
