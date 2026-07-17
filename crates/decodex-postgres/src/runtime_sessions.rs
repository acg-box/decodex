use serde_json::Value;

use crate::{
	PostgresStore, RoleProfileRole, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};
use decodex_core::{
	AccountId, AccountState, ConversationId, RuntimeSessionId, RuntimeSessionState,
};

/// Complete caller-observed, non-secret account facts consumed by one exact creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRuntimeSessionAccountSnapshot {
	/// Caller-selected immutable snapshot identity.
	pub account_snapshot_id: String,
	/// Stable non-secret source account identity.
	pub source_account_id: AccountId,
	/// Exact display label observed at binding time.
	pub display_label: String,
	/// Exact inert account state observed at binding time.
	pub observed_state: AccountState,
	/// Positive source account revision observed at binding time.
	pub source_revision: i64,
}

/// Complete immutable non-secret account snapshot returned by PostgreSQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionAccountSnapshot {
	/// Caller-selected immutable snapshot identity.
	pub account_snapshot_id: String,
	/// Stable non-secret source account identity.
	pub source_account_id: AccountId,
	/// Exact display label observed at binding time.
	pub display_label: String,
	/// Exact inert account state observed at binding time.
	pub observed_state: AccountState,
	/// Positive source account revision observed at binding time.
	pub source_revision: i64,
	/// PostgreSQL-authored immutable creation timestamp.
	pub created_at: String,
}

/// Immutable full RoleProfile revision selected by PostgreSQL at session creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionProfileSnapshot {
	/// PostgreSQL-generated immutable snapshot identity.
	pub profile_snapshot_id: String,
	/// Selected global role identity.
	pub role: RoleProfileRole,
	/// Selected immutable RoleProfile revision.
	pub source_revision: i64,
	/// Exact selected model.
	pub model: String,
	/// Exact selected reasoning effort.
	pub reasoning_effort: String,
	/// Exact selected service tier.
	pub service_tier: String,
	/// Digest of the exact selected instruction bytes.
	pub instructions_digest: String,
	/// Exact selected instruction bytes represented as UTF-8.
	pub instructions: String,
	/// Optional exact selected provenance.
	pub provenance: Option<String>,
	/// PostgreSQL-authored immutable creation timestamp.
	pub created_at: String,
}

/// Typed inputs consumed by the exact RuntimeSession creation command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRuntimeSession {
	/// Caller-selected RuntimeSession identity.
	pub runtime_session_id: RuntimeSessionId,
	/// Existing logical Conversation target.
	pub conversation_id: ConversationId,
	/// Role whose one current immutable revision PostgreSQL must select.
	pub role: RoleProfileRole,
	/// Complete non-secret account snapshot identity and facts.
	pub account_snapshot: CreateRuntimeSessionAccountSnapshot,
	/// Optional canonical Codex thread identity; this does not create a thread.
	pub codex_thread_id: Option<String>,
	/// Initial observed state; PostgreSQL rejects terminal initial states stably.
	pub initial_state: RuntimeSessionState,
}

/// Complete committed RuntimeSession and immutable snapshot readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredRuntimeSession {
	/// Stable RuntimeSession identity.
	pub runtime_session_id: RuntimeSessionId,
	/// Parent logical Conversation identity.
	pub conversation_id: ConversationId,
	/// PostgreSQL-selected full immutable RoleProfile snapshot.
	pub profile_snapshot: RuntimeSessionProfileSnapshot,
	/// Exact immutable non-secret account snapshot.
	pub account_snapshot: RuntimeSessionAccountSnapshot,
	/// Optional immutable Codex thread correlation.
	pub codex_thread_id: Option<String>,
	/// Always null at creation and immutable in this command slice.
	pub last_known_turn_id: Option<String>,
	/// Current persisted lifecycle state.
	pub state: RuntimeSessionState,
	/// Positive optimistic revision.
	pub revision: i64,
	/// PostgreSQL-authored creation timestamp.
	pub created_at: String,
	/// PostgreSQL-authored current revision timestamp.
	pub updated_at: String,
	/// PostgreSQL-authored terminal timestamp.
	pub ended_at: Option<String>,
}

