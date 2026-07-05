use std::path::Path;

use clap::Parser;

use crate::{
	cli::{Cli, Command, control_commands::mcp::McpSubcommand},
	mcp::{McpCapabilityProfile, McpTransport},
};

#[test]
fn parses_mcp_stdio_serve() {
	let cli = Cli::parse_from([
		"decodex",
		"mcp",
		"serve",
		"--config",
		"./project.toml",
		"--transport",
		"stdio",
	]);
	let Command::Mcp(command) = cli.command else {
		panic!("expected mcp command");
	};
	let McpSubcommand::Serve(serve) = command.command;

	assert_eq!(serve.project_config.config.as_deref(), Some(Path::new("./project.toml")));
	assert_eq!(serve.transport, McpTransport::Stdio);
	assert_eq!(serve.effective_capability_profile(), McpCapabilityProfile::Admin);
	assert_eq!(serve.listen_address, crate::mcp::DEFAULT_MCP_HTTP_LISTEN_ADDRESS);
	assert!(serve.allowed_origins.is_empty());
	assert_eq!(serve.bearer_token_env, None);
}

#[test]
fn parses_mcp_streamable_http_serve_with_safe_profile_default() {
	let cli = Cli::parse_from([
		"decodex",
		"mcp",
		"serve",
		"--transport",
		"streamable-http",
		"--listen-address",
		"127.0.0.1:8194",
		"--allow-origin",
		"http://127.0.0.1:8194",
		"--bearer-token-env",
		"DECODEX_MCP_TOKEN",
	]);
	let Command::Mcp(command) = cli.command else {
		panic!("expected mcp command");
	};
	let McpSubcommand::Serve(serve) = command.command;

	assert_eq!(serve.transport, McpTransport::StreamableHttp);
	assert_eq!(serve.effective_capability_profile(), McpCapabilityProfile::Observe);
	assert_eq!(serve.listen_address, "127.0.0.1:8194");
	assert_eq!(serve.allowed_origins, vec!["http://127.0.0.1:8194"]);
	assert_eq!(serve.bearer_token_env.as_deref(), Some("DECODEX_MCP_TOKEN"));
}
