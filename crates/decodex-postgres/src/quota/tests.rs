#![cfg(test)]

use serde_json::Value;

use crate::{
	AccountId, QuotaExclusionMutation, QuotaTimestampMicros, QuotaWindowMutation, StoreError,
	quota::{identity, validation},
};
use decodex_core::{ObservationConfidence, QuotaWindowClass, RemainingPercent};

#[test]
fn authoritative_operation_result_precedes_every_cleanup_outcome() {
	let successful_operation = || Ok::<_, StoreError>(41);
	let failed_operation =
		|| Err::<u8, _>(StoreError::InvalidInput("authoritative operation failed"));
	let successful_cleanup = || Ok::<_, StoreError>(());
	let failed_cleanup = || Err::<(), _>(StoreError::CapacityExhausted("advisory unlock failed"));

	assert_eq!(
		super::operation_result_precedes_cleanup(successful_operation(), successful_cleanup())
			.unwrap(),
		41
	);
	assert_eq!(
		super::operation_result_precedes_cleanup(successful_operation(), failed_cleanup()).unwrap(),
		41
	);
	assert!(matches!(
		super::operation_result_precedes_cleanup(failed_operation(), successful_cleanup()),
		Err(StoreError::InvalidInput("authoritative operation failed"))
	));
	assert!(matches!(
		super::operation_result_precedes_cleanup(failed_operation(), failed_cleanup()),
		Err(StoreError::InvalidInput("authoritative operation failed"))
	));
}

fn timestamp(value: i64) -> QuotaTimestampMicros {
	QuotaTimestampMicros::new(value).expect("test timestamp is valid")
}

fn mutation(metadata: Value) -> QuotaWindowMutation {
	QuotaWindowMutation {
		account_id: AccountId::new("10000000-0000-4000-8000-000000000001")
			.expect("test account identity is valid"),
		window: QuotaWindowClass::FiveHour,
		remaining_percent: Some(RemainingPercent::new(0).expect("zero is valid")),
		resets_at: Some(timestamp(1_800_000_000_000_000)),
		observed_at: timestamp(1_700_000_000_000_000),
		confidence: ObservationConfidence::High,
		metadata,
		expected_revision: Some(4),
	}
}

#[test]
fn exact_rfc3339_ingress_normalizes_offsets_without_quantization() {
	let utc = super::parse_quota_timestamp_rfc3339("2026-07-16T10:30:00.123456Z").unwrap();
	let offset = super::parse_quota_timestamp_rfc3339("2026-07-16T06:30:00.123456-04:00").unwrap();
	let lowercase = super::parse_quota_timestamp_rfc3339("2026-07-16t10:30:00.123456z").unwrap();

	assert_eq!(utc, offset);
	assert_eq!(utc, lowercase);
	assert_eq!(QuotaTimestampMicros::new(0).unwrap().get(), 0);
	assert_eq!(super::parse_quota_timestamp_rfc3339("1970-01-01T00:00:00Z").unwrap().get(), 0);
	assert_eq!(
		QuotaTimestampMicros::new(QuotaTimestampMicros::MAX).unwrap().get(),
		QuotaTimestampMicros::MAX
	);
	assert_eq!(
		super::parse_quota_timestamp_rfc3339("9999-12-31T23:59:59.999999Z").unwrap().get(),
		QuotaTimestampMicros::MAX
	);

	for invalid in [
		"2026-07-16T10:30:00.1234567Z",
		"9999-12-31T23:59:59.9999995Z",
		"9999-12-31T23:59:59.999999-00:01",
		"1969-12-31T23:59:59.999999Z",
		"1970-01-01T00:00:00+00:01",
		"10000-01-01T00:00:00Z",
		"2026-12-31T23:59:60Z",
		"infinity",
		"2026-07-16 10:30:00Z",
	] {
		assert!(super::parse_quota_timestamp_rfc3339(invalid).is_err(), "{invalid}");
	}

	assert!(QuotaTimestampMicros::new(-1).is_err());
	assert!(QuotaTimestampMicros::new(QuotaTimestampMicros::MAX + 1).is_err());
}

#[test]
fn freshness_is_exact_in_integer_microseconds() {
	let observed = timestamp(1_700_000_000_000_000);

	for age in [0, 299_999_999, 300_000_000] {
		assert_eq!(
			timestamp(observed.get() + age).checked_duration_since(observed).unwrap(),
			age as u64
		);
	}

	assert_eq!(
		timestamp(observed.get() + 300_000_001).checked_duration_since(observed).unwrap(),
		300_000_001
	);
	assert!(observed.checked_duration_since(timestamp(observed.get() + 1)).is_err());

	for age in [0, 299_999_999, 300_000_000] {
		let exclusion = QuotaExclusionMutation {
			observation: mutation(serde_json::json!({})),
			excluded_at: timestamp(observed.get() + age),
		};

		assert!(validation::validate_exclusion(&exclusion).is_ok(), "age {age}");
	}

	let stale = QuotaExclusionMutation {
		observation: mutation(serde_json::json!({})),
		excluded_at: timestamp(observed.get() + 300_000_001),
	};

	assert!(matches!(validation::validate_exclusion(&stale), Err(StoreError::InvalidInput(_))));
	assert_eq!(
		timestamp(QuotaTimestampMicros::MAX).checked_duration_since(timestamp(0)).unwrap(),
		QuotaTimestampMicros::MAX as u64
	);
}