/// Complete canonical effect returned by a successful exact RuntimeSession command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionCommandEffect {
	/// Complete current RuntimeSession and immutable snapshots.
	pub runtime_session: StoredRuntimeSession,
	/// State before the command; null only for creation.
	pub prior_state: Option<RuntimeSessionState>,
	/// State after the command.
	pub new_state: RuntimeSessionState,
	/// Revision before the command; null only for creation.
	pub prior_revision: Option<i64>,
	/// Revision after the command.
	pub new_revision: i64,
	/// Canonical append-only activity identity.
	pub activity_sequence: i64,
	/// Exact activity payload stored by PostgreSQL.
	pub activity_payload: Value,
	/// Canonical outbox identity.
	pub outbox_id: i64,
	/// Exact outbox payload stored by PostgreSQL.
	pub outbox_payload: Value,
}

/// Stable domain rejection committed and replayed by an exact RuntimeSession command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeSessionRejection {
	/// The referenced Conversation, profile, or RuntimeSession does not exist.
	MissingTarget,
	/// The requested RuntimeSession identity already exists.
	DuplicateTarget,
	/// The expected RuntimeSession revision is no longer current.
	StaleRevision,
	/// The requested initial state or state transition is illegal.
	IllegalTransition,
	/// The supplied account snapshot facts are not valid non-secret snapshot facts.
	InvalidAccountSnapshot,
	/// An existing account snapshot identity is bound to different immutable facts.
	AccountSnapshotConflict,
}

/// Parsed exact command result; stable rejections are values, not infrastructure errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeSessionCommandOutcome<T> {
	/// The command committed and returned the authoritative RuntimeSession snapshot.
	Success(T),
	/// The command committed a stable domain rejection.
	Rejected(RuntimeSessionRejection),
}

impl PostgresStore {
	/// Create one RuntimeSession and both snapshots through the command-complete V10 owner.
	pub async fn create_runtime_session(
		&self,
		idempotency_key: &str,
		create: &CreateRuntimeSession,
	) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&create.account_snapshot.account_snapshot_id, "account snapshot identity")?;
		if let Some(thread_id) = &create.codex_thread_id {
			validate_uuid(thread_id, "Codex thread identity")?;
		}

		let role = create.role.as_sql();
		let observed_state = account_state_sql(create.account_snapshot.observed_state);
		let initial_state = session_state_sql(create.initial_state);
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.create_runtime_session_exact(\
				 $1,$2,$3::text::uuid,$4::text::uuid,\
				 $5::text::decodex.role_profile_role,$6::text::uuid,$7::text::uuid,\
				 $8,$9::text::decodex.account_state,$10,$11::text::uuid,\
				 $12::text::decodex.runtime_session_state)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&create.runtime_session_id.as_str(),
					&create.conversation_id.as_str(),
					&role,
					&create.account_snapshot.account_snapshot_id,
					&create.account_snapshot.source_account_id.as_str(),
					&create.account_snapshot.display_label,
					&observed_state,
					&create.account_snapshot.source_revision,
					&create.codex_thread_id,
					&initial_state,
				],
			)
			.await?;

		parse_create_response(&response, create)
	}

	/// Transition one RuntimeSession through the command-complete V10 owner.
	pub async fn transition_runtime_session(
		&self,
		idempotency_key: &str,
		session_id: &RuntimeSessionId,
		expected_revision: i64,
		target_state: RuntimeSessionState,
	) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
		validate_exact_key(idempotency_key)?;
		let target_state_sql = session_state_sql(target_state);
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.transition_runtime_session_exact(\
				 $1,$2,$3::text::uuid,$4,$5::text::decodex.runtime_session_state)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&session_id.as_str(),
					&expected_revision,
					&target_state_sql,
				],
			)
			.await?;

		parse_transition_response(&response, session_id, expected_revision, target_state)
	}
}

enum ResponseContext<'a> {
	Create(&'a CreateRuntimeSession),
	Transition {
		session_id: &'a RuntimeSessionId,
		expected_revision: i64,
		target_state: RuntimeSessionState,
	},
}

