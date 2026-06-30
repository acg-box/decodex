//! App-server dynamic tool declaration, completion, and call dispatch.

use super::{
	Display, DynamicToolCallParams, DynamicToolCallResponse, DynamicToolContentItem,
	DynamicToolHandler, DynamicToolSpec, Error, Formatter, JsonRpcConnection, JsonRpcRequest,
	RequestDispatchContext, RequestWaitPhase, RunRecorder, Serialize, TurnCompletionStatus, eyre,
	fmt, record_server_request_response, serde_json, tracker_tool_bridge,
};
use color_eyre::eyre::Report;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppServerDynamicToolFailureKind {
	Protocol,
	Tool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerDynamicToolFailure {
	kind: AppServerDynamicToolFailureKind,
	tool: Option<String>,
	message: String,
}
impl AppServerDynamicToolFailure {
	fn protocol(tool: Option<String>, message: impl Into<String>) -> Self {
		Self { kind: AppServerDynamicToolFailureKind::Protocol, tool, message: message.into() }
	}

	fn tool(tool: Option<String>, message: impl Into<String>) -> Self {
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

	fn diagnostic_next_action(&self) -> &'static str {
		match self.kind {
			AppServerDynamicToolFailureKind::Protocol =>
				"inspect the declared dynamic tool surface and item/tool/call payload before retrying the lane",
			AppServerDynamicToolFailureKind::Tool =>
				"inspect the tool response, correct the call arguments or backing state, and retry the tool call",
		}
	}
}

impl Display for AppServerDynamicToolFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		write!(formatter, "app_server_dynamic_tool_failure: {}", self.message)?;

		if let Some(tool) = self.tool.as_deref() {
			write!(formatter, " (tool `{tool}`)")?;
		}

		Ok(())
	}
}

impl Error for AppServerDynamicToolFailure {}

#[derive(Debug)]
pub(super) struct DynamicToolCallDispatch {
	pub(super) response: DynamicToolCallResponse,
	pub(super) diagnostic: Option<DynamicToolFailureDiagnostic>,
	pub(super) terminal_failure: Option<AppServerDynamicToolFailure>,
}
impl DynamicToolCallDispatch {
	fn success(response: DynamicToolCallResponse) -> Self {
		Self { response, diagnostic: None, terminal_failure: None }
	}

	fn tool_failure(
		response: DynamicToolCallResponse,
		tool: Option<String>,
		namespace: Option<String>,
	) -> Self {
		let message = dynamic_tool_response_text(&response);
		let failure = AppServerDynamicToolFailure::tool(tool.clone(), message.clone());

		Self {
			response,
			diagnostic: Some(DynamicToolFailureDiagnostic::from_failure(&failure, namespace)),
			terminal_failure: None,
		}
	}

	fn protocol_failure(tool: Option<String>, namespace: Option<String>, message: String) -> Self {
		let failure = AppServerDynamicToolFailure::protocol(tool, message.clone());

		Self {
			response: DynamicToolCallResponse::failure(message),
			diagnostic: Some(DynamicToolFailureDiagnostic::from_failure(&failure, namespace)),
			terminal_failure: Some(failure),
		}
	}
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DynamicToolFailureDiagnostic {
	pub(super) failure_class: &'static str,
	pub(super) tool: Option<String>,
	pub(super) namespace: Option<String>,
	pub(super) message: String,
	pub(super) next_action: &'static str,
}
impl DynamicToolFailureDiagnostic {
	fn from_failure(failure: &AppServerDynamicToolFailure, namespace: Option<String>) -> Self {
		Self {
			failure_class: failure.error_class(),
			tool: failure.tool.clone(),
			namespace,
			message: failure.message.clone(),
			next_action: failure.diagnostic_next_action(),
		}
	}
}

pub(super) fn validated_dynamic_tool_specs(
	handler: &dyn DynamicToolHandler,
) -> crate::prelude::Result<Vec<DynamicToolSpec>> {
	let tool_specs = handler.tool_specs();

	for spec in &tool_specs {
		if !tracker_tool_bridge::dynamic_tool_identifier_is_valid(&spec.name) {
			return Err(Report::new(AppServerDynamicToolFailure::protocol(
				Some(spec.name.clone()),
				format!(
					"Dynamic tool name `{}` does not match the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`.",
					spec.name
				),
			)));
		}

		if let Some(namespace) = spec.namespace.as_deref()
			&& !tracker_tool_bridge::dynamic_tool_identifier_is_valid(namespace)
		{
			return Err(Report::new(AppServerDynamicToolFailure::protocol(
				Some(format!("{namespace}.{}", spec.name)),
				format!(
					"Dynamic tool namespace `{namespace}` does not match the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`."
				),
			)));
		}
	}

	Ok(tool_specs)
}

pub(super) fn classify_turn_completion(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	final_output: &str,
) -> crate::prelude::Result<TurnCompletionStatus> {
	if let Some(dynamic_tool_handler) = dynamic_tool_handler {
		return dynamic_tool_handler.classify_turn_completion(final_output);
	}

	Ok(TurnCompletionStatus::Complete)
}

pub(super) fn has_terminal_completion_signal(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
) -> bool {
	dynamic_tool_handler.is_some_and(DynamicToolHandler::has_terminal_completion_signal)
}

pub(super) fn reject_nonterminal_single_turn_completion(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	final_output: &str,
) -> crate::prelude::Result<()> {
	if let Some(dynamic_tool_handler) = dynamic_tool_handler {
		dynamic_tool_handler.validate_turn_completion(final_output)?;
	}

	eyre::bail!(
		"Turn completed without a terminal completion path while same-thread continuation is disabled."
	);
}

pub(super) fn dispatch_dynamic_tool_call(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	let target_thread_id = context.target_thread_id.ok_or_else(|| {
		eyre::eyre!("app_server_protocol_failure: turn execution request missing thread context")
	})?;
	let dispatch = handle_dynamic_tool_call(
		context.dynamic_tool_handler,
		request,
		target_thread_id,
		context.target_turn_id,
	);

	respond_to_dynamic_tool_call_dispatch(connection, recorder, request, dispatch)
}

pub(super) fn dynamic_tool_call_unavailable_for_phase(
	phase: RequestWaitPhase,
) -> DynamicToolCallDispatch {
	DynamicToolCallDispatch::protocol_failure(
		None,
		None,
		format!("Dynamic tool calls are unavailable while waiting for {}.", phase.label()),
	)
}

pub(super) fn respond_to_dynamic_tool_call_dispatch(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	dispatch: DynamicToolCallDispatch,
) -> crate::prelude::Result<()> {
	record_server_request_response(
		connection,
		recorder,
		request,
		"item/tool/call/response",
		&dispatch.response,
	)?;

	if let Some(diagnostic) = dispatch.diagnostic.as_ref() {
		tracing::warn!(
			failure_class = diagnostic.failure_class,
			tool = diagnostic.tool.as_deref().unwrap_or("unknown"),
			next_action = diagnostic.next_action,
			message = diagnostic.message,
			"Dynamic tool call failed."
		);

		recorder.record("item/tool/call/failure", &serde_json::to_string(diagnostic)?)?;
	}
	if let Some(terminal_failure) = dispatch.terminal_failure {
		return Err(Report::new(terminal_failure));
	}

	Ok(())
}

pub(super) fn handle_dynamic_tool_call(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: Option<&str>,
) -> DynamicToolCallDispatch {
	let payload =
		match validated_dynamic_tool_call_payload(request, target_thread_id, target_turn_id) {
			Ok(payload) => payload,
			Err(dispatch) => return *dispatch,
		};
	let Some(dynamic_tool_handler) = dynamic_tool_handler else {
		return DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from("Dynamic tool bridge is unavailable for this run attempt."),
		);
	};
	let tool_specs = dynamic_tool_handler.tool_specs();
	let spec_matches_namespace = tool_specs.iter().any(|spec| {
		spec.name == payload.tool && spec.namespace.as_deref() == payload.namespace.as_deref()
	});

