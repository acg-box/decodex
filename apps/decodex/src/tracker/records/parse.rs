use serde::de::DeserializeOwned;

use crate::tracker::{
	TrackerComment,
	records::{self, LinearExecutionEventRecord},
};

pub(crate) fn has_linear_execution_event_record(
	comments: &[TrackerComment],
	service_id: &str,
	issue_id: &str,
	idempotency_key: &str,
) -> bool {
	comments.iter().filter_map(|comment| parse_linear_execution_event_record(&comment.body)).any(
		|record| {
			record.service_id == service_id
				&& record.issue_id == issue_id
				&& record.idempotency_key == idempotency_key
		},
	)
}

pub(crate) fn parse_linear_execution_event_record(
	body: &str,
) -> Option<LinearExecutionEventRecord> {
	parse_structured_comment::<LinearExecutionEventRecord>(body)
		.filter(|record| records::validate_linear_execution_event_record(record).is_ok())
}

fn parse_structured_comment<T>(body: &str) -> Option<T>
where
	T: DeserializeOwned,
{
	extract_structured_json_blocks(body)
		.into_iter()
		.rev()
		.find_map(|payload| serde_json::from_str::<T>(payload).ok())
		.or_else(|| serde_json::from_str::<T>(body.trim()).ok())
}

fn extract_structured_json_blocks(body: &str) -> Vec<&str> {
	body.match_indices("```json")
		.filter_map(|(start, _)| {
			let fenced = &body[start + "```json".len()..];
			let fenced = fenced.strip_prefix("\r\n").or_else(|| fenced.strip_prefix('\n'))?;
			let end = fenced.find("\n```").or_else(|| fenced.find("\r\n```"))?;

			Some(fenced[..end].trim())
		})
		.collect()
}
