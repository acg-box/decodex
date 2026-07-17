use crate::{
	PostgresStore, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};
use serde_json::Value;

// Whole-transaction retry classification is shared with RuntimeSessions through
// exact_commands::is_retryable_exact_database_error; this module owns no second taxonomy.

/// One of the four immutable global RoleProfile identities.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RoleProfileRole {
	/// Global consultation and cross-project recommendation profile.
	Advisor,
	/// Project ownership and serial decision profile.
	Lead,
	/// Execution-scoped implementation profile.
	Task,
	/// Execution-scoped read-only review profile.
	Reviewer,
}
impl RoleProfileRole {
	pub(crate) const fn as_sql(self) -> &'static str {
		match self {
			Self::Advisor => "advisor",
			Self::Lead => "lead",
			Self::Task => "task",
			Self::Reviewer => "reviewer",
		}
	}

	pub(crate) fn from_sql(value: &str) -> Result<Self, StoreError> {
		match value {
			"advisor" => Ok(Self::Advisor),
			"lead" => Ok(Self::Lead),
			"task" => Ok(Self::Task),
			"reviewer" => Ok(Self::Reviewer),
			_ => Err(StoreError::Incompatible("stored RoleProfile role is invalid".into())),
		}
	}
}

/// Exact user-selected configuration for one RoleProfile revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleProfileConfiguration {
	/// Exact model identifier.
	pub model: String,
	/// Exact reasoning-effort identifier.
	pub reasoning_effort: String,
	/// Exact service-tier identifier.
	pub service_tier: String,
	/// Exact instruction bytes represented as UTF-8 text.
	pub instructions: String,
	/// Optional exact user-owned provenance.
	pub provenance: Option<String>,
}

/// Four fixed, role-implied bootstrap configuration groups.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapRoleProfiles {
	/// Advisor revision-one configuration.
	pub advisor: RoleProfileConfiguration,
	/// Lead revision-one configuration.
	pub lead: RoleProfileConfiguration,
	/// Task revision-one configuration.
	pub task: RoleProfileConfiguration,
	/// Reviewer revision-one configuration.
	pub reviewer: RoleProfileConfiguration,
}

/// One immutable RoleProfile revision returned by PostgreSQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleProfileRevision {
	/// Fixed global identity.
	pub role: RoleProfileRole,
	/// Positive immutable revision.
	pub revision: i64,
	/// Exact stored user configuration.
	pub configuration: RoleProfileConfiguration,
}

/// Stable domain rejection returned and replayed by an exact command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoleProfileRejection {
	/// Bootstrap was attempted after the global set already existed.
	AlreadyBootstrapped,
	/// Update was attempted before the global set existed.
	NotBootstrapped,
	/// The supplied expected revision was not current.
	StaleRevision,
	/// A nonpositive expected revision was supplied.
	InvalidExpectedRevision,
	/// One or more exact configuration values violated the stable domain contract.
	InvalidProfile,
}

/// Exact command outcome parsed from PostgreSQL-owned response bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoleProfileCommandOutcome<T> {
	/// The command committed the returned immutable revision evidence.
	Success(T),
	/// The command committed a stable rejection that will replay unchanged.
	Rejected(RoleProfileRejection),
}

impl PostgresStore {
	/// Bootstrap exactly advisor, lead, task, and reviewer in one exact transaction.
	pub async fn bootstrap_role_profiles(
		&self,
		idempotency_key: &str,
		profiles: &BootstrapRoleProfiles,
	) -> Result<RoleProfileCommandOutcome<Vec<RoleProfileRevision>>, StoreError> {
		validate_exact_key(idempotency_key)?;

		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.bootstrap_role_profiles_exact(\
				 $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&profiles.advisor.model,
					&profiles.advisor.reasoning_effort,
					&profiles.advisor.service_tier,
					&profiles.advisor.instructions,
					&profiles.advisor.provenance,
					&profiles.lead.model,
					&profiles.lead.reasoning_effort,
					&profiles.lead.service_tier,
					&profiles.lead.instructions,
					&profiles.lead.provenance,
					&profiles.task.model,
					&profiles.task.reasoning_effort,
					&profiles.task.service_tier,
					&profiles.task.instructions,
					&profiles.task.provenance,
					&profiles.reviewer.model,
					&profiles.reviewer.reasoning_effort,
					&profiles.reviewer.service_tier,
					&profiles.reviewer.instructions,
					&profiles.reviewer.provenance,
				],
			)
			.await?;

