use decodex_core::{
	CodexExperimentCommandOutcome, CodexExperimentCreationPossible, CodexExperimentIdentity,
	CodexExperimentObservation, CodexExperimentObservationKind, CodexExperimentPrepared,
	CodexExperimentRejection, CodexExperimentThreadBinding,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	PostgresStore, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};

/// Preparation input. PostgreSQL rechecks the complete V14 lineage and owns the clock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareCodexExperiment {
	/// Complete immutable experiment identity whose V14 lineage PostgreSQL must verify.
	pub identity: CodexExperimentIdentity,
}

/// Exact typed successful app-server creation response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindCodexExperimentThread {
	/// Immutable experiment identity previously advanced to the pre-effect fence.
	pub experiment_id: String,
	/// Required current experiment revision, fixed at the creation-possible revision two.
	pub expected_revision: i64,
	/// Exact creation-attempt identity durably fenced before the external effect.
	pub attempt_id: String,
	/// Exact thread identity returned by the successful typed app-server response.
	pub thread_id: String,
	/// Opaque identity of that exact successful app-server response.
	pub response_id: String,
	/// Response title that must equal the immutable prepared title, including its derived marker.
	pub response_title: String,
	/// Response working directory that must equal the immutable prepared repository path.
	pub response_cwd: String,
	/// Response marker that must equal the marker deterministically derived from the experiment identity.
	pub response_marker: String,
	/// App-server ephemeral flag, which V15 requires to be false for a durable binding.
	pub ephemeral: bool,
}

/// One exact positive observation. There is no negative observation variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordCodexExperimentObservation {
	/// Immutable identity of the experiment that owns the observation.
	pub experiment_id: String,
	/// Required bound-thread revision, fixed at revision three.
	pub expected_revision: i64,
	/// Globally unique identity for this append-only observation record.
	pub observation_id: String,
	/// Closed positive observation kind; no negative or absence kind exists.
	pub kind: CodexExperimentObservationKind,
	/// Exact observed thread, which must equal the experiment's unique bound thread.
	pub thread_id: String,
	/// Retained marker that must equal the value PostgreSQL derived from the experiment identity.
	pub marker: String,
	/// Opaque identity of the exact app-server item or event that supplied the fact.
	pub source_id: String,
	/// Lowercase SHA-256 digest of the positive fact payload at the observation boundary.
	pub fact_digest: String,
}

/// One-shot permission emitted only after this adapter verifies a freshly committed pre-effect
/// fence. A similarly shaped core value alone does not prove PostgreSQL provenance.
#[derive(Debug, Eq, PartialEq)]
pub struct FreshCodexExperimentCreation {
	fact: CodexExperimentCreationPossible,
}
impl FreshCodexExperimentCreation {
	/// Returns the experiment identity whose fresh pre-effect fence committed.
	pub fn experiment_id(&self) -> &str {
		&self.fact.experiment_id
	}

	/// Returns the committed creation-possible revision, fixed at two.
	pub fn revision(&self) -> i64 {
		self.fact.revision
	}

	/// Returns the sole creation-attempt identity bound by the fence.
	pub fn attempt_id(&self) -> &str {
		&self.fact.attempt_id
	}

	/// Returns the PostgreSQL-authored fence time as nonnegative Unix microseconds.
	pub fn fenced_at_micros(&self) -> i64 {
		self.fact.fenced_at_micros
	}
}

/// Pre-effect command outcome. Durable successful replay is ambiguity readback, never permission.
#[derive(Debug, Eq, PartialEq)]
pub enum CodexExperimentCreationFenceOutcome {
	/// Adapter-verified newly committed fence carrying one-shot permission for the possible effect.
	Fresh(FreshCodexExperimentCreation),
	/// Durable replay readback that is terminally ambiguous and never permits retry or adoption.
	ReplayedAmbiguous {
		/// Experiment identity retained by the completed fence response.
		experiment_id: String,
		/// Creation-possible revision retained by the response, fixed at two.
		revision: i64,
		/// Sole fenced attempt identity retained for causal audit.
		attempt_id: String,
	},
	/// Stable PostgreSQL-authored rejection of the requested state transition.
	Rejected(CodexExperimentRejection),
}

