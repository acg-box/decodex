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

/// Durable causal state. `CreationPossible` is terminal for creation authority unless the exact
/// typed response is later bound; recovery cannot turn it back into preparation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexExperimentState {
	/// Immutable intent exists, but no external thread-creation effect is yet permitted.
	Prepared,
	/// The pre-effect fence is durable; recovery treats creation as terminally ambiguous.
	CreationPossible,
	/// One exact successful typed response has bound the lineage to one owned thread.
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
	/// Return the fixed V15 PostgreSQL enum label for this positive observation kind.
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

/// Revisioned preparation effect returned by PostgreSQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentPrepared {
	/// Complete mechanism-neutral identity whose provenance PostgreSQL must independently verify.
	pub identity: CodexExperimentIdentity,
	/// Positive experiment revision returned for the initial prepared state; fixed to revision
	/// one.
	pub revision: i64,
	/// Deterministic retained marker derived from the canonical experiment identity.
	pub marker: String,
	/// PostgreSQL-owned preparation time in UTC Unix microseconds.
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
	/// PostgreSQL-owned fence time in UTC Unix microseconds.
	pub fenced_at_micros: i64,
}

/// Exact successful typed response bound to one immutable experiment lineage.
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
	/// Exact app-server response identity retained to prevent response aliasing.
	pub response_id: String,
	/// PostgreSQL-owned binding time in UTC Unix microseconds.
	pub bound_at_micros: i64,
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
	/// Closed positive fact shape; it has no absence or inferred-completion variant.
	pub kind: CodexExperimentObservationKind,
	/// Exact app-server source identity whose payload is retained by digest at persistence.
	pub source_id: String,
	/// PostgreSQL-owned observation time in UTC Unix microseconds.
	pub observed_at_micros: i64,
}

/// Closed stable domain rejection returned by a V15 exact command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentRejection {
	/// Fixed V15 command name that produced the durable stable rejection.
	pub operation: String,
	/// Closed operation-specific rejection code, not an external-effect or absence inference.
	pub code: String,
}

/// Fail-closed exact-command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexExperimentCommandOutcome<T> {
	/// Exact command effect accepted after PostgreSQL has verified and persisted its authority.
	///
	/// Constructing this Rust variant alone does not prove database authorship or authorize
	/// routing, dispatch, thread creation, retry, or adoption.
	Applied(T),
	/// Durable stable domain rejection; this shape grants no permission to infer thread absence.
	Rejected(CodexExperimentRejection),
}
