//! Image-only native quota feedback, separate from conversation capacity and account routing.
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

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
		.and_then(|timestamp| OffsetDateTime::from_unix_timestamp(timestamp).ok())
		.and_then(|date| date.format(&Rfc3339).ok());

	Some(match reset {
		Some(reset) =>
			format!("Image generation usage limit reached. Native reset time: {reset} (UTC)."),
		None => "Image generation usage limit reached. Reset time not reported.".into(),
	})
}

#[cfg(test)]
mod tests {

	use crate::agent::image_generation::{self, Value};

	#[test]
	fn image_quota_feedback_preserves_reset_without_classifying_other_failures() {
		let mut item = serde_json::json!({"id":"image","type":"imageGeneration","status":"failed","failure":{"type":"usageLimitExceeded","limitId":"image_gen","resetsAt":1_790_683_200}});

		assert!(image_generation::quota_detail(&item).unwrap().contains("2026-09-29T12:00:00Z"));

		for reset in [Value::Null, serde_json::json!("unknown"), serde_json::json!(i64::MAX)] {
			item["failure"]["resetsAt"] = reset;

			assert!(
				image_generation::quota_detail(&item).unwrap().contains("Reset time not reported")
			);
		}

		item["failure"]["limitId"] = serde_json::json!("codex");

		assert!(image_generation::quota_detail(&item).is_none());

		item["failure"]["limitId"] = serde_json::json!("image_gen");
		item["status"] = serde_json::json!("completed");

		assert!(image_generation::quota_detail(&item).is_none());

		item["status"] = serde_json::json!("failed");
		item["failure"]["type"] = serde_json::json!("futureFailure");

		assert!(image_generation::quota_detail(&item).is_none());
	}
}