impl PostgresStore {
	/// Persist exact immutable experiment intent before creation can become possible.
	pub async fn prepare_codex_experiment(
		&self,
		idempotency_key: &str,
		request: &PrepareCodexExperiment,
	) -> Result<CodexExperimentCommandOutcome<CodexExperimentPrepared>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_identity(&request.identity)?;
		let identity = &request.identity;
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.prepare_codex_experiment_exact($1,$2,$3::text::uuid,\
				 $4::text::uuid,$5,$6::text::uuid,$7::text::uuid,$8,$9,$10,$11,$12)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&identity.experiment_id,
					&identity.managed_run_id.as_str(),
					&identity.managed_run_revision,
					&identity.routing_snapshot_id,
					&identity.account_id.as_str(),
					&identity.account_revision,
					&identity.role_profile_revision,
					&identity.build_id,
					&identity.repository_cwd,
					&identity.thread_title,
				],
			)
			.await?;
		let (classification, effect) = parse_envelope(&response, "prepare_codex_experiment")?;
		if classification == "stable_domain_rejection" {
			return Ok(CodexExperimentCommandOutcome::Rejected(parse_rejection(&effect)?));
		}
		require_keys(
			&effect,
			&[
				"account_id",
				"account_revision",
				"build_id",
				"effect_digest",
				"effect_digest_source",
				"experiment_id",
				"managed_run_id",
				"managed_run_revision",
				"marker",
				"operation",
				"prepared_at_micros",
				"repository_cwd",
				"revision",
				"role_profile_revision",
				"routing_snapshot_id",
				"state",
				"thread_title",
			],
		)?;
		if required_str(&effect, "experiment_id")? != identity.experiment_id
			|| required_str(&effect, "managed_run_id")? != identity.managed_run_id.as_str()
			|| required_i64(&effect, "managed_run_revision")? != identity.managed_run_revision
			|| required_str(&effect, "routing_snapshot_id")? != identity.routing_snapshot_id
			|| required_str(&effect, "account_id")? != identity.account_id.as_str()
			|| required_i64(&effect, "account_revision")? != identity.account_revision
			|| required_i64(&effect, "role_profile_revision")? != identity.role_profile_revision
			|| required_str(&effect, "build_id")? != identity.build_id
			|| required_str(&effect, "repository_cwd")? != identity.repository_cwd
			|| required_str(&effect, "thread_title")? != identity.thread_title
			|| required_str(&effect, "marker")? != identity.retained_marker()
			|| required_str(&effect, "state")? != "prepared"
			|| required_i64(&effect, "revision")? != 1
		{
			return incompatible("V15 preparation response is cross-linked");
		}
		Ok(CodexExperimentCommandOutcome::Applied(CodexExperimentPrepared {
			identity: identity.clone(),
			revision: 1,
			marker: identity.retained_marker(),
			prepared_at_micros: required_i64(&effect, "prepared_at_micros")?,
		}))
	}

	/// Commit the terminal possibly-created fence before invoking `thread/start`.
	pub async fn mark_codex_experiment_creation_possible(
		&self,
		idempotency_key: &str,
		experiment_id: &str,
		expected_revision: i64,
		attempt_id: &str,
	) -> Result<CodexExperimentCreationFenceOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(experiment_id, "experiment identity")?;
		validate_uuid(attempt_id, "creation attempt identity")?;
		if expected_revision != 1 {
			return Err(StoreError::InvalidInput("creation fence requires prepared revision one"));
		}
		let (response, replayed) = self
			.execute_exact_with_replay_status(
				"SELECT response_bytes,replayed FROM decodex.mark_codex_experiment_creation_possible_exact(\
			 $1,$2,$3::text::uuid,$4,$5::text::uuid)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&experiment_id,
					&expected_revision,
					&attempt_id,
				],
			)
			.await?;
		let (classification, effect) =
			parse_envelope(&response, "mark_codex_experiment_creation_possible")?;
		if classification == "stable_domain_rejection" {
			return Ok(CodexExperimentCreationFenceOutcome::Rejected(parse_rejection(&effect)?));
		}
		require_keys(
			&effect,
			&[
				"attempt_id",
				"effect_digest",
				"effect_digest_source",
				"experiment_id",
				"fenced_at_micros",
				"operation",
				"revision",
				"state",
			],
		)?;
		if required_str(&effect, "experiment_id")? != experiment_id
			|| required_str(&effect, "attempt_id")? != attempt_id
			|| required_i64(&effect, "revision")? != 2
			|| required_str(&effect, "state")? != "creation_possible"
		{
			return incompatible("V15 creation fence response is cross-linked");
		}
		if replayed {
			return Ok(CodexExperimentCreationFenceOutcome::ReplayedAmbiguous {
				experiment_id: experiment_id.to_owned(),
				revision: 2,
				attempt_id: attempt_id.to_owned(),
			});
		}
		Ok(CodexExperimentCreationFenceOutcome::Fresh(FreshCodexExperimentCreation {
			fact: CodexExperimentCreationPossible {
				experiment_id: experiment_id.to_owned(),
				revision: 2,
				attempt_id: attempt_id.to_owned(),
				fenced_at_micros: required_i64(&effect, "fenced_at_micros")?,
			},
		}))
	}

	/// Bind one exact typed successful creation response. This never searches for or adopts a
	/// thread.
	pub async fn bind_codex_experiment_thread(
		&self,
		idempotency_key: &str,
		request: &BindCodexExperimentThread,
	) -> Result<CodexExperimentCommandOutcome<CodexExperimentThreadBinding>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&request.experiment_id, "experiment identity")?;
		validate_uuid(&request.attempt_id, "creation attempt identity")?;
		if request.expected_revision != 2
			|| !bounded(&request.thread_id, 1024)
			|| !bounded(&request.response_id, 1024)
			|| !bounded(&request.response_title, 512)
			|| !bounded(&request.response_cwd, 4096)
			|| !bounded(&request.response_marker, 256)
		{
			return Err(StoreError::InvalidInput("typed creation response is malformed"));
		}
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.bind_codex_experiment_thread_exact($1,$2,$3::text::uuid,$4,\
			 $5::text::uuid,$6,$7,$8,$9,$10,$11)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.experiment_id,
					&request.expected_revision,
					&request.attempt_id,
					&request.thread_id,
					&request.response_id,
					&request.response_title,
					&request.response_cwd,
					&request.response_marker,
					&request.ephemeral,
				],
			)
			.await?;
		let (classification, effect) = parse_envelope(&response, "bind_codex_experiment_thread")?;
		if classification == "stable_domain_rejection" {
			return Ok(CodexExperimentCommandOutcome::Rejected(parse_rejection(&effect)?));
		}
		require_keys(
			&effect,
			&[
				"attempt_id",
				"bound_at_micros",
				"effect_digest",
				"effect_digest_source",
				"experiment_id",
				"marker",
				"operation",
				"response_id",
				"revision",
				"state",
				"thread_id",
			],
		)?;
		if required_str(&effect, "experiment_id")? != request.experiment_id
			|| required_str(&effect, "attempt_id")? != request.attempt_id
			|| required_str(&effect, "thread_id")? != request.thread_id
			|| required_str(&effect, "response_id")? != request.response_id
			|| required_str(&effect, "marker")? != request.response_marker
			|| required_i64(&effect, "revision")? != 3
			|| required_str(&effect, "state")? != "thread_bound"
		{
			return incompatible("V15 thread binding response is cross-linked");
		}
		Ok(CodexExperimentCommandOutcome::Applied(CodexExperimentThreadBinding {
			experiment_id: request.experiment_id.clone(),
			revision: 3,
			attempt_id: request.attempt_id.clone(),
			thread_id: request.thread_id.clone(),
			response_id: request.response_id.clone(),
			bound_at_micros: required_i64(&effect, "bound_at_micros")?,
		}))
	}

	/// Append one causally bound positive exact observation.
	pub async fn record_codex_experiment_observation(
		&self,
		idempotency_key: &str,
		request: &RecordCodexExperimentObservation,
	) -> Result<CodexExperimentCommandOutcome<CodexExperimentObservation>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&request.experiment_id, "experiment identity")?;
		validate_uuid(&request.observation_id, "observation identity")?;
		if request.expected_revision != 3
			|| !bounded(&request.thread_id, 1024)
			|| !bounded(&request.marker, 256)
			|| !bounded(&request.source_id, 1024)
			|| !is_hex_digest(&request.fact_digest)
		{
			return Err(StoreError::InvalidInput("positive observation is malformed"));
		}
		let kind = request.kind.as_sql();
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.record_codex_experiment_observation_exact($1,$2,$3::text::uuid,$4,\
			 $5::text::uuid,$6::text::decodex.codex_experiment_observation_kind,$7,$8,$9,$10)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.experiment_id,
					&request.expected_revision,
					&request.observation_id,
					&kind,
					&request.thread_id,
					&request.marker,
					&request.source_id,
					&request.fact_digest,
				],
			)
			.await?;
		let (classification, effect) =
			parse_envelope(&response, "record_codex_experiment_observation")?;
		if classification == "stable_domain_rejection" {
			return Ok(CodexExperimentCommandOutcome::Rejected(parse_rejection(&effect)?));
		}
		require_keys(
			&effect,
			&[
				"effect_digest",
				"effect_digest_source",
				"experiment_id",
				"experiment_revision",
				"fact_digest",
				"kind",
				"marker",
				"observation_id",
				"observed_at_micros",
				"operation",
				"source_id",
				"thread_id",
			],
		)?;
		if required_str(&effect, "experiment_id")? != request.experiment_id
			|| required_i64(&effect, "experiment_revision")? != 3
			|| required_str(&effect, "observation_id")? != request.observation_id
			|| required_str(&effect, "kind")? != kind
			|| required_str(&effect, "thread_id")? != request.thread_id
			|| required_str(&effect, "marker")? != request.marker
			|| required_str(&effect, "source_id")? != request.source_id
			|| required_str(&effect, "fact_digest")? != request.fact_digest
		{
			return incompatible("V15 observation response is cross-linked");
		}
		Ok(CodexExperimentCommandOutcome::Applied(CodexExperimentObservation {
			experiment_id: request.experiment_id.clone(),
			experiment_revision: 3,
			observation_id: request.observation_id.clone(),
			kind: request.kind,
			source_id: request.source_id.clone(),
			observed_at_micros: required_i64(&effect, "observed_at_micros")?,
		}))
	}
}