#[test]
fn canonical_identity_recursively_sorts_objects_and_preserves_value_distinctions() {
	let first = mutation(
		serde_json::from_str(r#"{"z":{"beta":2,"alpha":1},"array":[{"d":4,"c":3},true]}"#).unwrap(),
	);
	let reordered = mutation(
		serde_json::from_str(r#"{"array":[{"c":3,"d":4},true],"z":{"alpha":1,"beta":2}}"#).unwrap(),
	);
	let first_identity = identity::quota_window_mutation_identity(&first).unwrap();
	let reordered_identity = identity::quota_window_mutation_identity(&reordered).unwrap();
	let canonical = String::from_utf8(
		identity::canonical_mutation_bytes(&identity::quota_window_mutation_document(&first))
			.unwrap(),
	)
	.unwrap();

	assert_eq!(first_identity.sha256, reordered_identity.sha256);
	assert_eq!(first_identity.length, reordered_identity.length);
	assert_eq!(
		first_identity.sha256,
		"02a31a80e82869eabd247007781a5eb5fadb2c0d259ef04f293bf8c3b69ace15"
	);
	assert_eq!(first_identity.length, 351);
	assert_eq!(
		canonical,
		r#"{"account_id":"10000000-0000-4000-8000-000000000001","confidence":"high","duration_minutes":300,"expected_revision":4,"metadata":{"array":[{"c":3,"d":4},true],"z":{"alpha":1,"beta":2}},"observed_at_micros":1700000000000000,"remaining_percent":0,"resets_at_micros":1800000000000000,"schema":"decodex/quota-window-mutation/2","window_class":"five_hour"}"#
	);

	for metadata in [
		serde_json::json!({"array": [true, {"c": 3, "d": 4}], "z": {"alpha": 1, "beta": 2}}),
		serde_json::json!({"array": [{"c": "3", "d": 4}, true], "z": {"alpha": 1, "beta": 2}}),
		serde_json::json!({"array": [{"c": 3.0, "d": 4}, true], "z": {"alpha": 1, "beta": 2}}),
	] {
		assert_ne!(
			identity::quota_window_mutation_identity(&mutation(metadata)).unwrap().sha256,
			first_identity.sha256
		);
	}
}

#[test]
fn canonical_identity_changes_for_every_authoritative_field() {
	let first = mutation(
		serde_json::from_str(r#"{"z":{"beta":2,"alpha":1},"array":[{"d":4,"c":3},true]}"#)
			.expect("canonical identity fixture is valid JSON"),
	);
	let first_identity = identity::quota_window_mutation_identity(&first)
		.expect("canonical identity fixture is valid");
	let original = first_identity.sha256;
	let mut variants = Vec::new();
	let mut changed = first.clone();

	changed.account_id = AccountId::new("10000000-0000-4000-8000-000000000002").unwrap();

	variants.push(changed);

	let mut changed = first.clone();

	changed.window = QuotaWindowClass::SevenDay;

	variants.push(changed);

	let mut changed = first.clone();

	changed.remaining_percent = Some(RemainingPercent::new(1).unwrap());

	variants.push(changed);

	let mut changed = first.clone();

	changed.resets_at = Some(timestamp(1_800_000_000_000_001));

	variants.push(changed);

	let mut changed = first.clone();

	changed.observed_at = timestamp(1_700_000_000_000_001);

	variants.push(changed);

	let mut changed = first.clone();

	changed.confidence = ObservationConfidence::Low;

	variants.push(changed);

	let mut changed = first.clone();

	changed.metadata = serde_json::json!({"changed": true});

	variants.push(changed);

	let mut changed = first.clone();

	changed.expected_revision = Some(5);

	variants.push(changed);

	let exclusion = QuotaExclusionMutation {
		observation: first,
		excluded_at: timestamp(1_700_000_300_000_000),
	};
	let exclusion_identity = identity::quota_exclusion_mutation_identity(&exclusion).unwrap();

	for changed in variants {
		assert_ne!(identity::quota_window_mutation_identity(&changed).unwrap().sha256, original);
		assert_ne!(
			identity::quota_exclusion_mutation_identity(&QuotaExclusionMutation {
				observation: changed,
				excluded_at: exclusion.excluded_at,
			})
			.unwrap()
			.sha256,
			exclusion_identity.sha256
		);
	}

	assert_eq!(
		exclusion_identity.sha256,
		"01aa6017334f2e612bb3988a0868aadf915f16cb57ff668e9ae448c0c2f210f4"
	);
	assert_eq!(exclusion_identity.length, 482);

	let mut changed_exclusion = exclusion;

	changed_exclusion.excluded_at = timestamp(1_700_000_299_999_999);

	assert_ne!(
		identity::quota_exclusion_mutation_identity(&changed_exclusion).unwrap().sha256,
		exclusion_identity.sha256
	);
}
