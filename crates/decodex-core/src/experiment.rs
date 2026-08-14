//! Mechanism-neutral contracts for causally owned Codex experiments.
//!
//! These values describe durable positive facts. They deliberately expose no creation retry,
//! thread adoption, negative presence, or inferred completion authority.

use crate::{AccountId, ManagedRunId};

/// Immutable identity from which the retained Codex marker is derived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentIdentity {
	/// Canonical experiment UUID text that uniquely names the immutable lineage and marker.
	pub experiment_id: String,
	/// ManagedRun whose exact durable revision authorizes this experimental lineage.
	pub managed_run_id: ManagedRunId,
	/// Positive ManagedRun revision bound at preparation; later revisions cannot be substituted.
	pub managed_run_revision: i64,
	/// Canonical V14 snapshot UUID text supplying the complete routing facts for the experiment.
	pub routing_snapshot_id: String,
	/// Exact account member retained from the bound routing snapshot.
	pub account_id: AccountId,
	/// Positive immutable account revision observed in that routing snapshot.
	pub account_revision: i64,
	/// Positive immutable RoleProfile revision required by the selected experimental lineage.
	pub role_profile_revision: i64,
	/// Exact Codex build identity required by the bound routing snapshot.
	pub build_id: String,
	/// Repository working directory expected in the typed non-ephemeral thread response.
	pub repository_cwd: String,
	/// Exact retained thread title, including the marker derived from `experiment_id`.
	pub thread_title: String,
}
impl CodexExperimentIdentity {
	/// Deterministic retained marker. Callers never supply marker authority.
	pub fn retained_marker(&self) -> String {
		format!("decodex.experiment.v1:{}", self.experiment_id)
	}
}

/// Durable causal state. `CreationPossible` never grants creation authority on replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexExperimentState {
	/// Immutable intent exists, but no external thread-creation effect is yet permitted.
	Prepared,
	/// The pre-effect fence is durable; recovery treats creation as terminally ambiguous.
	CreationPossible,
	/// One exact nullable-name `thread/start` response has bound the lineage to one owned thread.
	ThreadBound,
}

/// Positive app-server fact kinds accepted by V15.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexExperimentObservationKind {
	/// One positive matching item returned by a bounded `thread/list` observation.
	ThreadListItem,
	/// One positive exact-thread item returned by `thread/read`.
	ThreadReadItem,
	/// One positively observed event that a turn started on the bound thread.
	TurnStartedEvent,
	/// One positively observed terminal turn event, without inference from missing events.
	TurnTerminalEvent,
	/// One positively observed message item tied to the bound thread and marker.
	MessageItem,
}
impl CodexExperimentObservationKind {
	/// Return the fixed V15 durable-store enum label for this positive observation kind.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::ThreadListItem => "thread_list_item",
			Self::ThreadReadItem => "thread_read_item",
			Self::TurnStartedEvent => "turn_started_event",
			Self::TurnTerminalEvent => "turn_terminal_event",
			Self::MessageItem => "message_item",
		}
	}
}

/// Revisioned preparation effect returned by durable-store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentPrepared {
	/// Complete mechanism-neutral identity whose provenance durable-store must independently verify.
	pub identity: CodexExperimentIdentity,
	/// Positive experiment revision returned for the initial prepared state; fixed to revision
	/// one.
	pub revision: i64,
	/// Deterministic retained marker derived from the canonical experiment identity.
	pub marker: String,
	/// durable-store-owned preparation time in UTC Unix microseconds.
	pub prepared_at_micros: i64,
}

/// Revisioned pre-effect fence. This is intentionally not proof that a thread exists.
#[derive(Debug, Eq, PartialEq)]
pub struct CodexExperimentCreationPossible {
	/// Canonical experiment UUID text whose prepared intent was fenced.
	pub experiment_id: String,
	/// Positive experiment revision for the creation fence; fixed to revision two.
	pub revision: i64,
	/// Canonical UUID text identifying the sole fenced creation attempt.
	pub attempt_id: String,
	/// durable-store-owned fence time in UTC Unix microseconds.
	pub fenced_at_micros: i64,
}