fn validate_identity(identity: &CodexExperimentIdentity) -> Result<(), StoreError> {
	validate_uuid(&identity.experiment_id, "experiment identity")?;
	validate_uuid(identity.managed_run_id.as_str(), "ManagedRun identity")?;
	validate_uuid(&identity.routing_snapshot_id, "routing snapshot identity")?;
	if identity.managed_run_revision <= 0
		|| identity.account_revision <= 0
		|| identity.role_profile_revision <= 0
		|| !bounded(&identity.build_id, 512)
		|| !bounded(&identity.repository_cwd, 4096)
		|| !bounded(&identity.thread_title, 512)
		|| !identity.thread_title.contains(&identity.retained_marker())
	{
		return Err(StoreError::InvalidInput("experiment identity is malformed"));
	}
	Ok(())
}

fn parse_envelope(bytes: &[u8], operation: &str) -> Result<(String, Value), StoreError> {
	let document: Value = serde_json::from_slice(bytes)
		.map_err(|_| StoreError::Incompatible("stored V15 response bytes are malformed".into()))?;
	require_keys(&document, &["classification", "effect"])?;
	let classification = required_str(&document, "classification")?;
	if !matches!(classification, "success" | "stable_domain_rejection") {
		return incompatible("stored V15 response classification is unknown");
	}
	let effect = document
		.get("effect")
		.filter(|value| value.is_object())
		.ok_or_else(|| StoreError::Incompatible("stored V15 effect is malformed".into()))?;
	verify_digest(effect)?;
	if required_str(effect, "operation")? != operation {
		return incompatible("stored V15 response operation is cross-linked");
	}
	Ok((classification.to_owned(), effect.clone()))
}

