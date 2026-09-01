//! Closed execution-consumer lineage shared by routing, continuation, and provider attempts.
//!
//! These values identify one ordinary Conversation turn or one ManagedRun execution. They carry
//! no account list, credential, process, RuntimeSession creation, dispatch, retry, or lifecycle
//! authority.

use crate::{ConversationId, ManagedExecutionId, ManagedRunId, RuntimeSessionId, TurnId};

/// One exact consumer intent that Routing Decision and Continuation Plan preserve without changing
/// its domain owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionConsumer {
	/// One ordinary Conversation turn. Conversation uses this variant.
	ConversationTurn {
		/// Owning logical Conversation.
		conversation_id: ConversationId,
		/// Positive Conversation revision accepted before routing.
		conversation_revision: i64,
		/// Existing RuntimeSession for continuation routing, absent for an initial Turn intent.
		source_runtime_session_id: Option<RuntimeSessionId>,
		/// Positive source RuntimeSession revision, jointly absent for an initial Turn intent.
		source_runtime_session_revision: Option<i64>,
		/// Prospective ordinary Turn identity. Only the Conversation owner can materialize it.
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
	/// Return the canonical durable-store consumer label.
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
