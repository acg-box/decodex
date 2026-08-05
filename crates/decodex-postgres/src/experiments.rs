use decodex_core::{
	CodexExperimentCommandOutcome, CodexExperimentCreationPossible, CodexExperimentIdentity,
	CodexExperimentObservation, CodexExperimentObservationKind, CodexExperimentPrepared,
	CodexExperimentRejection, CodexExperimentRetainedTitleAttestation,
	CodexExperimentThreadBinding, CodexExperimentTitleSetPossible,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	PostgresStore, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};

const BIND_CODEX_EXPERIMENT_START_SQL: &str = "SELECT decodex.bind_codex_experiment_start_exact($1,$2,$3::text::uuid,$4,\
	 $5::text::uuid,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)";
const READ_CODEX_EXPERIMENT_START_SQL: &str = "SELECT experiment_id::text,attempt_id::text,experiment_revision,thread_id,\
	 start_request_id,start_request_digest,request_cwd,request_marker,request_ephemeral,\
	 start_response_id,start_response_digest,response_cwd,response_marker,\
	 response_ephemeral,returned_name,bound_at_micros \
	 FROM decodex.read_codex_experiment_start_exact($1::text::uuid,$2::text::uuid)";
const MARK_CODEX_EXPERIMENT_TITLE_SET_POSSIBLE_SQL: &str = "SELECT response_bytes,replayed FROM \
	 decodex.mark_codex_experiment_title_set_possible_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6,$7,$8,$9)";
const ATTEST_CODEX_EXPERIMENT_RETAINED_TITLE_SQL: &str = "SELECT decodex.attest_codex_experiment_retained_title_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6::text::uuid,$7,$8,$9,$10,$11,$12,$13,$14)";
const RECORD_ATTESTED_CODEX_EXPERIMENT_OBSERVATION_SQL: &str = "SELECT decodex.record_attested_codex_experiment_observation_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6::text::uuid,\
	 $7::text::decodex.codex_experiment_observation_kind,$8,$9,$10,$11)";

/// Preparation input. PostgreSQL rechecks the complete V14 lineage and owns the clock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareCodexExperiment {
	/// Complete immutable experiment identity whose V14 lineage PostgreSQL must verify.
	pub identity: CodexExperimentIdentity,
}

/// Exact nullable-name `thread/start` request and response facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindCodexExperimentStart {
	/// Immutable experiment identity previously advanced to the pre-effect fence.
	pub experiment_id: String,
	/// Required current experiment revision, fixed at the creation-possible revision two.
	pub expected_revision: i64,
	/// Exact creation-attempt identity durably fenced before the external effect.
	pub attempt_id: String,
	/// Exact thread identity returned by the successful typed app-server response.
	pub thread_id: String,
	/// Positive numeric JSON-RPC identity of the exact serialized request.
	pub start_request_id: i64,
	/// Lowercase SHA-256 of the exact serialized request frame, including its newline.
	pub start_request_digest: String,
	/// Exact prepared repository working directory sent in the request.
	pub request_cwd: String,
	/// Exact prepared provenance marker sent in the request.
	pub request_marker: String,
	/// Request ephemeral flag. It must be false.
	pub request_ephemeral: bool,
	/// Positive numeric JSON-RPC identity of the exact raw response.
	pub start_response_id: i64,
	/// Lowercase SHA-256 of the exact raw response frame, including its newline.
	pub start_response_digest: String,
	/// Response working directory that must equal the immutable prepared repository path.
	pub response_cwd: String,
	/// Response marker that must equal the marker deterministically derived from the experiment
	/// identity.
	pub response_marker: String,
	/// App-server ephemeral flag, which must be false for a durable binding.
	pub response_ephemeral: bool,
	/// Nullable name returned by the pinned build. This value must be `None`.
	pub returned_name: Option<String>,
}

