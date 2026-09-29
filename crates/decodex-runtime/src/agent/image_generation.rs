//! Image-only native quota feedback, separate from conversation capacity and account routing.
use serde_json::Value;

pub(super) fn is_quota_failure(item: &Value) -> bool {
	item["type"] == "imageGeneration"
		&& item["status"] == "failed"
		&& item["failure"]["type"] == "usageLimitExceeded"
		&& item["failure"]["limitId"] == "image_gen"
}

pub(crate) fn quota_detail(item: &Value) -> Option<String> {
	if !is_quota_failure(item) {
		return None;
	}
	let reset = item["failure"]["resetsAt"]
		.as_i64()
		.and_then(|timestamp| time::OffsetDateTime::from_unix_timestamp(timestamp).ok())
		.and_then(|date| date.format(&time::format_description::well_known::Rfc3339).ok());
	Some(match reset {
		Some(reset) =>
			format!("Image generation usage limit reached. Native reset time: {reset} (UTC)."),
		None => "Image generation usage limit reached. Reset time not reported.".into(),
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn image_quota_feedback_preserves_reset_without_classifying_other_failures() {
		let mut item = json!({"id":"image","type":"imageGeneration","status":"failed","failure":{"type":"usageLimitExceeded","limitId":"image_gen","resetsAt":1790683200}});
		assert!(quota_detail(&item).unwrap().contains("2026-09-29T12:00:00Z"));
		for reset in [Value::Null, json!("unknown"), json!(i64::MAX)] {
			item["failure"]["resetsAt"] = reset;
			assert!(quota_detail(&item).unwrap().contains("Reset time not reported"));
		}
		item["failure"]["limitId"] = json!("codex");
		assert!(quota_detail(&item).is_none());
		item["failure"]["limitId"] = json!("image_gen");
		item["status"] = json!("completed");
		assert!(quota_detail(&item).is_none());
		item["status"] = json!("failed");
		item["failure"]["type"] = json!("futureFailure");
		assert!(quota_detail(&item).is_none());
	}
}