fn parse_create_response(
	response: &[u8],
	create: &CreateRuntimeSession,
) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
	parse_response(response, &ResponseContext::Create(create))
}

fn parse_transition_response(
	response: &[u8],
	session_id: &RuntimeSessionId,
	expected_revision: i64,
	target_state: RuntimeSessionState,
) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
	parse_response(
		response,
		&ResponseContext::Transition { session_id, expected_revision, target_state },
	)
}

fn parse_response(
	response: &[u8],
	context: &ResponseContext<'_>,
) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
	let document: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("exact RuntimeSession response bytes are invalid".into())
	})?;

	match document.get("classification").and_then(Value::as_str) {
		Some("stable_domain_rejection") => {
			validate_request_context(required_pointer(&document, "/effect/request")?, context)?;
			return rejection_from_document(&document).map(RuntimeSessionCommandOutcome::Rejected);
		},
		Some("success") => {},
		_ => {
			return Err(StoreError::Incompatible(
				"exact RuntimeSession response classification is invalid".into(),
			));
		},
	}

	let effect = required_pointer(&document, "/effect")?;
	validate_request_context(required_value(effect, "request")?, context)?;
	let session = required_value(effect, "runtime_session")?;
	let profile = profile_from_value(required_value(effect, "profile_snapshot")?)?;
	let account = account_from_value(required_value(effect, "account_snapshot")?)?;
	let state = session_state_from_sql(required_str(session, "state")?)?;
	let revision = required_i64(session, "revision")?;
	let new_state = session_state_from_sql(required_str(effect, "new_state")?)?;
	let new_revision = positive_i64(effect, "new_revision")?;
	let prior_state = optional_state(effect, "prior_state")?;
	let prior_revision = optional_positive_i64(effect, "prior_revision")?;
	let profile_snapshot_id = required_str(session, "profile_snapshot_id")?;
	let account_snapshot_id = required_str(session, "account_snapshot_id")?;
	if revision < 1
		|| new_revision != revision
		|| new_state != state
		|| profile_snapshot_id != profile.profile_snapshot_id
		|| account_snapshot_id != account.account_snapshot_id
		|| match (prior_state, prior_revision, revision) {
			(None, None, 1) => false,
			(Some(_), Some(prior), current) => prior.checked_add(1) != Some(current),
			_ => true,
		} {
		return Err(StoreError::Incompatible(
			"exact RuntimeSession response effect is inconsistent".into(),
		));
	}

	let runtime_session = StoredRuntimeSession {
		runtime_session_id: RuntimeSessionId::new(required_str(session, "runtime_session_id")?)
			.map_err(|_| {
				StoreError::Incompatible("stored RuntimeSession identity is invalid".into())
			})?,
		conversation_id: ConversationId::new(required_str(session, "conversation_id")?).map_err(
			|_| StoreError::Incompatible("stored Conversation identity is invalid".into()),
		)?,
		profile_snapshot: profile,
		account_snapshot: account,
		codex_thread_id: optional_uuid(session, "codex_thread_id")?,
		last_known_turn_id: optional_str(session, "last_known_turn_id")?,
		state,
		revision,
		created_at: required_str(session, "created_at")?.to_owned(),
		updated_at: required_str(session, "updated_at")?.to_owned(),
		ended_at: optional_str(session, "ended_at")?,
	};
	validate_effect_context(&runtime_session, prior_state, prior_revision, context)?;
	let activity_sequence = positive_i64(effect, "activity_sequence")?;
	let outbox_id = positive_i64(effect, "outbox_id")?;
	let activity_payload = required_value(effect, "activity_payload")?.clone();
	let outbox_payload = required_value(effect, "outbox_payload")?.clone();
	let expected_event_kind = match context {
		ResponseContext::Create(_) => "runtime_session_created",
		ResponseContext::Transition { .. } => "runtime_session_transitioned",
	};
	let session_id = runtime_session.runtime_session_id.as_str();
	let activity_transition_matches = match context {
		ResponseContext::Create(_) => true,
		ResponseContext::Transition { expected_revision, target_state, .. } =>
			activity_payload.get("prior_state").and_then(Value::as_str)
				== prior_state.map(session_state_sql)
				&& activity_payload.get("new_state").and_then(Value::as_str)
					== Some(session_state_sql(*target_state))
				&& activity_payload.get("prior_revision").and_then(Value::as_i64)
					== Some(*expected_revision)
				&& activity_payload.get("new_revision").and_then(Value::as_i64)
					== Some(new_revision),
	};
	if !activity_transition_matches
		|| required_str(effect, "activity_aggregate_kind")? != "runtime_session"
		|| required_str(effect, "activity_aggregate_id")? != session_id
		|| required_i64(effect, "activity_revision")? != new_revision
		|| required_str(effect, "activity_event_kind")? != expected_event_kind
		|| required_str(effect, "outbox_effect_key")? != format!("activity/{activity_sequence}")
		|| required_str(effect, "outbox_aggregate_kind")? != "runtime_session"
		|| required_str(effect, "outbox_aggregate_id")? != session_id
		|| required_i64(effect, "outbox_aggregate_revision")? != new_revision
		|| activity_payload.get("runtime_session") != Some(session)
		|| activity_payload.get("profile_snapshot") != effect.get("profile_snapshot")
		|| activity_payload.get("account_snapshot") != effect.get("account_snapshot")
		|| activity_payload.get("kind").and_then(Value::as_str) != Some("runtime_session")
		|| outbox_payload.get("activity_sequence").and_then(Value::as_i64)
			!= Some(activity_sequence)
		|| outbox_payload.get("payload") != Some(&activity_payload)
		|| outbox_payload.get("event_kind").and_then(Value::as_str) != Some(expected_event_kind)
		|| outbox_payload.get("aggregate_kind").and_then(Value::as_str) != Some("runtime_session")
		|| outbox_payload.get("aggregate_id").and_then(Value::as_str) != Some(session_id)
		|| outbox_payload.get("revision").and_then(Value::as_i64) != Some(new_revision)
	{
		return Err(StoreError::Incompatible(
			"exact RuntimeSession audit effect is inconsistent".into(),
		));
	}

	Ok(RuntimeSessionCommandOutcome::Success(RuntimeSessionCommandEffect {
		runtime_session,
		prior_state,
		new_state,
		prior_revision,
		new_revision,
		activity_sequence,
		activity_payload,
		outbox_id,
		outbox_payload,
	}))
}

