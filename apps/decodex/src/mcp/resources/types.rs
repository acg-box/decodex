use reqwest::Url;
use serde::Serialize;
use serde_json::{self, Value};

use crate::mcp::{McpError, observability};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct McpResource {
	uri: String,
	name: String,
	description: String,
	mime_type: String,
}
impl McpResource {
	pub(super) fn json(
		uri: impl Into<String>,
		name: impl Into<String>,
		description: impl Into<String>,
	) -> Self {
		Self {
			uri: uri.into(),
			name: name.into(),
			description: description.into(),
			mime_type: String::from("application/json"),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ResourceUri {
	pub(super) raw: String,
	pub(super) host: String,
	pub(super) segments: Vec<String>,
}
impl ResourceUri {
	pub(super) fn parse(uri: &str) -> Result<Self, McpError> {
		let parsed = Url::parse(uri).map_err(|_| McpError::invalid_params())?;

		if parsed.scheme() != "decodex" {
			return Err(McpError::invalid_params());
		}

		let host = parsed.host_str().map(str::to_owned).ok_or_else(McpError::invalid_params)?;
		let segments = parsed
			.path_segments()
			.map(|segments| {
				segments
					.filter(|segment| !segment.is_empty())
					.map(str::to_owned)
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();

		Ok(Self { raw: uri.to_owned(), host, segments })
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mcp) struct ResourceContent {
	pub(in crate::mcp) uri: String,
	pub(in crate::mcp) mime_type: String,
	pub(in crate::mcp) text: String,
}
impl ResourceContent {
	pub(super) fn json(uri: &str, value: Value) -> Result<Self, McpError> {
		let text = serde_json::to_string_pretty(&value).map_err(McpError::internal)?;

		Ok(Self { uri: uri.to_owned(), mime_type: String::from("application/json"), text })
	}

	pub(in crate::mcp) fn mcp_observability_json(
		uri: &str,
		mut value: Value,
	) -> Result<Self, McpError> {
		observability::sanitize_mcp_observability_value(&mut value);

		Self::json(uri, value)
	}
}
