use std::env;

use serde_json::Value;

use crate::{
	mcp,
	mcp::http::{
		MCP_AUTHORIZATION_HEADER, MCP_WWW_AUTHENTICATE_HEADER,
		message::{McpHttpRequest, McpHttpResponse},
	},
	prelude::{Result, eyre},
};

#[derive(Clone, Default)]
pub(in crate::mcp) struct McpHttpAuthorization {
	token: Option<String>,
}
impl McpHttpAuthorization {
	pub(in crate::mcp) fn disabled() -> Self {
		Self { token: None }
	}

	pub(in crate::mcp) fn from_env_var_name(env_var: Option<&str>) -> Result<Self> {
		let Some(env_var) = env_var else {
			return Ok(Self::disabled());
		};

		validate_mcp_bearer_token_env_var_name(env_var)?;

		let token = env::var(env_var).map_err(|_| {
			eyre::eyre!(
				"Streamable HTTP bearer token env var `{env_var}` is not set; set it or remove --bearer-token-env."
			)
		})?;

		validate_mcp_bearer_token(&token, env_var)?;

		Ok(Self { token: Some(token) })
	}

	pub(super) fn is_required(&self) -> bool {
		self.token.is_some()
	}

	pub(super) fn request_is_authorized(&self, request: &McpHttpRequest) -> bool {
		let Some(expected) = self.token.as_deref() else {
			return true;
		};
		let Some(header) = request.header(MCP_AUTHORIZATION_HEADER) else {
			return false;
		};
		let Some((scheme, supplied)) = header.trim().split_once(' ') else {
			return false;
		};

		scheme.eq_ignore_ascii_case("Bearer") && supplied == expected
	}

	pub(super) fn unauthorized_response() -> McpHttpResponse {
		let mut response = McpHttpResponse::json_error(
			"401 Unauthorized",
			mcp::json_rpc_error(Value::Null, -32_000, "Unauthorized"),
		);

		response.headers.push(("WWW-Authenticate", String::from(MCP_WWW_AUTHENTICATE_HEADER)));

		response
	}

	#[cfg(test)]
	pub(in crate::mcp) fn from_token_for_test(token: &str) -> Self {
		Self { token: Some(token.to_owned()) }
	}
}

fn validate_mcp_bearer_token_env_var_name(env_var: &str) -> Result<()> {
	if env_var.is_empty() || env_var.trim() != env_var {
		eyre::bail!("--bearer-token-env must name a non-empty environment variable.");
	}

	let mut chars = env_var.chars();
	let Some(first) = chars.next() else {
		eyre::bail!("--bearer-token-env must name a non-empty environment variable.");
	};

	if !(first.is_ascii_alphabetic() || first == '_')
		|| !chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
	{
		eyre::bail!(
			"--bearer-token-env must start with an ASCII letter or underscore and contain only ASCII letters, digits, or underscores."
		);
	}

	Ok(())
}

fn validate_mcp_bearer_token(token: &str, env_var: &str) -> Result<()> {
	if token.is_empty() || token.trim().is_empty() {
		eyre::bail!("Streamable HTTP bearer token env var `{env_var}` is empty.");
	}
	if token.chars().any(char::is_whitespace) {
		eyre::bail!(
			"Streamable HTTP bearer token env var `{env_var}` must not contain whitespace."
		);
	}

	Ok(())
}