fn validate_request_context(
	request: &Value,
	context: &ResponseContext<'_>,
) -> Result<(), StoreError> {
	let expected = match context {
		ResponseContext::Create(create) => serde_json::json!({
			"protocol_version": EXACT_COMMAND_PROTOCOL,
			"operation": "create_runtime_session",
			"runtime_session_id": create.runtime_session_id.as_str(),
			"conversation_id": create.conversation_id.as_str(),
			"role": create.role.as_sql(),
			"account_snapshot_id": create.account_snapshot.account_snapshot_id,
			"source_account_id": create.account_snapshot.source_account_id.as_str(),
			"display_label": create.account_snapshot.display_label,
			"observed_state": account_state_sql(create.account_snapshot.observed_state),
			"account_source_revision": create.account_snapshot.source_revision,
			"codex_thread_id": create.codex_thread_id,
			"initial_state": session_state_sql(create.initial_state),
		}),
		ResponseContext::Transition { session_id, expected_revision, target_state } =>
			serde_json::json!({
				"protocol_version": EXACT_COMMAND_PROTOCOL,
				"operation": "transition_runtime_session",
				"runtime_session_id": session_id.as_str(),
				"expected_revision": expected_revision,
				"target_state": session_state_sql(*target_state),
			}),
	};
	if request == &expected {
		Ok(())
	} else {
		Err(StoreError::Incompatible(
			"exact RuntimeSession response belongs to a different request".into(),
		))
	}
}

