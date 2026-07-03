//! App-server dynamic tool declaration, completion, and call dispatch.

pub(in crate::agent::app_server) mod dispatch;
pub(in crate::agent::app_server) mod failure;

mod completion;
mod validation;

pub(super) use self::{
	completion::{
		classify_turn_completion, has_terminal_completion_signal,
		reject_nonterminal_single_turn_completion,
	},
	dispatch::{
		dispatch_dynamic_tool_call, dynamic_tool_call_unavailable_for_phase,
		respond_to_dynamic_tool_call_dispatch,
	},
	validation::validated_dynamic_tool_specs,
};
