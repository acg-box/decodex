use clap::{Args, Subcommand};

use crate::{
	cli::ProjectConfigArgs,
	mcp::{self, McpCapabilityProfile, McpServeRequest, McpTransport},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct McpCommand {
	#[command(subcommand)]
	pub(in crate::cli) command: McpSubcommand,
}
impl McpCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		match &self.command {
			McpSubcommand::Serve(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct McpServeCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	/// MCP transport.
	#[arg(long, value_enum, default_value_t = McpTransport::Stdio)]
	pub(in crate::cli) transport: McpTransport,
	/// Capability profile exposed by the MCP gateway. Defaults to admin for stdio and observe for
	/// Streamable HTTP.
	#[arg(long, value_enum)]
	pub(in crate::cli) capability_profile: Option<McpCapabilityProfile>,
	/// Streamable HTTP listen address.
	#[arg(long, value_name = "ADDR", default_value_t = mcp::DEFAULT_MCP_HTTP_LISTEN_ADDRESS.to_owned())]
	pub(in crate::cli) listen_address: String,
	/// Trusted browser Origin for Streamable HTTP. Repeat for multiple origins.
	#[arg(long = "allow-origin", value_name = "ORIGIN")]
	pub(in crate::cli) allowed_origins: Vec<String>,
	/// Environment variable containing the Streamable HTTP bearer token.
	#[arg(long = "bearer-token-env", value_name = "ENV_VAR")]
	pub(in crate::cli) bearer_token_env: Option<String>,
}
impl McpServeCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		mcp::serve(McpServeRequest {
			transport: self.transport,
			config_path: self.project_config.as_path(),
			capability_profile: self.effective_capability_profile(),
			listen_address: &self.listen_address,
			allowed_origins: &self.allowed_origins,
			bearer_token_env: self.bearer_token_env.as_deref(),
		})
	}

	pub(in crate::cli) fn effective_capability_profile(&self) -> McpCapabilityProfile {
		self.capability_profile.unwrap_or_else(|| self.transport.default_capability_profile())
	}
}

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum McpSubcommand {
	/// Serve Decodex MCP protocol primitives.
	Serve(McpServeCommand),
}