fn validate_effect_context(
	session: &StoredRuntimeSession,
	prior_state: Option<RuntimeSessionState>,
	prior_revision: Option<i64>,
	context: &ResponseContext<'_>,
) -> Result<(), StoreError> {
	let valid = match context {
		ResponseContext::Create(create) =>
			session.runtime_session_id == create.runtime_session_id
				&& session.conversation_id == create.conversation_id
				&& session.profile_snapshot.role == create.role
				&& session.account_snapshot.account_snapshot_id
					== create.account_snapshot.account_snapshot_id
				&& session.account_snapshot.source_account_id
					== create.account_snapshot.source_account_id
				&& session.account_snapshot.display_label == create.account_snapshot.display_label
				&& session.account_snapshot.observed_state == create.account_snapshot.observed_state
				&& session.account_snapshot.source_revision
					== create.account_snapshot.source_revision
				&& session.codex_thread_id == create.codex_thread_id
				&& session.last_known_turn_id.is_none()
				&& session.state == create.initial_state
				&& session.revision == 1
				&& prior_state.is_none()
				&& prior_revision.is_none(),
		ResponseContext::Transition { session_id, expected_revision, target_state } =>
			session.runtime_session_id.as_str() == session_id.as_str()
				&& session.state == *target_state
				&& prior_revision == Some(*expected_revision)
				&& session.revision == expected_revision.checked_add(1).unwrap_or(i64::MIN)
				&& matches!(
					(prior_state, target_state),
					(Some(RuntimeSessionState::Starting), RuntimeSessionState::Active)
						| (Some(RuntimeSessionState::Starting), RuntimeSessionState::Ended)
						| (Some(RuntimeSessionState::Starting), RuntimeSessionState::Diverged)
						| (Some(RuntimeSessionState::Active), RuntimeSessionState::Ended)
						| (Some(RuntimeSessionState::Active), RuntimeSessionState::Diverged)
				),
	};
	if valid {
		Ok(())
	} else {
		Err(StoreError::Incompatible(
			"exact RuntimeSession response does not match the command request".into(),
		))
	}
}

fn rejection_from_document(document: &Value) -> Result<RuntimeSessionRejection, StoreError> {
	let code = document.get("code").and_then(Value::as_str);
	let effect = required_pointer(document, "/effect")?;
	if effect.get("changed").and_then(Value::as_bool) != Some(false)
		|| effect.get("code").and_then(Value::as_str) != code
	{
		return Err(StoreError::Incompatible(
			"exact RuntimeSession rejection effect is inconsistent".into(),
		));
	}

	match code {
		Some("missing_target") => Ok(RuntimeSessionRejection::MissingTarget),
		Some("duplicate_target") => Ok(RuntimeSessionRejection::DuplicateTarget),
		Some("stale_revision") => Ok(RuntimeSessionRejection::StaleRevision),
		Some("illegal_transition") => Ok(RuntimeSessionRejection::IllegalTransition),
		Some("invalid_account_snapshot") => Ok(RuntimeSessionRejection::InvalidAccountSnapshot),
		Some("account_snapshot_conflict") => Ok(RuntimeSessionRejection::AccountSnapshotConflict),
		_ => Err(StoreError::Incompatible("exact RuntimeSession rejection code is invalid".into())),
	}
}

fn profile_from_value(value: &Value) -> Result<RuntimeSessionProfileSnapshot, StoreError> {
	let source_profile_id = required_str(value, "source_profile_id")?;
	let role = RoleProfileRole::from_sql(required_str(value, "role")?)?;
	if source_profile_id != role.as_sql() {
		return Err(StoreError::Incompatible(
			"stored RoleProfile snapshot identity is inconsistent".into(),
		));
	}

	Ok(RuntimeSessionProfileSnapshot {
		profile_snapshot_id: required_uuid(value, "profile_snapshot_id")?,
		role,
		source_revision: positive_i64(value, "source_revision")?,
		model: required_str(value, "model")?.to_owned(),
		reasoning_effort: required_str(value, "reasoning_effort")?.to_owned(),
		service_tier: required_str(value, "service_tier")?.to_owned(),
		instructions_digest: required_str(value, "instructions_digest")?.to_owned(),
		instructions: required_str(value, "instructions")?.to_owned(),
		provenance: optional_str(value, "provenance")?,
		created_at: required_str(value, "created_at")?.to_owned(),
	})
}

