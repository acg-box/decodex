mod accounts;
mod child_agent;
mod loop_status;
mod protocol;
mod time;

pub(crate) use self::{
	accounts::{render_account_summary, render_accounts_summary},
	child_agent::{render_child_agent_activity_summary, render_child_agent_context_pressure},
	loop_status::{
		render_control_capability_summary, render_loop_architecture_recovery_summary,
		render_loop_autonomy_signals_summary, render_loop_boundary_summary,
		render_loop_review_summary, render_loop_status_summary,
	},
	protocol::render_protocol_activity_summary,
	time::format_seconds_compact,
};
