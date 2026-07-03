//! Local execution-ledger projection for operator status snapshots.

mod local;
mod predicates;
mod terminal;

pub(super) use self::{
	local::{
		current_lane_terminal_projection_from_local_ledger, hydrate_history_lanes_from_local_ledger,
	},
	predicates::{
		current_lane_has_authoritative_live_owner, history_lane_group_key,
		history_ledger_outcome_is_terminal, history_ledger_outcome_requires_attention,
	},
	terminal::{
		apply_operator_lane_terminal_projection, apply_terminal_history_ledger_outcome_to_run,
		apply_terminal_history_ledger_outcomes, suppress_terminal_attention_queue_echoes,
	},
};
