use crate::agent::app_server::{CodexAccountProvider, DynamicToolHandler};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RequestWaitPhase {
	Initialize,
	AccountLogin,
	ThreadStart,
	ThreadResume,
	TurnStart,
	TurnExecution,
}
impl RequestWaitPhase {
	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Initialize => "initialize",
			Self::AccountLogin => "account/login/start",
			Self::ThreadStart => "thread/start",
			Self::ThreadResume => "thread/resume",
			Self::TurnStart => "turn/start",
			Self::TurnExecution => "turn execution",
		}
	}

	pub(crate) fn transport_failure_is_retryable_startup(self) -> bool {
		matches!(
			self,
			Self::Initialize | Self::AccountLogin | Self::ThreadStart | Self::ThreadResume
		)
	}
}

#[derive(Clone, Copy)]
pub(crate) struct RequestDispatchContext<'a> {
	pub(crate) phase: RequestWaitPhase,
	pub(crate) dynamic_tool_handler: Option<&'a dyn DynamicToolHandler>,
	pub(crate) codex_account_provider: Option<&'a dyn CodexAccountProvider>,
	pub(crate) target_thread_id: Option<&'a str>,
	pub(crate) target_turn_id: Option<&'a str>,
}
impl<'a> RequestDispatchContext<'a> {
	pub(crate) fn new(
		phase: RequestWaitPhase,
		dynamic_tool_handler: Option<&'a dyn DynamicToolHandler>,
		codex_account_provider: Option<&'a dyn CodexAccountProvider>,
		target_thread_id: Option<&'a str>,
		target_turn_id: Option<&'a str>,
	) -> Self {
		Self {
			phase,
			dynamic_tool_handler,
			codex_account_provider,
			target_thread_id,
			target_turn_id,
		}
	}
}
