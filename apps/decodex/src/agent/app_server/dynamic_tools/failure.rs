use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppServerDynamicToolFailureKind {
	Protocol,
	Tool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerDynamicToolFailure {
	kind: AppServerDynamicToolFailureKind,
	pub(in crate::agent::app_server::dynamic_tools) tool: Option<String>,
	message: String,
}
impl AppServerDynamicToolFailure {
	pub(in crate::agent::app_server::dynamic_tools) fn protocol(
		tool: Option<String>,
		message: impl Into<String>,
	) -> Self {
		Self { kind: AppServerDynamicToolFailureKind::Protocol, tool, message: message.into() }
	}

	pub(in crate::agent::app_server::dynamic_tools) fn tool(
		tool: Option<String>,
		message: impl Into<String>,
	) -> Self {
		Self { kind: AppServerDynamicToolFailureKind::Tool, tool, message: message.into() }
	}

	#[cfg(test)]
	pub(crate) fn protocol_for_test(tool: Option<String>, message: impl Into<String>) -> Self {
		Self::protocol(tool, message)
	}

	#[cfg(test)]
	pub(crate) fn tool_for_test(tool: Option<String>, message: impl Into<String>) -> Self {
		Self::tool(tool, message)
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match self.kind {
			AppServerDynamicToolFailureKind::Protocol => "app_server_dynamic_tool_protocol_failure",
			AppServerDynamicToolFailureKind::Tool => "app_server_dynamic_tool_failed",
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.kind {
			AppServerDynamicToolFailureKind::Protocol => format!(
				"inspect the app-server dynamic tool declaration and `item/tool/call` payload, repair the protocol mismatch manually, {recovery_gate}"
			),
			AppServerDynamicToolFailureKind::Tool => format!(
				"inspect the dynamic tool response and lane state, correct the tool call or underlying service state manually, {recovery_gate}"
			),
		}
	}

	pub(crate) fn retry_next_action(&self) -> String {
		format!("decodex will retry automatically; {}", self.diagnostic_next_action())
	}

	pub(in crate::agent::app_server::dynamic_tools) fn diagnostic_next_action(
		&self,
	) -> &'static str {
		match self.kind {
			AppServerDynamicToolFailureKind::Protocol =>
				"inspect the declared dynamic tool surface and item/tool/call payload before retrying the lane",
			AppServerDynamicToolFailureKind::Tool =>
				"inspect the tool response, correct the call arguments or backing state, and retry the tool call",
		}
	}

	pub(in crate::agent::app_server::dynamic_tools) fn message(&self) -> &str {
		&self.message
	}
}

impl Display for AppServerDynamicToolFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "app_server_dynamic_tool_failure: {}", self.message)?;

		if let Some(tool) = self.tool.as_deref() {
			write!(formatter, " (tool `{tool}`)")?;
		}

		Ok(())
	}
}

impl Error for AppServerDynamicToolFailure {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct DynamicToolFailureDiagnostic {
	pub(in crate::agent::app_server) failure_class: &'static str,
	pub(in crate::agent::app_server) tool: Option<String>,
	pub(in crate::agent::app_server) namespace: Option<String>,
	pub(in crate::agent::app_server) message: String,
	pub(in crate::agent::app_server) next_action: &'static str,
}
impl DynamicToolFailureDiagnostic {
	pub(in crate::agent::app_server::dynamic_tools) fn from_failure(
		failure: &AppServerDynamicToolFailure,
		namespace: Option<String>,
	) -> Self {
		Self {
			failure_class: failure.error_class(),
			tool: failure.tool.clone(),
			namespace,
			message: failure.message().to_owned(),
			next_action: failure.diagnostic_next_action(),
		}
	}
}
