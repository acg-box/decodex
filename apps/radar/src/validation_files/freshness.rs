use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	DEFAULT_QUEUE_OUT, DEFAULT_RELEASE_DELTA_OUT, Path, RELEASE_DELTA_SCHEMA,
	UPSTREAM_IMPACT_SCHEMA, UPSTREAM_REVIEW_QUEUE_SCHEMA, UPSTREAM_REVIEW_SCHEMA, Value,
};

const FUTURE_CLOCK_SKEW_MINUTES: i64 = 5;

pub(crate) fn is_default_source_snapshot(path: &Path) -> bool {
	path == Path::new(DEFAULT_QUEUE_OUT) || path == Path::new(DEFAULT_RELEASE_DELTA_OUT)
}

pub(crate) fn validate_source_freshness(
	path: &Path,
	payload: &Value,
	max_age_hours: u64,
	now: OffsetDateTime,
	errors: &mut Vec<String>,
) {
	let Some((field, value)) = source_timestamp(payload) else {
		return;
	};
	let Ok(timestamp) = OffsetDateTime::parse(value, &Rfc3339) else {
		return;
	};
	let Ok(max_age_hours) = i64::try_from(max_age_hours) else {
		errors.push(format!("{}: source freshness limit is too large", path.display()));

		return;
	};
	let future_limit = now + Duration::minutes(FUTURE_CLOCK_SKEW_MINUTES);

	if timestamp > future_limit {
		errors.push(format!(
			"{}: {field} is more than {FUTURE_CLOCK_SKEW_MINUTES} minutes in the future",
			path.display()
		));

		return;
	}
	if now - timestamp > Duration::hours(max_age_hours) {
		errors.push(format!(
			"{}: {field} is older than the {max_age_hours}-hour source freshness limit",
			path.display()
		));
	}
}

fn source_timestamp(payload: &Value) -> Option<(&'static str, &str)> {
	let schema = payload.get("schema").and_then(Value::as_str)?;
	let field = match schema {
		RELEASE_DELTA_SCHEMA | UPSTREAM_REVIEW_QUEUE_SCHEMA => "generated_at",
		UPSTREAM_IMPACT_SCHEMA | UPSTREAM_REVIEW_SCHEMA => "reviewed_at",
		_ => return None,
	};

	payload.get(field).and_then(Value::as_str).map(|value| (field, value))
}