		parse_bootstrap_response(&response)
	}

	/// Append one immutable revision and atomically advance its role's current pointer.
	pub async fn update_role_profile(
		&self,
		idempotency_key: &str,
		role: RoleProfileRole,
		expected_revision: i64,
		configuration: &RoleProfileConfiguration,
	) -> Result<RoleProfileCommandOutcome<RoleProfileRevision>, StoreError> {
		validate_exact_key(idempotency_key)?;
		let role = role.as_sql();
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.update_role_profile_exact(\
				 $1,$2,$3::pg_catalog.text::decodex.role_profile_role,$4,$5,$6,$7,$8,$9)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&role,
					&expected_revision,
					&configuration.model,
					&configuration.reasoning_effort,
					&configuration.service_tier,
					&configuration.instructions,
					&configuration.provenance,
				],
			)
			.await?;

		parse_update_response(&response)
	}
}

fn parse_bootstrap_response(
	response: &[u8],
) -> Result<RoleProfileCommandOutcome<Vec<RoleProfileRevision>>, StoreError> {
	let document = response_document(response)?;

	if document.get("classification").and_then(Value::as_str) == Some("stable_domain_rejection") {
		return rejection_from_document(&document).map(RoleProfileCommandOutcome::Rejected);
	}
	if document.get("classification").and_then(Value::as_str) != Some("success") {
		return Err(StoreError::Incompatible(
			"exact RoleProfile response classification is invalid".into(),
		));
	}

	let effects =
		document.pointer("/effect/profiles").and_then(Value::as_array).ok_or_else(|| {
			StoreError::Incompatible("bootstrap response lacks profile effects".into())
		})?;
	let profiles = effects
		.iter()
		.map(|effect| profile_from_value(required_value(effect, "profile")?))
		.collect::<Result<Vec<_>, _>>()?;
	let roles = profiles.iter().map(|profile| profile.role).collect::<Vec<_>>();

	if roles
		!= [
			RoleProfileRole::Advisor,
			RoleProfileRole::Lead,
			RoleProfileRole::Task,
			RoleProfileRole::Reviewer,
		] {
		return Err(StoreError::Incompatible(
			"bootstrap response does not contain the exact ordered RoleProfile set".into(),
		));
	}

	Ok(RoleProfileCommandOutcome::Success(profiles))
}

fn parse_update_response(
	response: &[u8],
) -> Result<RoleProfileCommandOutcome<RoleProfileRevision>, StoreError> {
	let document = response_document(response)?;

	if document.get("classification").and_then(Value::as_str) == Some("stable_domain_rejection") {
		return rejection_from_document(&document).map(RoleProfileCommandOutcome::Rejected);
	}
	if document.get("classification").and_then(Value::as_str) != Some("success") {
		return Err(StoreError::Incompatible(
			"exact RoleProfile response classification is invalid".into(),
		));
	}

	profile_from_value(required_pointer(&document, "/effect/profile")?)
		.map(RoleProfileCommandOutcome::Success)
}

fn response_document(response: &[u8]) -> Result<Value, StoreError> {
	serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("exact RoleProfile response bytes are invalid".into())
	})
}

