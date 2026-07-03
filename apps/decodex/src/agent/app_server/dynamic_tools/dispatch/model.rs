use crate::agent::app_server::{
	DynamicToolCallResponse, DynamicToolContentItem,
	dynamic_tools::failure::{AppServerDynamicToolFailure, DynamicToolFailureDiagnostic},
};

#[derive(Debug)]
pub(in crate::agent::app_server) struct DynamicToolCallDispatch {
	pub(in crate::agent::app_server) response: DynamicToolCallResponse,
	pub(in crate::agent::app_server) diagnostic: Option<DynamicToolFailureDiagnostic>,
	pub(in crate::agent::app_server) terminal_failure: Option<AppServerDynamicToolFailure>,
}
impl DynamicToolCallDispatch {
	pub(in crate::agent::app_server::dynamic_tools) fn success(
		response: DynamicToolCallResponse,
	) -> Self {
		Self { response, diagnostic: None, terminal_failure: None }
	}

	pub(in crate::agent::app_server::dynamic_tools) fn tool_failure(
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

	pub(in crate::agent::app_server::dynamic_tools) fn protocol_failure(
		tool: Option<String>,
		namespace: Option<String>,
		message: String,
	) -> Self {
		let failure = AppServerDynamicToolFailure::protocol(tool, message.clone());

		Self {
			response: DynamicToolCallResponse::failure(message),
			diagnostic: Some(DynamicToolFailureDiagnostic::from_failure(&failure, namespace)),
			terminal_failure: Some(failure),
		}
	}
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
