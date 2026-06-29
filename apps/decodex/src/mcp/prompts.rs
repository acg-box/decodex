mod arguments;
mod catalog;
mod render;

use serde::Deserialize;
use serde_json::{self, Value};

use crate::{
	mcp::{McpError, McpServer},
	prelude::Result,
};

#[derive(Deserialize)]
struct GetPromptParams {
	name: String,
	arguments: Option<Value>,
}

impl McpServer {
	pub(super) fn list_prompts(&self) -> Value {
		serde_json::json!({ "prompts": catalog::mcp_prompts() })
	}

	pub(super) fn get_prompt(&self, params: Option<Value>) -> Result<Value, McpError> {
		let params = params.ok_or_else(McpError::invalid_params)?;
		let params = serde_json::from_value::<GetPromptParams>(params)
			.map_err(|_| McpError::invalid_params())?;
		let arguments = params.arguments.unwrap_or_default();

		if !arguments::prompt_required_arguments_are_present(&params.name, &arguments) {
			return Err(McpError::invalid_params());
		}

		render::mcp_prompt_result(&params.name, arguments).ok_or_else(McpError::invalid_params)
	}
}
