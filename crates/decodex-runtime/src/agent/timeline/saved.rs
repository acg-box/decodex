//! Preserve complete stored items when the native mixed timeline is unavailable.
use serde_json::{Value, json};

use super::{
	AgentTimelineContent, AgentTimelineResult, EntityId, ProjectionError, Source, SqliteStore,
	metrics,
};

const PREFIX: &str = "saved:";

pub(super) fn is_cursor(cursor: &str) -> bool {
	cursor.starts_with(PREFIX)
}

pub(super) async fn read(
	store: Option<&SqliteStore>,
	source: &Source,
	cursor: Option<&str>,
) -> Option<AgentTimelineResult> {
	let cursor = match cursor {
		Some(cursor) => {
			let (thread, native): (String, String) =
				serde_json::from_str(cursor.strip_prefix(PREFIX)?).ok()?;
			if thread != source.key.thread {
				return None;
			}
			Some(native)
		},
		None => None,
	};
	for limit in [30, 15, 7, 3, 1] {
		let value = source
			.client
			.thread_saved_history_page(&source.key.thread, cursor.as_deref(), limit)
			.await
			.ok()?;
		let mut items = match project(&value) {
			Ok(items) => items,
			Err(ProjectionError::Capacity) => continue,
			Err(ProjectionError::Malformed) => return None,
		};
		if let Some(store) = store {
			metrics::enrich_contents(store, &source.key.work, &source.key.thread, items.iter_mut())
				.await
				.ok()?;
		}
		let observed_activities = if let Some(store) = store {
			match super::observed::read(store, &source.key.work, &source.key.thread, items.iter())
				.await
			{
				Ok(activities) => activities,
				Err(ProjectionError::Capacity) => continue,
				Err(ProjectionError::Malformed) => return None,
			}
		} else {
			vec![]
		};
		let next_cursor = value["nextCursor"]
			.as_str()
			.map(|cursor| format!("{PREFIX}{}", json!([source.key.thread, cursor])));
		let result = AgentTimelineResult::Saved {
			work_id: EntityId::new(&source.key.work).ok()?,
			account_id: EntityId::new(source.key.account.as_str()).ok()?,
			thread_id: source.key.thread.clone(),
			items,
			observed_activities,
			next_cursor,
		};
		if serde_json::to_vec(&result).ok()?.len() <= 60 * 1024 {
			return Some(result);
		}
	}
	None
}

fn project(value: &Value) -> Result<Vec<AgentTimelineContent>, ProjectionError> {
	let rows = value["data"].as_array().ok_or(ProjectionError::Malformed)?;
	let mut items = Vec::new();
	for (index, row) in rows.iter().enumerate() {
		let mut row = row.clone();
		for field in ["startedAtMs", "completedAtMs"] {
			row["item"][field] = row[field].clone();
		}
		items.push(super::ordinary(&row).ok_or(ProjectionError::Malformed)?);
		if rows.get(index + 1).is_some_and(|next| next["turnId"] == row["turnId"]) {
			continue;
		}
		let turn = row["turnId"].as_str().ok_or(ProjectionError::Malformed)?;
		let header = &value["turns"][turn];
		let status = header["status"].as_str().ok_or(ProjectionError::Malformed)?;
		if status == "inProgress" {
			items.push(
				super::turn_boundary(&json!({"type":"turnStarted", "turn_id":turn,
				"started_at":header["startedAt"]}))
				.ok_or(ProjectionError::Malformed)?,
			);
			continue;
		}
		let boundary = super::turn_boundary(&json!({"type":"turnCompleted", "turn_id":turn,
			"started_at":header["startedAt"],
			"status":status,"duration_ms":header["durationMs"],"error":header["error"]}))
		.ok_or(ProjectionError::Malformed)?;
		items.push(boundary);
	}
	if serde_json::to_vec(&items).map_err(|_| ProjectionError::Malformed)?.len() > 58 * 1024 {
		return Err(ProjectionError::Capacity);
	}
	Ok(items)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn stored_process_keeps_turn_completion_and_duration_without_native_positions() {
		let items = project(&json!({"data":[
			{"turnId":"t","item":{"id":"process","type":"reasoning","summary":["Checking"]}},
			{"turnId":"t","item":{"id":"reply","type":"agentMessage","text":"Done","phase":"final_answer"}}
		],"turns":{"t":{"status":"completed","startedAt":1_800_000_000,"durationMs":1250}}}))
		.unwrap();
		assert_eq!(items.len(), 3);
		assert!(
			matches!(&items[0], AgentTimelineContent::Item {kind,text,..} if kind=="reasoning" && text=="Checking")
		);
		assert!(matches!(
			&items[2],
			AgentTimelineContent::TurnBoundary {
				completed: true,
				started_at_micros: Some(1_800_000_000_000_000),
				duration_ms: Some(1250),
				..
			}
		));
	}
}