fn account_from_value(value: &Value) -> Result<RuntimeSessionAccountSnapshot, StoreError> {
	Ok(RuntimeSessionAccountSnapshot {
		account_snapshot_id: required_uuid(value, "account_snapshot_id")?,
		source_account_id: AccountId::new(required_str(value, "source_account_id")?)
			.map_err(|_| StoreError::Incompatible("stored account identity is invalid".into()))?,
		display_label: required_str(value, "display_label")?.to_owned(),
		observed_state: account_state_from_sql(required_str(value, "observed_state")?)?,
		source_revision: positive_i64(value, "source_revision")?,
		created_at: required_str(value, "created_at")?.to_owned(),
	})
}

fn required_pointer<'a>(value: &'a Value, pointer: &str) -> Result<&'a Value, StoreError> {
	value.pointer(pointer).ok_or_else(|| {
		StoreError::Incompatible("exact RuntimeSession response shape is incomplete".into())
	})
}

fn required_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	value
		.get(key)
		.ok_or_else(|| StoreError::Incompatible("exact RuntimeSession effect is incomplete".into()))
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible("stored RuntimeSession text is invalid".into()))
}

fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.ok_or_else(|| StoreError::Incompatible("stored RuntimeSession integer is invalid".into()))
}

fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	let result = required_i64(value, key)?;
	if result < 1 {
		return Err(StoreError::Incompatible("stored RuntimeSession revision is invalid".into()));
	}
	Ok(result)
}

fn optional_str(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => Ok(Some(value.clone())),
		_ => Err(StoreError::Incompatible("stored RuntimeSession optional text is invalid".into())),
	}
}

fn optional_uuid(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match optional_str(value, key)? {
		Some(value) if is_uuid(&value) => Ok(Some(value)),
		Some(_) => Err(StoreError::Incompatible("stored RuntimeSession UUID is invalid".into())),
		None => Ok(None),
	}
}

fn optional_state(value: &Value, key: &str) -> Result<Option<RuntimeSessionState>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => session_state_from_sql(value).map(Some),
		_ =>
			Err(StoreError::Incompatible("stored RuntimeSession optional state is invalid".into())),
	}
}

fn optional_positive_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::Number(value)) => {
			let value = value.as_i64().filter(|value| *value > 0).ok_or_else(|| {
				StoreError::Incompatible(
					"stored RuntimeSession optional revision is invalid".into(),
				)
			})?;
			Ok(Some(value))
		},
		_ => Err(StoreError::Incompatible(
			"stored RuntimeSession optional revision is invalid".into(),
		)),
	}
}

fn required_uuid(value: &Value, key: &str) -> Result<String, StoreError> {
	let value = required_str(value, key)?.to_owned();
	if is_uuid(&value) {
		Ok(value)
	} else {
		Err(StoreError::Incompatible("stored RuntimeSession UUID is invalid".into()))
	}
}

fn validate_uuid(value: &str, field: &'static str) -> Result<(), StoreError> {
	if is_uuid(value) { Ok(()) } else { Err(StoreError::InvalidInput(field)) }
}

fn is_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}

const fn session_state_sql(value: RuntimeSessionState) -> &'static str {
	match value {
		RuntimeSessionState::Starting => "starting",
		RuntimeSessionState::Active => "active",
		RuntimeSessionState::Ended => "ended",
		RuntimeSessionState::Diverged => "diverged",
	}
}

fn session_state_from_sql(value: &str) -> Result<RuntimeSessionState, StoreError> {
	match value {
		"starting" => Ok(RuntimeSessionState::Starting),
		"active" => Ok(RuntimeSessionState::Active),
		"ended" => Ok(RuntimeSessionState::Ended),
		"diverged" => Ok(RuntimeSessionState::Diverged),
		_ => Err(StoreError::Incompatible("stored RuntimeSession state is invalid".into())),
	}
}

const fn account_state_sql(value: AccountState) -> &'static str {
	match value {
		AccountState::Unavailable => "unavailable",
		AccountState::Unknown => "unknown",
		AccountState::Available => "available",
		AccountState::Depleted => "depleted",
		AccountState::AuthFailed => "auth_failed",
		AccountState::PluginUnready => "plugin_unready",
		AccountState::Disabled => "disabled",
	}
}