/// Exact durable start receipt used only after a creation-fence replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentStartReceipt {
	/// Canonical experiment UUID text.
	pub experiment_id: String,
	/// Sole creation-attempt UUID text.
	pub attempt_id: String,
	/// Bound experiment revision, fixed at three.
	pub experiment_revision: i64,
	/// Exact thread identity returned by `thread/start`.
	pub thread_id: String,
	/// Exact request identity.
	pub start_request_id: i64,
	/// Exact request-frame digest.
	pub start_request_digest: String,
	/// Exact prepared repository working directory sent in the request.
	pub request_cwd: String,
	/// Exact prepared provenance marker sent in the request.
	pub request_marker: String,
	/// Request ephemeral flag, fixed to false.
	pub request_ephemeral: bool,
	/// Exact response identity.
	pub start_response_id: i64,
	/// Exact raw-response digest.
	pub start_response_digest: String,
	/// Exact prepared repository working directory.
	pub response_cwd: String,
	/// Exact prepared provenance marker.
	pub response_marker: String,
	/// Response ephemeral flag, fixed to false.
	pub response_ephemeral: bool,
	/// Nullable raw name. The pinned build requires `None`.
	pub returned_name: Option<String>,
	/// PostgreSQL-owned binding time in UTC Unix microseconds.
	pub bound_at_micros: i64,
}

/// Exact input for the durable one-shot `thread/name/set` fence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FenceCodexExperimentTitleSet {
	/// Canonical experiment UUID text.
	pub experiment_id: String,
	/// Required bound-thread revision, fixed at three.
	pub expected_revision: i64,
	/// Canonical UUID text for the sole title-set attempt.
	pub title_attempt_id: String,
	/// Exact thread identity from the durable start receipt.
	pub thread_id: String,
	/// Exact JSON-RPC request identity prepared before fencing.
	pub request_id: i64,
	/// Lowercase SHA-256 of the exact prepared `thread/name/set` request frame.
	pub request_digest: String,
	/// Exact immutable prepared title bound into the fenced request.
	pub requested_title: String,
}

/// Exact positive readback that attests the prepared title on the bound thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttestCodexExperimentRetainedTitle {
	/// Canonical experiment UUID text.
	pub experiment_id: String,
	/// Required bound-thread revision, fixed at three.
	pub expected_revision: i64,
	/// Canonical UUID text for this immutable attestation.
	pub attestation_id: String,
	/// Sole title-set attempt UUID text.
	pub title_attempt_id: String,
	/// Exact thread identity used by the read request and response.
	pub thread_id: String,
	/// Exact JSON-RPC identity of the read request.
	pub read_request_id: i64,
	/// Lowercase SHA-256 of the exact serialized read request frame.
	pub read_request_digest: String,
	/// Exact JSON-RPC identity of the raw read response.
	pub read_response_id: i64,
	/// Lowercase SHA-256 of the exact raw read response frame.
	pub read_response_digest: String,
	/// Exact title returned by the positive readback.
	pub returned_title: String,
	/// Exact working directory returned by the positive readback.
	pub returned_cwd: String,
	/// Exact retained provenance marker returned by the positive readback.
	pub returned_marker: String,
}

/// One exact positive observation. There is no negative observation variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordCodexExperimentObservation {
	/// Immutable identity of the experiment that owns the observation.
	pub experiment_id: String,
	/// Required bound-thread revision, fixed at revision three.
	pub expected_revision: i64,
	/// Exact retained-title attestation that authorizes this observation.
	pub attestation_id: String,
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

/// One-shot permission emitted only for a freshly committed title-set fence.
#[derive(Debug, Eq, PartialEq)]
pub struct FreshCodexExperimentTitleSet {
	fact: CodexExperimentTitleSetPossible,
}
impl FreshCodexExperimentTitleSet {
	/// Return the experiment identity whose fresh title fence committed.
	pub fn experiment_id(&self) -> &str {
		&self.fact.experiment_id
	}

	/// Return the sole title-attempt identity.
	pub fn title_attempt_id(&self) -> &str {
		&self.fact.title_attempt_id
	}

	/// Return the exact bound thread identity.
	pub fn thread_id(&self) -> &str {
		&self.fact.thread_id
	}

	/// Return the exact prepared request identity.
	pub fn request_id(&self) -> i64 {
		self.fact.request_id
	}

	/// Return the exact prepared request-frame digest.
	pub fn request_digest(&self) -> &str {
		&self.fact.request_digest
	}

	/// Return the exact prepared title bound into the request.
	pub fn requested_title(&self) -> &str {
		&self.fact.requested_title
	}
}

