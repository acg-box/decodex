use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::{
	CommandIdentity, QuotaExclusionMutation, QuotaTimestampMicros, QuotaWindowMutation, StoreError,
	accounts::CommandDescriptor,
	quota::{response, validation},
};
use decodex_core::RemainingPercent;

const QUOTA_WINDOW_SCHEMA: &str = "decodex/quota-window-mutation/2";
const QUOTA_EXCLUSION_SCHEMA: &str = "decodex/quota-exclusion-mutation/2";

pub(super) struct CanonicalMutationIdentity {
	pub(super) sha256: String,
	pub(super) length: i64,
}

pub(super) fn quota_window_mutation_identity(
	mutation: &QuotaWindowMutation,
) -> Result<CanonicalMutationIdentity, StoreError> {
	canonical_mutation_identity(&quota_window_mutation_document(mutation))
}

pub(super) fn quota_exclusion_mutation_identity(
	mutation: &QuotaExclusionMutation,
) -> Result<CanonicalMutationIdentity, StoreError> {
	canonical_mutation_identity(&serde_json::json!({
		"schema": QUOTA_EXCLUSION_SCHEMA,
		"observation": quota_window_mutation_document(&mutation.observation),
		"excluded_at_micros": mutation.excluded_at.get(),
		"maximum_age_micros": validation::MAXIMUM_OBSERVATION_AGE_MICROS,
	}))
}

pub(super) fn quota_window_mutation_document(mutation: &QuotaWindowMutation) -> Value {
	serde_json::json!({
		"schema": QUOTA_WINDOW_SCHEMA,
		"account_id": mutation.account_id.as_str(),
		"window_class": response::window_class_sql(mutation.window),
		"duration_minutes": mutation.window.duration_minutes(),
		"remaining_percent": mutation.remaining_percent.map(RemainingPercent::get),
		"resets_at_micros": mutation.resets_at.map(QuotaTimestampMicros::get),
		"observed_at_micros": mutation.observed_at.get(),
		"confidence": response::confidence_sql(mutation.confidence),
		"metadata": mutation.metadata,
		"expected_revision": mutation.expected_revision,
	})
}

pub(super) fn canonical_mutation_identity(
	document: &Value,
) -> Result<CanonicalMutationIdentity, StoreError> {
	let bytes = canonical_mutation_bytes(document)?;
	let length = i64::try_from(bytes.len())
		.map_err(|_| StoreError::InvalidInput("quota mutation is too large"))?;
	let sha256 = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();

	Ok(CanonicalMutationIdentity { sha256, length })
}

pub(super) fn canonical_mutation_bytes(document: &Value) -> Result<Vec<u8>, StoreError> {
	serde_json::to_vec(&canonical_json(document))
		.map_err(|_| StoreError::InvalidInput("quota mutation cannot be serialized"))
}

pub(super) fn canonical_json(value: &Value) -> Value {
	match value {
		Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
		Value::Object(document) => {
			let mut entries = document.iter().collect::<Vec<_>>();

			entries.sort_unstable_by(|(left, _), (right, _)| left.as_bytes().cmp(right.as_bytes()));

			Value::Object(
				entries
					.into_iter()
					.map(|(key, value)| (key.clone(), canonical_json(value)))
					.collect::<Map<_, _>>(),
			)
		},
		Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
	}
}

pub(super) fn derived_command(
	command: &CommandIdentity,
	identity: &CanonicalMutationIdentity,
) -> CommandIdentity {
	CommandIdentity { key: command.key.clone(), request_hash: identity.sha256.clone() }
}

pub(super) fn command_descriptor(
	operation: &'static str,
	mutation: &QuotaWindowMutation,
	identity: &CanonicalMutationIdentity,
) -> CommandDescriptor {
	CommandDescriptor {
		protocol_version: "decodex/store-command/1",
		operation,
		project_scope: "global",
		scope_id: "quota_windows".into(),
		entity_id: response::quota_aggregate_id(mutation),
		expected_revision: mutation.expected_revision,
		payload_hash: Some(identity.sha256.clone()),
		payload_length: Some(identity.length),
	}
}

pub(super) fn exclusion_command_descriptor(
	mutation: &QuotaExclusionMutation,
	identity: &CanonicalMutationIdentity,
) -> CommandDescriptor {
	CommandDescriptor {
		protocol_version: "decodex/store-command/1",
		operation: "persist_quota_exclusion",
		project_scope: "global",
		scope_id: "quota_exclusions".into(),
		entity_id: response::quota_aggregate_id(&mutation.observation),
		expected_revision: mutation.observation.expected_revision,
		payload_hash: Some(identity.sha256.clone()),
		payload_length: Some(identity.length),
	}
}