/// Exact successful nullable-name `thread/start` response bound to one experiment lineage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentThreadBinding {
	/// Canonical experiment UUID text owning the exact thread binding.
	pub experiment_id: String,
	/// Positive bound experiment revision; fixed to revision three.
	pub revision: i64,
	/// Exact creation-attempt UUID text consumed by the successful typed response.
	pub attempt_id: String,
	/// Exact Codex thread identity returned by that response; no searched thread may substitute.
	pub thread_id: String,
	/// Exact numeric JSON-RPC request identity retained for the one external start effect.
	pub start_request_id: i64,
	/// Lowercase SHA-256 of the exact serialized `thread/start` request frame.
	pub start_request_digest: String,
	/// Exact prepared repository working directory sent by `thread/start`.
	pub request_cwd: String,
	/// Exact prepared provenance marker sent by `thread/start`.
	pub request_marker: String,
	/// Request ephemeral flag, fixed to false.
	pub request_ephemeral: bool,
	/// Exact numeric JSON-RPC response identity. It equals `start_request_id`.
	pub start_response_id: i64,
	/// Lowercase SHA-256 of the exact raw `thread/start` response frame.
	pub start_response_digest: String,
	/// Exact prepared repository working directory returned by `thread/start`.
	pub response_cwd: String,
	/// Exact prepared provenance marker returned by `thread/start`.
	pub response_marker: String,
	/// Response ephemeral flag, fixed to false.
	pub response_ephemeral: bool,
	/// Nullable name returned by `thread/start`. The pinned build requires `None`.
	pub returned_name: Option<String>,
	/// durable-store-owned binding time in UTC Unix microseconds.
	pub bound_at_micros: i64,
}

/// Durable one-shot pre-effect fence for `thread/name/set`.
#[derive(Debug, Eq, PartialEq)]
pub struct CodexExperimentTitleSetPossible {
	/// Canonical experiment UUID text whose exact start binding owns this title effect.
	pub experiment_id: String,
	/// Exact experiment revision, fixed at three.
	pub experiment_revision: i64,
	/// Canonical UUID text identifying the sole title-set attempt.
	pub title_attempt_id: String,
	/// Exact thread identity returned by the bound `thread/start` response.
	pub thread_id: String,
	/// Exact numeric JSON-RPC identity for the one allowed `thread/name/set` request.
	pub request_id: i64,
	/// Lowercase SHA-256 of the exact serialized `thread/name/set` request frame.
	pub request_digest: String,
	/// Exact immutable prepared title bound into the fenced request.
	pub requested_title: String,
	/// durable-store-owned fence time in UTC Unix microseconds.
	pub fenced_at_micros: i64,
}

/// Positive exact-ID `thread/read` attestation for the retained prepared title.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentRetainedTitleAttestation {
	/// Canonical experiment UUID text that owns the attestation.
	pub experiment_id: String,
	/// Canonical UUID text identifying this immutable attestation.
	pub attestation_id: String,
	/// Canonical UUID text identifying the fenced title-set effect.
	pub title_attempt_id: String,
	/// Exact thread identity read after the title-set fence.
	pub thread_id: String,
	/// Exact numeric JSON-RPC identity for the exact-ID read request.
	pub read_request_id: i64,
	/// Lowercase SHA-256 of the exact serialized `thread/read` request frame.
	pub read_request_digest: String,
	/// Exact numeric JSON-RPC response identity. It equals `read_request_id`.
	pub read_response_id: i64,
	/// Lowercase SHA-256 of the exact raw `thread/read` response frame.
	pub read_response_digest: String,
	/// Exact prepared title returned by the positive readback.
	pub retained_title: String,
	/// Exact prepared repository working directory returned by the positive readback.
	pub returned_cwd: String,
	/// Exact retained provenance marker returned by the positive readback.
	pub marker: String,
	/// durable-store-owned attestation time in UTC Unix microseconds.
	pub attested_at_micros: i64,
}

/// One append-only positive exact observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentObservation {
	/// Canonical experiment UUID text owning this positive fact.
	pub experiment_id: String,
	/// Exact positive experiment revision to which the observation is bound; fixed to revision
	/// three.
	pub experiment_revision: i64,
	/// Canonical UUID text uniquely identifying this append-only observation.
	pub observation_id: String,
	/// Exact retained-title attestation that authorizes this title-qualified observation.
	pub attestation_id: String,
	/// Closed positive fact shape; it has no absence or inferred-completion variant.
	pub kind: CodexExperimentObservationKind,
	/// Exact app-server source identity whose payload is retained by digest at persistence.
	pub source_id: String,
	/// durable-store-owned observation time in UTC Unix microseconds.
	pub observed_at_micros: i64,
}

/// Closed stable domain rejection returned by a V15 or V22 exact command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentRejection {
	/// Fixed experiment command name that produced the durable stable rejection.
	pub operation: String,
	/// Closed operation-specific rejection code, not an external-effect or absence inference.
	pub code: String,
}

/// Fail-closed exact-command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexExperimentCommandOutcome<T> {
	/// Exact command effect accepted after durable-store has verified and persisted its authority.
	///
	/// Constructing this Rust variant alone does not prove database authorship or authorize
	/// routing, dispatch, thread creation, retry, or adoption.
	Applied(T),
	/// Durable stable domain rejection; this shape grants no permission to infer thread absence.
	Rejected(CodexExperimentRejection),
}
