use serde_json::Value;

use crate::{
	AccountId, HypotheticalFallbackFact, QuotaExclusionMutation, QuotaExclusionReceipt,
	QuotaTimestampMicros, QuotaWindow, QuotaWindowMutation, StoreError,
	quota::identity::CanonicalMutationIdentity,
};
use decodex_core::{ObservationConfidence, QuotaWindowClass, RemainingPercent};

pub(super) fn exclusion_receipt(
	mutation: &QuotaExclusionMutation,
	revision: i64,
	identity: &CanonicalMutationIdentity,
) -> Result<QuotaExclusionReceipt, StoreError> {
	Ok(QuotaExclusionReceipt {
		account_id: mutation.observation.account_id.clone(),
		window: mutation.observation.window,
		observation_revision: revision,
		remaining_percent: mutation
			.observation
			.remaining_percent
			.ok_or(StoreError::InvalidInput("quota exclusion lost remaining evidence"))?,
		resets_at: mutation
			.observation
			.resets_at
			.ok_or(StoreError::InvalidInput("quota exclusion lost reset evidence"))?,
		observed_at: mutation.observation.observed_at,
		excluded_at: mutation.excluded_at,
		confidence: mutation.observation.confidence,
		metadata: mutation.observation.metadata.clone(),
		mutation_sha256: identity.sha256.clone(),
		mutation_length: identity.length,
		hypothetical_fallback: HypotheticalFallbackFact,
	})
}

pub(super) fn window_response(window: &QuotaWindow) -> Value {
	serde_json::json!({
		"kind": "quota_window",
		"account_id": window.account_id.as_str(),
		"window_class": window_class_sql(window.window),
		"duration_minutes": window.window.duration_minutes(),
		"remaining_percent": window.remaining_percent.map(RemainingPercent::get),
		"resets_at_micros": window.resets_at.map(QuotaTimestampMicros::get),
		"observed_at_micros": window.observed_at.get(),
		"confidence": confidence_sql(window.confidence),
		"metadata": window.metadata,
		"revision": window.revision,
	})
}

pub(super) fn exclusion_response(receipt: &QuotaExclusionReceipt) -> Value {
	serde_json::json!({
		"kind": "quota_exclusion",
		"account_id": receipt.account_id.as_str(),
		"window_class": window_class_sql(receipt.window),
		"duration_minutes": receipt.window.duration_minutes(),
		"observation_revision": receipt.observation_revision,
		"remaining_percent": receipt.remaining_percent.get(),
		"resets_at_micros": receipt.resets_at.get(),
		"observed_at_micros": receipt.observed_at.get(),
		"excluded_at_micros": receipt.excluded_at.get(),
		"confidence": confidence_sql(receipt.confidence),
		"metadata": receipt.metadata,
		"mutation_sha256": receipt.mutation_sha256,
		"mutation_length": receipt.mutation_length,
		"dispatch_enabled": receipt.hypothetical_fallback.dispatch_enabled(),
	})
}

pub(super) fn window_from_response(response: Value) -> Result<QuotaWindow, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some("quota_window") {
		return Err(StoreError::IdempotencyConflict);
	}

	Ok(QuotaWindow {
		account_id: stored_account_id(required_str(&response, "account_id")?)?,
		window: window_class_from_sql(required_str(&response, "window_class")?)?,
		remaining_percent: optional_remaining(&response, "remaining_percent")?,
		resets_at: optional_timestamp(&response, "resets_at_micros")?,
		observed_at: required_timestamp(&response, "observed_at_micros")?,
		confidence: confidence_from_sql(required_str(&response, "confidence")?)?,
		metadata: required_value(&response, "metadata")?,
		revision: required_i64(&response, "revision")?,
	})
}

