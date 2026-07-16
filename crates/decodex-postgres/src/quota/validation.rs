use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{QuotaExclusionMutation, QuotaTimestampMicros, QuotaWindowMutation, StoreError};
use decodex_core::ObservationConfidence;

pub(super) const MAXIMUM_OBSERVATION_AGE_MICROS: u64 = 300_000_000;

/// Parse raw RFC 3339 ingress without rounding or truncation.
pub fn parse_quota_timestamp_rfc3339(value: &str) -> Result<QuotaTimestampMicros, StoreError> {
	if !matches!(value.as_bytes().get(10), Some(b'T' | b't')) {
		return Err(StoreError::InvalidInput("quota timestamp must be exact RFC 3339"));
	}

	let parsed = OffsetDateTime::parse(value, &Rfc3339)
		.map_err(|_| StoreError::InvalidInput("quota timestamp must be exact RFC 3339"))?;
	let nanos = parsed.unix_timestamp_nanos();

	if nanos % 1_000 != 0 {
		return Err(StoreError::InvalidInput(
			"quota timestamp must be exactly microsecond aligned",
		));
	}

	let micros = nanos / 1_000;
	let value = i64::try_from(micros)
		.map_err(|_| StoreError::InvalidInput("quota timestamp is outside the product range"))?;

	QuotaTimestampMicros::new(value)
}

pub(super) fn validate_window(mutation: &QuotaWindowMutation) -> Result<(), StoreError> {
	if mutation.expected_revision.is_some_and(|revision| revision < 1) {
		return Err(StoreError::InvalidInput("expected revision must be positive"));
	}
	if mutation.resets_at.is_some_and(|reset| reset < mutation.observed_at) {
		return Err(StoreError::InvalidInput("quota reset precedes its observation"));
	}

	crate::ensure_credential_negative_json(&mutation.metadata)
}

pub(super) fn validate_exclusion(mutation: &QuotaExclusionMutation) -> Result<(), StoreError> {
	validate_window(&mutation.observation)?;

	let Some(remaining) = mutation.observation.remaining_percent else {
		return Err(StoreError::InvalidInput(
			"quota exclusion requires a known remaining percentage",
		));
	};

	if !remaining.is_depleted() {
		return Err(StoreError::InvalidInput("quota exclusion requires a depleted observation"));
	}
	if mutation.observation.confidence != ObservationConfidence::High {
		return Err(StoreError::InvalidInput("quota exclusion requires high-confidence evidence"));
	}

	let Some(reset) = mutation.observation.resets_at else {
		return Err(StoreError::InvalidInput("quota exclusion requires an exact reset"));
	};

	if reset <= mutation.excluded_at {
		return Err(StoreError::InvalidInput("quota exclusion reset must remain in the future"));
	}

	let age = mutation.excluded_at.checked_duration_since(mutation.observation.observed_at)?;

	if age > MAXIMUM_OBSERVATION_AGE_MICROS {
		return Err(StoreError::InvalidInput(
			"quota exclusion requires an observation no more than 300 seconds old",
		));
	}

	Ok(())
}