	if !spec_matches_namespace {
		let message = match payload.namespace.as_deref() {
			Some(namespace) => format!(
				"Dynamic tool `{}` was called under namespace `{namespace}`, but this run did not declare that tool namespace.",
				payload.tool
			),
			None => {
				format!("Dynamic tool `{}` is not declared for this run attempt.", payload.tool)
			},
		};

		return DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			message,
		);
	}

	let response = dynamic_tool_handler.handle_call_with_namespace(
		payload.namespace.as_deref(),
		&payload.tool,
		payload.arguments,
	);

	if let Err(message) = validate_dynamic_tool_call_response(&response) {
		return DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			message,
		);
	}

	if !response.success {
		return DynamicToolCallDispatch::tool_failure(
			response,
			Some(payload.tool),
			payload.namespace,
		);
	}

	DynamicToolCallDispatch::success(response)
}

fn validated_dynamic_tool_call_payload(
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: Option<&str>,
) -> std::result::Result<DynamicToolCallParams, Box<DynamicToolCallDispatch>> {
	let payload = serde_json::from_value::<DynamicToolCallParams>(request.params.clone()).map_err(
		|error| {
			Box::new(DynamicToolCallDispatch::protocol_failure(
				None,
				None,
				format!("Invalid `item/tool/call` payload: {error}"),
			))
		},
	)?;

	if payload.call_id.trim().is_empty() {
		return Err(Box::new(DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from("Dynamic tool call payload included an empty `callId`."),
		)));
	}
	if !tracker_tool_bridge::dynamic_tool_identifier_is_valid(&payload.tool) {
		return Err(Box::new(DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from(
				"Dynamic tool call payload included a tool name outside the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`.",
			),
		)));
	}

	if let Some(namespace) = payload.namespace.as_deref()
		&& !tracker_tool_bridge::dynamic_tool_identifier_is_valid(namespace)
	{
		return Err(Box::new(DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from(
				"Dynamic tool call payload included a namespace outside the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`.",
			),
		)));
	}

	if payload.thread_id != target_thread_id {
		return Err(Box::new(DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			format!(
				"Dynamic tool call targeted thread `{}`, but the active thread is `{target_thread_id}`.",
				payload.thread_id
			),
		)));
	}

	if let Some(target_turn_id) = target_turn_id
		&& payload.turn_id != target_turn_id
	{
		tracing::warn!(
			target_thread_id,
			target_turn_id,
			payload_thread_id = payload.thread_id.as_str(),
			payload_turn_id = payload.turn_id.as_str(),
			tool = payload.tool.as_str(),
			namespace = payload.namespace.as_deref().unwrap_or(""),
			"Dynamic tool call turn id differed from the active turn; accepting thread-bound request."
		);
	}

	Ok(payload)
}

fn validate_dynamic_tool_call_response(response: &DynamicToolCallResponse) -> Result<(), String> {
	if response.content_items.is_empty() {
		return Err(String::from(
			"Dynamic tool handler returned an invalid response with no `contentItems`.",
		));
	}

	Ok(())
}

fn dynamic_tool_response_text(response: &DynamicToolCallResponse) -> String {
	let text_items = response
		.content_items
		.iter()
		.map(|item| match item {
			DynamicToolContentItem::InputText { text } => text.trim(),
		})
		.filter(|text| !text.is_empty())
		.collect::<Vec<_>>();

	if text_items.is_empty() {
		String::from("Dynamic tool call failed without a text response.")
	} else {
		text_items.join("\n")
	}
}