fn verify_digest(effect: &Value) -> Result<(), StoreError> {
	let source = required_str(effect, "effect_digest_source")?;
	let digest = required_str(effect, "effect_digest")?;
	if !is_hex_digest(digest) || hex_sha256(source.as_bytes()) != digest {
		return incompatible("stored V15 effect digest is invalid");
	}
	let source_value: Value = serde_json::from_str(source)
		.map_err(|_| StoreError::Incompatible("stored V15 digest source is malformed".into()))?;
	let mut projection = effect.as_object().expect("checked object").clone();
	projection.remove("effect_digest");
	projection.remove("effect_digest_source");
	if source_value != Value::Object(projection) {
		return incompatible("stored V15 effect projection differs from its digest source");
	}
	Ok(())
}

fn parse_rejection(effect: &Value) -> Result<CodexExperimentRejection, StoreError> {
	require_keys(effect, &["effect_digest", "effect_digest_source", "operation", "rejection"])?;
	let operation = required_str(effect, "operation")?;
	let code = required_str(effect, "rejection")?;
	let known = match operation {
		"prepare_codex_experiment" =>
			matches!(code, "invalid_identity" | "lineage_mismatch" | "experiment_exists"),
		"mark_codex_experiment_creation_possible" =>
			matches!(code, "creation_not_authorized" | "attempt_identity_conflict"),
		"bind_codex_experiment_thread" =>
			matches!(code, "typed_response_mismatch" | "thread_identity_conflict"),
		"record_codex_experiment_observation" => matches!(
			code,
			"observation_lineage_mismatch"
				| "observation_identity_conflict"
				| "observation_fact_conflict"
		),
		_ => false,
	};
	if !known {
		return incompatible("stored V15 rejection code is unknown");
	}
	Ok(CodexExperimentRejection { operation: operation.to_owned(), code: code.to_owned() })
}

fn require_keys(value: &Value, expected: &[&str]) -> Result<(), StoreError> {
	let object = value
		.as_object()
		.ok_or_else(|| StoreError::Incompatible("stored V15 object is malformed".into()))?;
	let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
	actual.sort_unstable();
	let mut expected = expected.to_vec();
	expected.sort_unstable();
	if actual == expected {
		Ok(())
	} else {
		incompatible("stored V15 object has missing or unknown keys")
	}
}
fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible(format!("stored V15 {key} is malformed")))
}
fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.filter(|number| *number > 0)
		.ok_or_else(|| StoreError::Incompatible(format!("stored V15 {key} is malformed")))
}
fn validate_uuid(value: &str, label: &'static str) -> Result<(), StoreError> {
	if value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		}) {
		Ok(())
	} else {
		Err(StoreError::InvalidInput(label))
	}
}
fn bounded(value: &str, maximum: usize) -> bool {
	!value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}
fn is_hex_digest(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn hex_sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
fn incompatible<T>(message: &str) -> Result<T, StoreError> {
	Err(StoreError::Incompatible(message.into()))
}