fn account_state_from_sql(value: &str) -> Result<AccountState, StoreError> {
	match value {
		"unavailable" => Ok(AccountState::Unavailable),
		"unknown" => Ok(AccountState::Unknown),
		"available" => Ok(AccountState::Available),
		"depleted" => Ok(AccountState::Depleted),
		"auth_failed" => Ok(AccountState::AuthFailed),
		"plugin_unready" => Ok(AccountState::PluginUnready),
		"disabled" => Ok(AccountState::Disabled),
		_ => Err(StoreError::Incompatible("stored account state is invalid".into())),
	}
}

#[cfg(test)]
mod tests {
	use super::{
		CreateRuntimeSession, CreateRuntimeSessionAccountSnapshot, RuntimeSessionRejection,
		parse_create_response, parse_transition_response,
	};
	use crate::{RoleProfileRole, RuntimeSessionCommandOutcome, StoreError};
	use decodex_core::{
		AccountId, AccountState, ConversationId, RuntimeSessionId, RuntimeSessionState,
	};
	use serde_json::json;

	fn create() -> CreateRuntimeSession {
		CreateRuntimeSession {
			runtime_session_id: RuntimeSessionId::new("41000000-0000-4000-8000-000000000001")
				.unwrap(),
			conversation_id: ConversationId::new("40000000-0000-4000-8000-000000000001").unwrap(),
			role: RoleProfileRole::Task,
			account_snapshot: CreateRuntimeSessionAccountSnapshot {
				account_snapshot_id: "43000000-0000-4000-8000-000000000001".into(),
				source_account_id: AccountId::new("13000000-0000-4000-8000-000000000001").unwrap(),
				display_label: "Account".into(),
				observed_state: AccountState::Unknown,
				source_revision: 1,
			},
			codex_thread_id: None,
			initial_state: RuntimeSessionState::Starting,
		}
	}

	fn create_request() -> serde_json::Value {
		json!({
			"protocol_version": "decodex/exact-command/1",
			"operation": "create_runtime_session",
			"runtime_session_id": "41000000-0000-4000-8000-000000000001",
			"conversation_id": "40000000-0000-4000-8000-000000000001",
			"role": "task",
			"account_snapshot_id": "43000000-0000-4000-8000-000000000001",
			"source_account_id": "13000000-0000-4000-8000-000000000001",
			"display_label": "Account",
			"observed_state": "unknown",
			"account_source_revision": 1,
			"codex_thread_id": null,
			"initial_state": "starting"
		})
	}

