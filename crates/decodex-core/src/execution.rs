//! Closed execution-consumer lineage shared by routing, continuation, and provider attempts.
//!
//! These values identify one ordinary Conversation turn or one ManagedRun execution. They carry
//! no account list, credential, process, RuntimeSession creation, dispatch, retry, or lifecycle
//! authority.

use crate::{ConversationId, ManagedExecutionId, ManagedRunId, RuntimeSessionId, TurnId};

/// One exact consumer intent that V16 and V17 preserve without changing its domain owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionConsumer {
	/// One ordinary Conversation turn. Quick Task uses this variant.
	ConversationTurn {
		/// Owning logical Conversation.
		conversation_id: ConversationId,
		/// Positive Conversation revision accepted before routing.
		conversation_revision: i64,
		/// Exact existing RuntimeSession that supplies continuity and sticky affinity.
		source_runtime_session_id: RuntimeSessionId,
		/// Positive source RuntimeSession revision.
		source_runtime_session_revision: i64,
		/// Reserved ordinary Turn identity. Only the Conversation owner can materialize it.
		turn_id: TurnId,
	},
	/// One execution-scoped intent within a ManagedRun.
	ManagedRunExecution {
		/// Owning ManagedRun.
		managed_run_id: ManagedRunId,
		/// Positive immutable ManagedRun revision accepted before routing.
		managed_run_revision: i64,
		/// Distinct execution intent within the ManagedRun.
		execution_id: ManagedExecutionId,
	},
}

impl ExecutionConsumer {
	/// Return the canonical PostgreSQL consumer label.
	pub const fn as_sql(&self) -> &'static str {
		match self {
			Self::ConversationTurn { .. } => "conversation_turn",
			Self::ManagedRunExecution { .. } => "managed_run_execution",
		}
	}

	/// Return the exact optimistic domain revision.
	pub const fn domain_revision(&self) -> i64 {
		match self {
			Self::ConversationTurn { conversation_revision, .. } => *conversation_revision,
			Self::ManagedRunExecution { managed_run_revision, .. } => *managed_run_revision,
		}
	}
}