/// Title-set fence result. Replay authorizes readback only and never another name-set effect.
#[derive(Debug, Eq, PartialEq)]
pub enum CodexExperimentTitleSetFenceOutcome {
	/// Newly committed fence that authorizes one `thread/name/set` request.
	Fresh(FreshCodexExperimentTitleSet),
	/// Durable replay that authorizes bounded exact-ID readback but no second set request.
	ReplayedReadbackOnly {
		/// Canonical experiment UUID text.
		experiment_id: String,
		/// Sole title-set attempt UUID text.
		title_attempt_id: String,
		/// Exact bound thread identity.
		thread_id: String,
	},
	/// Stable PostgreSQL-authored rejection.
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

	/// Bind one exact nullable-name `thread/start` response without search or adoption.
	pub async fn bind_codex_experiment_start(
		&self,
		idempotency_key: &str,
		request: &BindCodexExperimentStart,
	) -> Result<CodexExperimentCommandOutcome<CodexExperimentThreadBinding>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&request.experiment_id, "experiment identity")?;
		validate_uuid(&request.attempt_id, "creation attempt identity")?;
		if request.expected_revision != 2
			|| !bounded(&request.thread_id, 1024)
			|| request.start_request_id <= 0
			|| request.start_response_id <= 0
			|| !is_hex_digest(&request.start_request_digest)
			|| !is_hex_digest(&request.start_response_digest)
			|| !bounded(&request.request_cwd, 4096)
			|| !bounded(&request.request_marker, 256)
			|| !bounded(&request.response_cwd, 4096)
			|| !bounded(&request.response_marker, 256)
			|| request.returned_name.as_deref().is_some_and(|name| !bounded(name, 512))
		{
			return Err(StoreError::InvalidInput("typed thread/start response is malformed"));
		}
		let response = self
			.execute_exact_with_retry(
				BIND_CODEX_EXPERIMENT_START_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.experiment_id,
					&request.expected_revision,
					&request.attempt_id,
					&request.thread_id,
					&request.start_request_id,
					&request.start_request_digest,
					&request.request_cwd,
					&request.request_marker,
					&request.request_ephemeral,
					&request.start_response_id,
					&request.start_response_digest,
					&request.response_cwd,
					&request.response_marker,
					&request.response_ephemeral,
					&request.returned_name,
				],
			)
			.await?;
		let (classification, effect) = parse_envelope(&response, "bind_codex_experiment_start")?;
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
				"operation",
				"request_cwd",
				"request_ephemeral",
				"request_marker",
				"response_cwd",
				"response_ephemeral",
				"response_marker",
				"returned_name",
				"revision",
				"start_request_digest",
				"start_request_id",
				"start_response_digest",
				"start_response_id",
				"state",
				"thread_id",
			],
		)?;
		if required_str(&effect, "experiment_id")? != request.experiment_id
			|| required_str(&effect, "attempt_id")? != request.attempt_id
			|| required_str(&effect, "thread_id")? != request.thread_id
			|| required_i64(&effect, "start_request_id")? != request.start_request_id
			|| required_str(&effect, "start_request_digest")? != request.start_request_digest
			|| required_str(&effect, "request_cwd")? != request.request_cwd
			|| required_str(&effect, "request_marker")? != request.request_marker
			|| required_bool(&effect, "request_ephemeral")? != request.request_ephemeral
			|| required_i64(&effect, "start_response_id")? != request.start_response_id
			|| required_str(&effect, "start_response_digest")? != request.start_response_digest
			|| required_str(&effect, "response_cwd")? != request.response_cwd
			|| required_str(&effect, "response_marker")? != request.response_marker
			|| required_bool(&effect, "response_ephemeral")? != request.response_ephemeral
			|| optional_str(&effect, "returned_name")? != request.returned_name.as_deref()
			|| required_i64(&effect, "revision")? != 3
			|| required_str(&effect, "state")? != "thread_bound"
		{
			return incompatible("V22 start binding response is cross-linked");
		}
		Ok(CodexExperimentCommandOutcome::Applied(CodexExperimentThreadBinding {
			experiment_id: request.experiment_id.clone(),
			revision: 3,
			attempt_id: request.attempt_id.clone(),
			thread_id: request.thread_id.clone(),
			start_request_id: request.start_request_id,
			start_request_digest: request.start_request_digest.clone(),
			request_cwd: request.request_cwd.clone(),
			request_marker: request.request_marker.clone(),
			request_ephemeral: request.request_ephemeral,
			start_response_id: request.start_response_id,
			start_response_digest: request.start_response_digest.clone(),
			response_cwd: request.response_cwd.clone(),
			response_marker: request.response_marker.clone(),
			response_ephemeral: request.response_ephemeral,
			returned_name: request.returned_name.clone(),
			bound_at_micros: required_i64(&effect, "bound_at_micros")?,
		}))
	}

	/// Read one exact durable start receipt after a creation-fence replay.
	pub async fn read_codex_experiment_start_exact(
		&self,
		experiment_id: &str,
		attempt_id: &str,
	) -> Result<Option<CodexExperimentStartReceipt>, StoreError> {
		validate_uuid(experiment_id, "experiment identity")?;
		validate_uuid(attempt_id, "creation attempt identity")?;
		let client = self.pool().get().await?;
		let row = client
			.query_opt(READ_CODEX_EXPERIMENT_START_SQL, &[&experiment_id, &attempt_id])
			.await?;
		let Some(row) = row else { return Ok(None) };
		let receipt = CodexExperimentStartReceipt {
			experiment_id: row.get(0),
			attempt_id: row.get(1),
			experiment_revision: row.get(2),
			thread_id: row.get(3),
			start_request_id: row.get(4),
			start_request_digest: row.get(5),
			request_cwd: row.get(6),
			request_marker: row.get(7),
			request_ephemeral: row.get(8),
			start_response_id: row.get(9),
			start_response_digest: row.get(10),
			response_cwd: row.get(11),
			response_marker: row.get(12),
			response_ephemeral: row.get(13),
			returned_name: row.get(14),
			bound_at_micros: row.get(15),
		};
		if receipt.experiment_id != experiment_id
			|| receipt.attempt_id != attempt_id
			|| receipt.experiment_revision != 3
			|| receipt.start_request_id <= 0
			|| receipt.start_response_id != receipt.start_request_id
			|| !is_hex_digest(&receipt.start_request_digest)
			|| !is_hex_digest(&receipt.start_response_digest)
			|| !bounded(&receipt.thread_id, 1024)
			|| !bounded(&receipt.response_cwd, 4096)
			|| !bounded(&receipt.response_marker, 256)
			|| receipt.request_cwd != receipt.response_cwd
			|| receipt.request_marker != receipt.response_marker
			|| receipt.request_ephemeral
			|| receipt.response_ephemeral
			|| receipt.returned_name.is_some()
		{
			return incompatible("V22 exact start receipt is malformed or cross-linked");
		}
		Ok(Some(receipt))
	}

	/// Commit the one-shot title-set fence before `thread/name/set`.
	pub async fn mark_codex_experiment_title_set_possible(
		&self,
		idempotency_key: &str,
		request: &FenceCodexExperimentTitleSet,
	) -> Result<CodexExperimentTitleSetFenceOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&request.experiment_id, "experiment identity")?;
		validate_uuid(&request.title_attempt_id, "title attempt identity")?;
		if request.expected_revision != 3
			|| !bounded(&request.thread_id, 1024)
			|| request.request_id <= 0
			|| !is_hex_digest(&request.request_digest)
			|| !bounded(&request.requested_title, 512)
		{
			return Err(StoreError::InvalidInput("title-set fence is malformed"));
		}
		let (response, replayed) = self
			.execute_exact_with_replay_status(
				MARK_CODEX_EXPERIMENT_TITLE_SET_POSSIBLE_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.experiment_id,
					&request.expected_revision,
					&request.title_attempt_id,
					&request.thread_id,
					&request.request_id,
					&request.request_digest,
					&request.requested_title,
				],
			)
			.await?;
		let (classification, effect) =
			parse_envelope(&response, "mark_codex_experiment_title_set_possible")?;
		if classification == "stable_domain_rejection" {
			return Ok(CodexExperimentTitleSetFenceOutcome::Rejected(parse_rejection(&effect)?));
		}
		require_keys(
			&effect,
			&[
				"effect_digest",
				"effect_digest_source",
				"experiment_id",
				"experiment_revision",
				"fenced_at_micros",
				"operation",
				"request_digest",
				"request_id",
				"requested_title",
				"thread_id",
				"title_attempt_id",
			],
		)?;
		if required_str(&effect, "experiment_id")? != request.experiment_id
			|| required_i64(&effect, "experiment_revision")? != 3
			|| required_str(&effect, "title_attempt_id")? != request.title_attempt_id
			|| required_str(&effect, "thread_id")? != request.thread_id
			|| required_i64(&effect, "request_id")? != request.request_id
			|| required_str(&effect, "request_digest")? != request.request_digest
			|| required_str(&effect, "requested_title")? != request.requested_title
		{
			return incompatible("V22 title-set fence response is cross-linked");
		}
		if replayed {
			return Ok(CodexExperimentTitleSetFenceOutcome::ReplayedReadbackOnly {
				experiment_id: request.experiment_id.clone(),
				title_attempt_id: request.title_attempt_id.clone(),
				thread_id: request.thread_id.clone(),
			});
		}
		Ok(CodexExperimentTitleSetFenceOutcome::Fresh(FreshCodexExperimentTitleSet {
			fact: CodexExperimentTitleSetPossible {
				experiment_id: request.experiment_id.clone(),
				experiment_revision: 3,
				title_attempt_id: request.title_attempt_id.clone(),
				thread_id: request.thread_id.clone(),
				request_id: request.request_id,
				request_digest: request.request_digest.clone(),
				requested_title: request.requested_title.clone(),
				fenced_at_micros: required_i64(&effect, "fenced_at_micros")?,
			},
		}))
	}

	/// Persist an exact-ID readback only when all retained-title facts match.
	pub async fn attest_codex_experiment_retained_title(
		&self,
		idempotency_key: &str,
		request: &AttestCodexExperimentRetainedTitle,
	) -> Result<CodexExperimentCommandOutcome<CodexExperimentRetainedTitleAttestation>, StoreError>
	{
		validate_exact_key(idempotency_key)?;
		validate_uuid(&request.experiment_id, "experiment identity")?;
		validate_uuid(&request.attestation_id, "attestation identity")?;
		validate_uuid(&request.title_attempt_id, "title attempt identity")?;
		if request.expected_revision != 3
			|| !bounded(&request.thread_id, 1024)
			|| request.read_request_id <= 0
			|| request.read_response_id <= 0
			|| !is_hex_digest(&request.read_request_digest)
			|| !is_hex_digest(&request.read_response_digest)
			|| !bounded(&request.returned_title, 512)
			|| !bounded(&request.returned_cwd, 4096)
			|| !bounded(&request.returned_marker, 256)
		{
			return Err(StoreError::InvalidInput("retained-title attestation is malformed"));
		}
		let response = self
			.execute_exact_with_retry(
				ATTEST_CODEX_EXPERIMENT_RETAINED_TITLE_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.experiment_id,
					&request.expected_revision,
					&request.attestation_id,
					&request.title_attempt_id,
					&request.thread_id,
					&request.read_request_id,
					&request.read_request_digest,
					&request.read_response_id,
					&request.read_response_digest,
					&request.returned_title,
					&request.returned_cwd,
					&request.returned_marker,
				],
			)
			.await?;
		let (classification, effect) =
			parse_envelope(&response, "attest_codex_experiment_retained_title")?;
		if classification == "stable_domain_rejection" {
			return Ok(CodexExperimentCommandOutcome::Rejected(parse_rejection(&effect)?));
		}
		require_keys(
			&effect,
			&[
				"attestation_id",
				"attested_at_micros",
				"effect_digest",
				"effect_digest_source",
				"experiment_id",
				"experiment_revision",
				"marker",
				"operation",
				"read_request_digest",
				"read_request_id",
				"read_response_digest",
				"read_response_id",
				"retained_title",
				"returned_cwd",
				"thread_id",
				"title_attempt_id",
			],
		)?;
		if required_str(&effect, "experiment_id")? != request.experiment_id
			|| required_i64(&effect, "experiment_revision")? != 3
			|| required_str(&effect, "attestation_id")? != request.attestation_id
			|| required_str(&effect, "title_attempt_id")? != request.title_attempt_id
			|| required_str(&effect, "thread_id")? != request.thread_id
			|| required_i64(&effect, "read_request_id")? != request.read_request_id
			|| required_str(&effect, "read_request_digest")? != request.read_request_digest
			|| required_i64(&effect, "read_response_id")? != request.read_response_id
			|| required_str(&effect, "read_response_digest")? != request.read_response_digest
			|| required_str(&effect, "retained_title")? != request.returned_title
			|| required_str(&effect, "returned_cwd")? != request.returned_cwd
			|| required_str(&effect, "marker")? != request.returned_marker
		{
			return incompatible("V22 retained-title attestation response is cross-linked");
		}
		Ok(CodexExperimentCommandOutcome::Applied(CodexExperimentRetainedTitleAttestation {
			experiment_id: request.experiment_id.clone(),
			attestation_id: request.attestation_id.clone(),
			title_attempt_id: request.title_attempt_id.clone(),
			thread_id: request.thread_id.clone(),
			read_request_id: request.read_request_id,
			read_request_digest: request.read_request_digest.clone(),
			read_response_id: request.read_response_id,
			read_response_digest: request.read_response_digest.clone(),
			retained_title: request.returned_title.clone(),
			returned_cwd: request.returned_cwd.clone(),
			marker: request.returned_marker.clone(),
			attested_at_micros: required_i64(&effect, "attested_at_micros")?,
		}))
	}

	/// Append one causally bound positive exact observation.
	pub async fn record_attested_codex_experiment_observation(
		&self,
		idempotency_key: &str,
		request: &RecordCodexExperimentObservation,
	) -> Result<CodexExperimentCommandOutcome<CodexExperimentObservation>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&request.experiment_id, "experiment identity")?;
		validate_uuid(&request.attestation_id, "attestation identity")?;
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
				RECORD_ATTESTED_CODEX_EXPERIMENT_OBSERVATION_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.experiment_id,
					&request.expected_revision,
					&request.attestation_id,
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
			parse_envelope(&response, "record_attested_codex_experiment_observation")?;
		if classification == "stable_domain_rejection" {
			return Ok(CodexExperimentCommandOutcome::Rejected(parse_rejection(&effect)?));
		}
		require_keys(
			&effect,
			&[
				"attestation_id",
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
			|| required_str(&effect, "attestation_id")? != request.attestation_id
			|| required_str(&effect, "observation_id")? != request.observation_id
			|| required_str(&effect, "kind")? != kind
			|| required_str(&effect, "thread_id")? != request.thread_id
			|| required_str(&effect, "marker")? != request.marker
			|| required_str(&effect, "source_id")? != request.source_id
			|| required_str(&effect, "fact_digest")? != request.fact_digest
		{
			return incompatible("V22 attested observation response is cross-linked");
		}
		Ok(CodexExperimentCommandOutcome::Applied(CodexExperimentObservation {
			experiment_id: request.experiment_id.clone(),
			experiment_revision: 3,
			observation_id: request.observation_id.clone(),
			attestation_id: request.attestation_id.clone(),
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
		"prepare_codex_experiment" => {
			matches!(code, "invalid_identity" | "lineage_mismatch" | "experiment_exists")
		},
		"mark_codex_experiment_creation_possible" => {
			matches!(code, "creation_not_authorized" | "attempt_identity_conflict")
		},
		"bind_codex_experiment_start" => {
			matches!(code, "start_response_mismatch" | "start_identity_conflict")
		},
		"mark_codex_experiment_title_set_possible" => {
			matches!(code, "title_set_not_authorized" | "title_attempt_identity_conflict")
		},
		"attest_codex_experiment_retained_title" => {
			matches!(code, "retained_title_mismatch" | "attestation_identity_conflict")
		},
		"record_attested_codex_experiment_observation" => matches!(
			code,
			"attested_observation_lineage_mismatch"
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
		.ok_or_else(|| StoreError::Incompatible(format!("stored experiment {key} is malformed")))
}
fn required_bool(value: &Value, key: &str) -> Result<bool, StoreError> {
	value
		.get(key)
		.and_then(Value::as_bool)
		.ok_or_else(|| StoreError::Incompatible(format!("stored V22 {key} is malformed")))
}
fn optional_str<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(text)) => Ok(Some(text)),
		_ => Err(StoreError::Incompatible(format!("stored V22 {key} is malformed"))),
	}
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