	#[test]
	fn stable_rejection_parser_is_closed() {
		let session_id = RuntimeSessionId::new("41000000-0000-4000-8000-000000000001").unwrap();
		let response = json!({
			"classification": "stable_domain_rejection",
			"code": "stale_revision",
			"effect": {
				"changed": false,
				"code": "stale_revision",
				"request": {
					"protocol_version": "decodex/exact-command/1",
					"operation": "transition_runtime_session",
					"runtime_session_id": session_id.as_str(),
					"expected_revision": 1,
					"target_state": "active"
				}
			}
		});
		assert_eq!(
			parse_transition_response(
				&serde_json::to_vec(&response).unwrap(),
				&session_id,
				1,
				RuntimeSessionState::Active,
			)
			.unwrap(),
			RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::StaleRevision),
		);
		assert!(
			parse_transition_response(
				&serde_json::to_vec(&response).unwrap(),
				&session_id,
				2,
				RuntimeSessionState::Active,
			)
			.is_err()
		);
	}

	#[test]
	fn success_parser_requires_complete_immutable_snapshots() {
		let incomplete = br#"{"classification":"success","effect":{"runtime_session":{}}}"#;
		assert!(parse_create_response(incomplete, &create()).is_err());
	}

	#[test]
	fn success_parser_closes_snapshot_and_audit_cross_references() {
		let session = json!({
			"runtime_session_id": "41000000-0000-4000-8000-000000000001",
			"conversation_id": "40000000-0000-4000-8000-000000000001",
			"profile_snapshot_id": "42000000-0000-4000-8000-000000000001",
			"account_snapshot_id": "43000000-0000-4000-8000-000000000001",
			"codex_thread_id": null,
			"last_known_turn_id": null,
			"state": "starting",
			"revision": 1,
			"created_at": "2026-07-17T00:00:00Z",
			"updated_at": "2026-07-17T00:00:00Z",
			"ended_at": null
		});
		let profile = json!({
			"profile_snapshot_id": "42000000-0000-4000-8000-000000000001",
			"source_profile_id": "task",
			"role": "task",
			"source_revision": 1,
			"model": "gpt-5.6-sol",
			"reasoning_effort": "medium",
			"service_tier": "priority",
			"instructions_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
			"instructions": "Own the task.",
			"provenance": null,
			"created_at": "2026-07-17T00:00:00Z"
		});
		let account = json!({
			"account_snapshot_id": "43000000-0000-4000-8000-000000000001",
			"source_account_id": "13000000-0000-4000-8000-000000000001",
			"display_label": "Account",
			"observed_state": "unknown",
			"source_revision": 1,
			"created_at": "2026-07-17T00:00:00Z"
		});
		let activity = json!({
			"kind": "runtime_session",
			"runtime_session": session,
			"profile_snapshot": profile,
			"account_snapshot": account
		});
		let outbox = json!({
			"activity_sequence": 1,
			"event_kind": "runtime_session_created",
			"aggregate_kind": "runtime_session",
			"aggregate_id": "41000000-0000-4000-8000-000000000001",
			"revision": 1,
			"payload": activity
		});
		let response = json!({
			"classification": "success",
			"effect": {
				"request": create_request(),
				"runtime_session": session,
				"profile_snapshot": profile,
				"account_snapshot": account,
				"prior_state": null,
				"new_state": "starting",
				"prior_revision": null,
				"new_revision": 1,
				"activity_sequence": 1,
				"activity_aggregate_kind": "runtime_session",
				"activity_aggregate_id": "41000000-0000-4000-8000-000000000001",
				"activity_revision": 1,
				"activity_event_kind": "runtime_session_created",
				"activity_payload": activity,
				"outbox_id": 1,
				"outbox_effect_key": "activity/1",
				"outbox_aggregate_kind": "runtime_session",
				"outbox_aggregate_id": "41000000-0000-4000-8000-000000000001",
				"outbox_aggregate_revision": 1,
				"outbox_payload": outbox
			}
		});
		let parsed =
			parse_create_response(&serde_json::to_vec(&response).unwrap(), &create()).unwrap();
		let RuntimeSessionCommandOutcome::Success(effect) = parsed else {
			panic!("golden response must parse")
		};
		assert_eq!(effect.new_state, RuntimeSessionState::Starting);
		assert_eq!(effect.activity_sequence, 1);

		let mut substituted = response;
		substituted["effect"]["runtime_session"]["profile_snapshot_id"] =
			json!("42000000-0000-4000-8000-000000000099");
		assert!(
			parse_create_response(&serde_json::to_vec(&substituted).unwrap(), &create()).is_err()
		);
	}

	#[test]
	fn success_parser_rejects_wrong_request_and_malformed_stored_uuid_as_incompatible() {
		let incomplete = json!({
			"classification": "success",
			"effect": {"request": create_request(), "runtime_session": {}}
		});
		let mut wrong = create();
		wrong.account_snapshot.display_label = "Other account".into();
		assert!(matches!(
			parse_create_response(&serde_json::to_vec(&incomplete).unwrap(), &wrong),
			Err(StoreError::Incompatible(_))
		));

		let malformed = json!({
			"profile_snapshot_id": "not-a-uuid",
			"source_profile_id": "task",
			"role": "task",
			"source_revision": 1,
			"model": "m",
			"reasoning_effort": "medium",
			"service_tier": "priority",
			"instructions_digest": "d",
			"instructions": "i",
			"provenance": null,
			"created_at": "t"
		});
		let malformed_response = json!({
			"classification": "success",
			"effect": {
				"request": create_request(),
				"runtime_session": {},
				"profile_snapshot": malformed,
				"account_snapshot": {}
			}
		});
		assert!(matches!(
			parse_create_response(&serde_json::to_vec(&malformed_response).unwrap(), &create()),
			Err(StoreError::Incompatible(_))
		));
	}
}
