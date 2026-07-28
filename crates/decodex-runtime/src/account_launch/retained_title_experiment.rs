//! Manual V22 retained-title experiment composition.
//!
//! This module has no production feature edge. It does not expose turns, thread search, thread
//! list, adoption, archive, routing, scheduling, or dispatch.

use std::{
	env,
	fmt::{Display, Formatter},
	fs,
	path::Path,
	time::Duration,
};

use decodex_codex::{
	ExactRpcRequestFact, ExactRpcResponseFact, ExactThreadId, ThreadCwd, ThreadProvenance,
	ThreadTitle, TypedRetainedTitleReadResponse, TypedThreadNameSetRequest,
	TypedThreadStartResponse,
};
use decodex_core::{
	AccountId, CodexExperimentCommandOutcome, CodexExperimentIdentity,
	CodexExperimentObservationKind, DecodexConfig, DecodexRoot, ManagedRunId,
};
use decodex_postgres::{
	AttestCodexExperimentRetainedTitle, BindCodexExperimentStart,
	CodexExperimentCreationFenceOutcome, CodexExperimentStartReceipt,
	CodexExperimentTitleSetFenceOutcome, FenceCodexExperimentTitleSet, PostgresStore,
	PrepareCodexExperiment, RecordCodexExperimentObservation,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{
	RunnerCapacity,
	process::{
		AccountBinding, AppServerCommand, RpcError, launch_retained_title_process,
		retained_title_name_set_request_digest, retained_title_start_request_digest,
	},
};

const MAX_REQUEST_BYTES: usize = 64 * 1_024;
const MIN_TIMEOUT_MILLISECONDS: u64 = 1_000;
const MAX_TIMEOUT_MILLISECONDS: u64 = 60_000;
const START_REQUEST_ID: i64 = 3;
const TITLE_SET_REQUEST_ID: i64 = 4;
const READ_REQUEST_ID: i64 = 5;

/// Sanitized manual-run failure. External effect ambiguity never carries retry authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualRetainedTitleExperimentError {
	/// The request file or one of its exact identities is invalid.
	InvalidRequest,
	/// The local Decodex configuration is unavailable or invalid.
	ConfigurationUnavailable,
	/// PostgreSQL credentials or exact authority are unavailable.
	ProductStateUnavailable,
	/// The selected account revision is not ready before or after the experiment.
	AccountNotReady,
	/// The exact executable, schema, process, or account attestation failed before an effect.
	CodexPreflightFailed,
	/// The prepared build differs from the exact executable build.
	BuildMismatch,
	/// PostgreSQL rejected an exact durable transition.
	PersistenceRejected,
	/// `thread/start` can have happened, but no exact durable start binding exists.
	StartOutcomeAmbiguous,
	/// The title-set outcome or exact-ID readback cannot attest the prepared title.
	RetainedTitleAmbiguous,
	/// The bounded process group did not stop cleanly.
	CleanupFailed,
}
impl std::error::Error for ManualRetainedTitleExperimentError {}
impl Display for ManualRetainedTitleExperimentError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{self:?}")
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExperimentIdentityWire {
	experiment_id: String,
	managed_run_id: String,
	managed_run_revision: i64,
	routing_snapshot_id: String,
	account_id: String,
	account_revision: i64,
	role_profile_revision: i64,
	build_id: String,
	repository_cwd: String,
	thread_title: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualRequest {
	identity: ExperimentIdentityWire,
	creation_attempt_id: String,
	title_attempt_id: String,
	attestation_id: String,
	observation_id: String,
	timeout_milliseconds: u64,
}

/// Durable positive result from the inert manual runner.
#[derive(Debug, Serialize)]
pub struct ManualRetainedTitleExperimentReport {
	/// Canonical experiment UUID text.
	pub experiment_id: String,
	/// Exact thread identity returned by `thread/start` and used for all later calls.
	pub thread_id: String,
	/// Canonical retained-title attestation UUID text.
	pub attestation_id: String,
	/// Canonical positive observation UUID text.
	pub observation_id: String,
	/// True when this process observed the successful name-set response.
	pub title_set_response_observed: bool,
}

/// Run one manual, feature-gated V22 experiment from a bounded JSON request file.
#[allow(clippy::too_many_lines)] // Keep one closed, auditable experiment authority sequence.
pub async fn run_manual_retained_title_experiment(
	root: DecodexRoot,
	request_path: &Path,
) -> Result<ManualRetainedTitleExperimentReport, ManualRetainedTitleExperimentError> {
	let request = load_request(request_path)?;
	let identity = identity(&request.identity)?;
	validate_request(&request, &identity)?;
	let prepare_key = idempotency_key(&identity.experiment_id, "prepare");
	let creation_fence_key = idempotency_key(&identity.experiment_id, "creation-fence");
	let start_bind_key = idempotency_key(&identity.experiment_id, "start-bind");
	let title_fence_key = idempotency_key(&identity.experiment_id, "title-fence");
	let attestation_key = idempotency_key(&identity.experiment_id, "title-attestation");
	let observation_key = idempotency_key(&identity.experiment_id, "positive-observation");
	let config = DecodexConfig::load(&root.paths())
		.map_err(|_| ManualRetainedTitleExperimentError::ConfigurationUnavailable)?;
	let migration_password = credential(config.postgres().migration())?;
	let runtime_password = credential(config.postgres().runtime())?;
	let store = PostgresStore::connect_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
		runtime_password.as_deref().map(String::as_str),
	)
	.await
	.map_err(|_| ManualRetainedTitleExperimentError::ProductStateUnavailable)?;

	if !store
		.account_is_ready_at_revision(&identity.account_id, identity.account_revision)
		.await
		.map_err(|_| ManualRetainedTitleExperimentError::ProductStateUnavailable)?
	{
		return Err(ManualRetainedTitleExperimentError::AccountNotReady);
	}

	let timeout = Duration::from_millis(request.timeout_milliseconds);
	let binding = AccountBinding::shared_home(identity.account_id.clone())
		.map_err(|_| ManualRetainedTitleExperimentError::CodexPreflightFailed)?;
	let command = AppServerCommand::new(&identity.repository_cwd)
		.map_err(|_| ManualRetainedTitleExperimentError::CodexPreflightFailed)?;
	let guard = RunnerCapacity::daemon()
		.map_err(|_| ManualRetainedTitleExperimentError::CodexPreflightFailed)?
		.reserve(identity.account_id.clone(), identity.account_revision)
		.map_err(|_| ManualRetainedTitleExperimentError::CodexPreflightFailed)?;
	let (build, mut process) = launch_retained_title_process(command, binding, guard, timeout)
		.map_err(|_| ManualRetainedTitleExperimentError::CodexPreflightFailed)?;

	if build.as_str() != identity.build_id {
		return Err(ManualRetainedTitleExperimentError::BuildMismatch);
	}

	require_applied(
		store
			.prepare_codex_experiment(
				&prepare_key,
				&PrepareCodexExperiment { identity: identity.clone() },
			)
			.await,
	)?;
	let start_receipt = match store
		.mark_codex_experiment_creation_possible(
			&creation_fence_key,
			&identity.experiment_id,
			1,
			&request.creation_attempt_id,
		)
		.await
		.map_err(|_| ManualRetainedTitleExperimentError::ProductStateUnavailable)?
	{
		CodexExperimentCreationFenceOutcome::Fresh(permission) => {
			if permission.experiment_id() != identity.experiment_id
				|| permission.attempt_id() != request.creation_attempt_id
			{
				return Err(ManualRetainedTitleExperimentError::PersistenceRejected);
			}
			let start = process
				.start_retained_title_thread(
					START_REQUEST_ID,
					&identity.repository_cwd,
					&identity.retained_marker(),
					timeout,
				)
				.map_err(|_| ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?;
			let typed = typed_start(start)?;

			require_applied(
				store
					.bind_codex_experiment_start(
						&start_bind_key,
						&BindCodexExperimentStart {
							experiment_id: identity.experiment_id.clone(),
							expected_revision: 2,
							attempt_id: request.creation_attempt_id.clone(),
							thread_id: typed.thread_id.as_str().to_owned(),
							start_request_id: typed.request.id,
							start_request_digest: typed.request.digest,
							request_cwd: identity.repository_cwd.clone(),
							request_marker: identity.retained_marker(),
							request_ephemeral: false,
							start_response_id: typed.response.id,
							start_response_digest: typed.response.digest,
							response_cwd: typed.cwd.as_str().to_owned(),
							response_marker: typed.provenance.as_str().to_owned(),
							response_ephemeral: typed.ephemeral,
							returned_name: typed
								.returned_name
								.as_ref()
								.map(|name| name.as_str().to_owned()),
						},
					)
					.await,
			)
			.map_err(|_| ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?;
			read_start_receipt(&store, &identity, &request).await?
		},
		CodexExperimentCreationFenceOutcome::ReplayedAmbiguous { .. } =>
			read_start_receipt(&store, &identity, &request).await?,
		CodexExperimentCreationFenceOutcome::Rejected(_) => {
			return Err(ManualRetainedTitleExperimentError::PersistenceRejected);
		},
	};

	let title_request_digest = retained_title_name_set_request_digest(
		TITLE_SET_REQUEST_ID,
		&start_receipt.thread_id,
		&identity.thread_title,
	)
	.map_err(|_| ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?;
	let typed_title_request = TypedThreadNameSetRequest {
		request: ExactRpcRequestFact {
			id: TITLE_SET_REQUEST_ID,
			digest: title_request_digest.clone(),
		},
		thread_id: ExactThreadId::new(start_receipt.thread_id.clone())
			.map_err(|_| ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?,
		title: ThreadTitle::from_protocol(identity.thread_title.clone())
			.map_err(|_| ManualRetainedTitleExperimentError::InvalidRequest)?,
	};
	let title_set_response_observed = match store
		.mark_codex_experiment_title_set_possible(
			&title_fence_key,
			&FenceCodexExperimentTitleSet {
				experiment_id: identity.experiment_id.clone(),
				expected_revision: 3,
				title_attempt_id: request.title_attempt_id.clone(),
				thread_id: start_receipt.thread_id.clone(),
				request_id: typed_title_request.request.id,
				request_digest: typed_title_request.request.digest.clone(),
				requested_title: typed_title_request.title.as_str().to_owned(),
			},
		)
		.await
		.map_err(|_| ManualRetainedTitleExperimentError::ProductStateUnavailable)?
	{
		CodexExperimentTitleSetFenceOutcome::Fresh(permission) => {
			if permission.experiment_id() != identity.experiment_id
				|| permission.title_attempt_id() != request.title_attempt_id
				|| permission.thread_id() != start_receipt.thread_id
				|| permission.request_id() != typed_title_request.request.id
				|| permission.request_digest() != typed_title_request.request.digest
				|| permission.requested_title() != typed_title_request.title.as_str()
			{
				return Err(ManualRetainedTitleExperimentError::PersistenceRejected);
			}
			match process.set_retained_title(
				typed_title_request.request.id,
				typed_title_request.thread_id.as_str(),
				typed_title_request.title.as_str(),
				timeout,
			) {
				Ok(_) => true,
				Err(RpcError::MethodRejected(_)) => {
					return Err(ManualRetainedTitleExperimentError::RetainedTitleAmbiguous);
				},
				Err(RpcError::Supervision(_)) => false,
			}
		},
		CodexExperimentTitleSetFenceOutcome::ReplayedReadbackOnly { .. } => false,
		CodexExperimentTitleSetFenceOutcome::Rejected(_) => {
			return Err(ManualRetainedTitleExperimentError::PersistenceRejected);
		},
	};

	let read = process
		.read_retained_title_thread(READ_REQUEST_ID, &start_receipt.thread_id, timeout)
		.map_err(|_| ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?;
	let typed_read = typed_read(read)?;
	if typed_read.thread_id.as_str() != start_receipt.thread_id {
		return Err(ManualRetainedTitleExperimentError::RetainedTitleAmbiguous);
	}
	require_applied(
		store
			.attest_codex_experiment_retained_title(
				&attestation_key,
				&AttestCodexExperimentRetainedTitle {
					experiment_id: identity.experiment_id.clone(),
					expected_revision: 3,
					attestation_id: request.attestation_id.clone(),
					title_attempt_id: request.title_attempt_id.clone(),
					thread_id: typed_read.thread_id.as_str().to_owned(),
					read_request_id: typed_read.request.id,
					read_request_digest: typed_read.request.digest.clone(),
					read_response_id: typed_read.response.id,
					read_response_digest: typed_read.response.digest.clone(),
					returned_title: typed_read.title.as_str().to_owned(),
					returned_cwd: typed_read.cwd.as_str().to_owned(),
					returned_marker: typed_read.provenance.as_str().to_owned(),
				},
			)
			.await,
	)
	.map_err(|_| ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?;
	require_applied(
		store
			.record_attested_codex_experiment_observation(
				&observation_key,
				&RecordCodexExperimentObservation {
					experiment_id: identity.experiment_id.clone(),
					expected_revision: 3,
					attestation_id: request.attestation_id.clone(),
					observation_id: request.observation_id.clone(),
					kind: CodexExperimentObservationKind::ThreadReadItem,
					thread_id: typed_read.thread_id.as_str().to_owned(),
					marker: typed_read.provenance.as_str().to_owned(),
					source_id: format!("thread/read:{}", typed_read.response.id),
					fact_digest: typed_read.response.digest,
				},
			)
			.await,
	)?;

	process.shutdown(timeout).map_err(|_| ManualRetainedTitleExperimentError::CleanupFailed)?;
	if !store
		.account_is_ready_at_revision(&identity.account_id, identity.account_revision)
		.await
		.map_err(|_| ManualRetainedTitleExperimentError::ProductStateUnavailable)?
	{
		return Err(ManualRetainedTitleExperimentError::AccountNotReady);
	}

	Ok(ManualRetainedTitleExperimentReport {
		experiment_id: identity.experiment_id,
		thread_id: start_receipt.thread_id,
		attestation_id: request.attestation_id,
		observation_id: request.observation_id,
		title_set_response_observed,
	})
}

fn load_request(path: &Path) -> Result<ManualRequest, ManualRetainedTitleExperimentError> {
	let bytes = fs::read(path).map_err(|_| ManualRetainedTitleExperimentError::InvalidRequest)?;

	if bytes.len() > MAX_REQUEST_BYTES {
		return Err(ManualRetainedTitleExperimentError::InvalidRequest);
	}
	serde_json::from_slice(&bytes).map_err(|_| ManualRetainedTitleExperimentError::InvalidRequest)
}

fn identity(
	wire: &ExperimentIdentityWire,
) -> Result<CodexExperimentIdentity, ManualRetainedTitleExperimentError> {
	Ok(CodexExperimentIdentity {
		experiment_id: wire.experiment_id.clone(),
		managed_run_id: ManagedRunId::new(wire.managed_run_id.as_str())
			.map_err(|_| ManualRetainedTitleExperimentError::InvalidRequest)?,
		managed_run_revision: wire.managed_run_revision,
		routing_snapshot_id: wire.routing_snapshot_id.clone(),
		account_id: AccountId::new(wire.account_id.as_str())
			.map_err(|_| ManualRetainedTitleExperimentError::InvalidRequest)?,
		account_revision: wire.account_revision,
		role_profile_revision: wire.role_profile_revision,
		build_id: wire.build_id.clone(),
		repository_cwd: wire.repository_cwd.clone(),
		thread_title: wire.thread_title.clone(),
	})
}

fn validate_request(
	request: &ManualRequest,
	identity: &CodexExperimentIdentity,
) -> Result<(), ManualRetainedTitleExperimentError> {
	if !(MIN_TIMEOUT_MILLISECONDS..=MAX_TIMEOUT_MILLISECONDS)
		.contains(&request.timeout_milliseconds)
		|| !identity.thread_title.contains(&identity.retained_marker())
		|| !identity.repository_cwd.starts_with('/')
	{
		return Err(ManualRetainedTitleExperimentError::InvalidRequest);
	}
	Ok(())
}

fn idempotency_key(experiment_id: &str, operation: &str) -> String {
	format!("v22:retained-title:{experiment_id}:{operation}")
}

fn credential(
	identity: &decodex_core::PostgresIdentityConfig,
) -> Result<Option<Zeroizing<String>>, ManualRetainedTitleExperimentError> {
	match identity.credential_env_var() {
		Some(name) => env::var(name)
			.ok()
			.filter(|value| !value.is_empty())
			.map(Zeroizing::new)
			.map(Some)
			.ok_or(ManualRetainedTitleExperimentError::ProductStateUnavailable),
		None => Ok(None),
	}
}

async fn read_start_receipt(
	store: &PostgresStore,
	identity: &CodexExperimentIdentity,
	request: &ManualRequest,
) -> Result<CodexExperimentStartReceipt, ManualRetainedTitleExperimentError> {
	let marker = identity.retained_marker();
	let request_digest =
		retained_title_start_request_digest(START_REQUEST_ID, &identity.repository_cwd, &marker)
			.map_err(|_| ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?;
	store
		.read_codex_experiment_start_exact(&identity.experiment_id, &request.creation_attempt_id)
		.await
		.map_err(|_| ManualRetainedTitleExperimentError::ProductStateUnavailable)?
		.filter(|receipt| {
			receipt.start_request_id == START_REQUEST_ID
				&& receipt.start_request_digest == request_digest
				&& receipt.request_cwd == identity.repository_cwd
				&& receipt.request_marker == marker
				&& !receipt.request_ephemeral
				&& receipt.start_response_id == START_REQUEST_ID
				&& receipt.response_cwd == identity.repository_cwd
				&& receipt.response_marker == marker
				&& !receipt.response_ephemeral
				&& receipt.returned_name.is_none()
		})
		.ok_or(ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)
}

fn typed_start(
	fact: super::process::RetainedTitleStartFact,
) -> Result<TypedThreadStartResponse, ManualRetainedTitleExperimentError> {
	let thread = fact.thread;
	Ok(TypedThreadStartResponse {
		request: ExactRpcRequestFact { id: fact.wire.request_id, digest: fact.wire.request_digest },
		response: ExactRpcResponseFact {
			id: fact.wire.response_id,
			digest: fact.wire.response_digest,
		},
		thread_id: ExactThreadId::new(thread.id.as_str())
			.map_err(|_| ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?,
		returned_name: thread
			.name
			.as_ref()
			.map(|name| ThreadTitle::from_protocol(name.as_str()))
			.transpose()
			.map_err(|_| ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?,
		cwd: ThreadCwd::from_protocol(
			thread
				.cwd
				.as_ref()
				.ok_or(ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?
				.as_str(),
		)
		.map_err(|_| ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?,
		provenance: ThreadProvenance::from_protocol(
			thread
				.thread_source
				.as_ref()
				.ok_or(ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?
				.as_str(),
		)
		.map_err(|_| ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?,
		ephemeral: thread
			.ephemeral
			.ok_or(ManualRetainedTitleExperimentError::StartOutcomeAmbiguous)?,
	})
}

fn typed_read(
	fact: super::process::RetainedTitleReadFact,
) -> Result<TypedRetainedTitleReadResponse, ManualRetainedTitleExperimentError> {
	let thread = fact.thread;
	Ok(TypedRetainedTitleReadResponse {
		request: ExactRpcRequestFact { id: fact.wire.request_id, digest: fact.wire.request_digest },
		response: ExactRpcResponseFact {
			id: fact.wire.response_id,
			digest: fact.wire.response_digest,
		},
		thread_id: ExactThreadId::new(thread.id.as_str())
			.map_err(|_| ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?,
		title: ThreadTitle::from_protocol(
			thread
				.name
				.as_ref()
				.ok_or(ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?
				.as_str(),
		)
		.map_err(|_| ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?,
		cwd: ThreadCwd::from_protocol(
			thread
				.cwd
				.as_ref()
				.ok_or(ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?
				.as_str(),
		)
		.map_err(|_| ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?,
		provenance: ThreadProvenance::from_protocol(
			thread
				.thread_source
				.as_ref()
				.ok_or(ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?
				.as_str(),
		)
		.map_err(|_| ManualRetainedTitleExperimentError::RetainedTitleAmbiguous)?,
	})
}

fn require_applied<T>(
	result: Result<CodexExperimentCommandOutcome<T>, decodex_postgres::StoreError>,
) -> Result<T, ManualRetainedTitleExperimentError> {
	match result.map_err(|_| ManualRetainedTitleExperimentError::ProductStateUnavailable)? {
		CodexExperimentCommandOutcome::Applied(value) => Ok(value),
		CodexExperimentCommandOutcome::Rejected(_) =>
			Err(ManualRetainedTitleExperimentError::PersistenceRejected),
	}
}