pub(super) fn exclusion_from_response(
	response: Value,
) -> Result<QuotaExclusionReceipt, StoreError> {
	if response.get("kind").and_then(Value::as_str) != Some("quota_exclusion")
		|| response.get("dispatch_enabled").and_then(Value::as_bool) != Some(false)
	{
		return Err(StoreError::IdempotencyConflict);
	}

	Ok(QuotaExclusionReceipt {
		account_id: stored_account_id(required_str(&response, "account_id")?)?,
		window: window_class_from_sql(required_str(&response, "window_class")?)?,
		observation_revision: required_i64(&response, "observation_revision")?,
		remaining_percent: required_remaining(&response, "remaining_percent")?,
		resets_at: required_timestamp(&response, "resets_at_micros")?,
		observed_at: required_timestamp(&response, "observed_at_micros")?,
		excluded_at: required_timestamp(&response, "excluded_at_micros")?,
		confidence: confidence_from_sql(required_str(&response, "confidence")?)?,
		metadata: required_value(&response, "metadata")?,
		mutation_sha256: required_str(&response, "mutation_sha256")?.to_owned(),
		mutation_length: required_i64(&response, "mutation_length")?,
		hypothetical_fallback: HypotheticalFallbackFact,
	})
}

pub(super) fn quota_aggregate_id(mutation: &QuotaWindowMutation) -> String {
	format!("{}:{}", mutation.account_id, window_class_sql(mutation.window))
}

pub(super) const fn window_class_sql(window: QuotaWindowClass) -> &'static str {
	match window {
		QuotaWindowClass::FiveHour => "five_hour",
		QuotaWindowClass::SevenDay => "seven_day",
	}
}

pub(super) const fn confidence_sql(confidence: ObservationConfidence) -> &'static str {
	match confidence {
		ObservationConfidence::Unknown => "unknown",
		ObservationConfidence::Low => "low",
		ObservationConfidence::High => "high",
	}
}

fn window_class_from_sql(value: &str) -> Result<QuotaWindowClass, StoreError> {
	match value {
		"five_hour" => Ok(QuotaWindowClass::FiveHour),
		"seven_day" => Ok(QuotaWindowClass::SevenDay),
		_ => Err(StoreError::Incompatible("stored quota window class is invalid".into())),
	}
}

fn confidence_from_sql(value: &str) -> Result<ObservationConfidence, StoreError> {
	match value {
		"unknown" => Ok(ObservationConfidence::Unknown),
		"low" => Ok(ObservationConfidence::Low),
		"high" => Ok(ObservationConfidence::High),
		_ => Err(StoreError::Incompatible("stored quota confidence is invalid".into())),
	}
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible(format!("quota response missing {key}")))
}

fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.ok_or_else(|| StoreError::Incompatible(format!("quota response missing {key}")))
}

fn required_value(value: &Value, key: &str) -> Result<Value, StoreError> {
	value
		.get(key)
		.cloned()
		.ok_or_else(|| StoreError::Incompatible(format!("quota response missing {key}")))
}

fn required_timestamp(value: &Value, key: &str) -> Result<QuotaTimestampMicros, StoreError> {
	QuotaTimestampMicros::new(required_i64(value, key)?)
		.map_err(|_| StoreError::Incompatible(format!("quota response has invalid {key}")))
}

fn optional_timestamp(
	value: &Value,
	key: &str,
) -> Result<Option<QuotaTimestampMicros>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(value) => value
			.as_i64()
			.ok_or_else(|| StoreError::Incompatible(format!("quota response has invalid {key}")))
			.and_then(|value| {
				QuotaTimestampMicros::new(value).map(Some).map_err(|_| {
					StoreError::Incompatible(format!("quota response has invalid {key}"))
				})
			}),
		None => Err(StoreError::Incompatible(format!("quota response missing {key}"))),
	}
}

fn required_remaining(value: &Value, key: &str) -> Result<RemainingPercent, StoreError> {
	let raw = value
		.get(key)
		.and_then(Value::as_u64)
		.and_then(|value| u16::try_from(value).ok())
		.ok_or_else(|| StoreError::Incompatible(format!("quota response has invalid {key}")))?;

	RemainingPercent::new(raw)
		.map_err(|_| StoreError::Incompatible(format!("quota response has invalid {key}")))
}

fn optional_remaining(value: &Value, key: &str) -> Result<Option<RemainingPercent>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(_) => required_remaining(value, key).map(Some),
		None => Err(StoreError::Incompatible(format!("quota response missing {key}"))),
	}
}

fn stored_account_id(value: impl Into<String>) -> Result<AccountId, StoreError> {
	AccountId::new(value)
		.map_err(|_| StoreError::Incompatible("stored account identity is invalid".into()))
}
