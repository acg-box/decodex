//! Recap-specific budget for the shared public-message excerpt renderer.
pub(super) use crate::agent_message_history::excerpts::{Exchange, render_budget};
pub(super) fn render(exchanges: &[Exchange]) -> String {
	render_budget(exchanges, super::prompt::HISTORY_MAX_BYTES)
}