fn rejection_from_document(document: &Value) -> Result<RoleProfileRejection, StoreError> {
	match document.get("code").and_then(Value::as_str) {
		Some("already_bootstrapped") => Ok(RoleProfileRejection::AlreadyBootstrapped),
		Some("not_bootstrapped") => Ok(RoleProfileRejection::NotBootstrapped),
		Some("stale_revision") => Ok(RoleProfileRejection::StaleRevision),
		Some("invalid_expected_revision") => Ok(RoleProfileRejection::InvalidExpectedRevision),
		Some("invalid_profile") => Ok(RoleProfileRejection::InvalidProfile),
		_ => Err(StoreError::Incompatible("exact RoleProfile rejection code is invalid".into())),
	}
}

fn profile_from_value(value: &Value) -> Result<RoleProfileRevision, StoreError> {
	let role = RoleProfileRole::from_sql(required_str(value, "role")?)?;
	let revision = required_i64(value, "revision")?;

	if revision < 1 {
		return Err(StoreError::Incompatible("stored RoleProfile revision is invalid".into()));
	}

	Ok(RoleProfileRevision {
		role,
		revision,
		configuration: RoleProfileConfiguration {
			model: required_str(value, "model")?.to_owned(),
			reasoning_effort: required_str(value, "reasoning_effort")?.to_owned(),
			service_tier: required_str(value, "service_tier")?.to_owned(),
			instructions: required_str(value, "instructions")?.to_owned(),
			provenance: optional_str(value, "provenance")?,
		},
	})
}

fn required_pointer<'a>(value: &'a Value, pointer: &str) -> Result<&'a Value, StoreError> {
	value.pointer(pointer).ok_or_else(|| {
		StoreError::Incompatible("exact RoleProfile response shape is incomplete".into())
	})
}

fn required_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	value
		.get(key)
		.ok_or_else(|| StoreError::Incompatible("exact RoleProfile effect is incomplete".into()))
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible("stored RoleProfile text is invalid".into()))
}

fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.ok_or_else(|| StoreError::Incompatible("stored RoleProfile revision is invalid".into()))
}

fn optional_str(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => Ok(Some(value.clone())),
		_ => Err(StoreError::Incompatible("stored RoleProfile provenance is invalid".into())),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio_postgres::error::SqlState;

	fn profile(role: &str, revision: i64) -> Value {
		serde_json::json!({
			"role": role,
			"revision": revision,
			"model": "gpt-5.6-sol",
			"reasoning_effort": "medium",
			"service_tier": "priority",
			"instructions": "Own the exact role.",
			"provenance": null,
		})
	}

	#[test]
	fn bootstrap_parser_requires_exact_ordered_roles() {
		let effects = ["advisor", "lead", "task", "reviewer"]
			.into_iter()
			.map(|role| serde_json::json!({"profile": profile(role, 1)}))
			.collect::<Vec<_>>();
		let bytes = serde_json::to_vec(&serde_json::json!({
			"classification": "success",
			"effect": {"profiles": effects},
		}))
		.expect("fixture serializes");

		let RoleProfileCommandOutcome::Success(parsed) =
			parse_bootstrap_response(&bytes).expect("exact response parses")
		else {
			panic!("bootstrap fixture must succeed");
		};
		assert_eq!(parsed.len(), 4);
		assert_eq!(parsed[3].role, RoleProfileRole::Reviewer);
	}

	#[test]
	fn stable_rejection_parser_is_distinct_from_infrastructure_failure() {
		let bytes = br#"{"classification":"stable_domain_rejection","code":"stale_revision","effect":{"changed":false,"code":"stale_revision"}}"#;

		assert_eq!(
			parse_update_response(bytes).expect("stable response parses"),
			RoleProfileCommandOutcome::Rejected(RoleProfileRejection::StaleRevision)
		);
	}

	#[test]
	fn retry_classifier_is_closed_to_accepted_sqlstates() {
		assert!(matches!(SqlState::from_code("40001"), SqlState::T_R_SERIALIZATION_FAILURE));
		assert!(matches!(SqlState::from_code("40P01"), SqlState::T_R_DEADLOCK_DETECTED));
		assert!(!SqlState::from_code("DX001").code().starts_with("08"));
	}
}
