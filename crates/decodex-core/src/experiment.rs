//! Mechanism-neutral contracts for causally owned Codex experiments.
//!
//! These values describe durable positive facts. They deliberately expose no creation retry,
//! thread adoption, negative presence, or inferred completion authority.

use crate::{AccountId, ManagedRunId};

/// Immutable identity from which the retained Codex marker is derived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentIdentity {
	pub experiment_id: String,
	pub managed_run_id: ManagedRunId,
	pub managed_run_revision: i64,
	pub routing_snapshot_id: String,
	pub account_id: AccountId,
	pub account_revision: i64,
	pub role_profile_revision: i64,
	pub build_id: String,
	pub repository_cwd: String,
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
	Prepared,
	CreationPossible,
	ThreadBound,
}

/// Positive app-server fact kinds accepted by V15.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexExperimentObservationKind {
	ThreadListItem,
	ThreadReadItem,
	TurnStartedEvent,
	TurnTerminalEvent,
	MessageItem,
}
impl CodexExperimentObservationKind {
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
	pub identity: CodexExperimentIdentity,
	pub revision: i64,
	pub marker: String,
	pub prepared_at_micros: i64,
}

/// Revisioned pre-effect fence. This is intentionally not proof that a thread exists.
#[derive(Debug, Eq, PartialEq)]
pub struct CodexExperimentCreationPossible {
	pub experiment_id: String,
	pub revision: i64,
	pub attempt_id: String,
	pub fenced_at_micros: i64,
}

/// Exact successful typed response bound to one immutable experiment lineage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentThreadBinding {
	pub experiment_id: String,
	pub revision: i64,
	pub attempt_id: String,
	pub thread_id: String,
	pub response_id: String,
	pub bound_at_micros: i64,
}

/// One append-only positive exact observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentObservation {
	pub experiment_id: String,
	pub experiment_revision: i64,
	pub observation_id: String,
	pub kind: CodexExperimentObservationKind,
	pub source_id: String,
	pub observed_at_micros: i64,
}

/// Closed stable domain rejection returned by a V15 exact command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExperimentRejection {
	pub operation: String,
	pub code: String,
}

/// Fail-closed exact-command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexExperimentCommandOutcome<T> {
	Applied(T),
	Rejected(CodexExperimentRejection),
}
