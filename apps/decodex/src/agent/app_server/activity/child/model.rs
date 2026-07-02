#[derive(Clone, Debug)]
pub(in crate::agent::app_server::activity::child) struct ChildActivityEvent {
	pub(in crate::agent::app_server::activity::child) event_bucket: String,
	pub(in crate::agent::app_server::activity::child) event_detail: Option<String>,
	pub(in crate::agent::app_server::activity::child) transition_bucket: Option<String>,
	pub(in crate::agent::app_server::activity::child) transition_detail: Option<String>,
	pub(in crate::agent::app_server::activity::child) tool_name: Option<String>,
	pub(in crate::agent::app_server::activity::child) tool_call: bool,
	pub(in crate::agent::app_server::activity::child) tool_output_bytes: Option<i64>,
	pub(in crate::agent::app_server::activity::child) input_tokens: Option<i64>,
	pub(in crate::agent::app_server::activity::child) output_tokens: Option<i64>,
	pub(in crate::agent::app_server::activity::child) completed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::agent::app_server::activity::child) struct LargeOutputStats {
	pub(in crate::agent::app_server::activity::child) count: i64,
	pub(in crate::agent::app_server::activity::child) max_bytes: i64,
}
