use std::net::IpAddr;

use reqwest::Url;

use crate::{
	mcp::{McpCapabilityProfile, http::auth::McpHttpAuthorization},
	prelude::{Result, eyre},
};

pub(in crate::mcp) fn validate_mcp_http_listen_address(
	address: &str,
	allowed_origins: &[String],
	authorization: &McpHttpAuthorization,
) -> Result<()> {
	if listen_address_host_is_loopback(address) {
		return Ok(());
	}
	if allowed_origins.is_empty() {
		eyre::bail!(
			"Refusing to bind Decodex MCP Streamable HTTP to `{address}` without --allow-origin; use the loopback default or set explicit trusted origins."
		)
	}
	if !authorization.is_required() {
		eyre::bail!(
			"Refusing to bind Decodex MCP Streamable HTTP to `{address}` without --bearer-token-env; direct non-loopback listeners require bearer authorization."
		)
	}

	Ok(())
}

pub(in crate::mcp) fn validate_mcp_http_capability_profile(
	capability_profile: McpCapabilityProfile,
	authorization: &McpHttpAuthorization,
) -> Result<()> {
	if capability_profile == McpCapabilityProfile::Observe || authorization.is_required() {
		return Ok(());
	}

	eyre::bail!(
		"Refusing to expose Decodex MCP Streamable HTTP profile `{}` without --bearer-token-env; elevated HTTP profiles require bearer authorization.",
		capability_profile.as_str()
	)
}

pub(super) fn mcp_http_origin_is_allowed(
	origin: &str,
	listen_address: Option<&str>,
	allowed_origins: &[String],
) -> bool {
	if allowed_origins.iter().any(|allowed| allowed == origin) {
		return true;
	}

	let Ok(parsed) = Url::parse(origin) else {
		return false;
	};
	let Some(host) = parsed.host_str() else {
		return false;
	};

	if !matches!(parsed.scheme(), "http" | "https") || !host_is_loopback(host) {
		return false;
	}

	let Some(listen_port) = listen_address.and_then(listen_address_port) else {
		return true;
	};

	parsed.port_or_known_default() == Some(listen_port)
}

fn listen_address_host_is_loopback(address: &str) -> bool {
	let host = listen_address_host(address);

	host.as_deref().is_some_and(host_is_loopback)
}

fn host_is_loopback(host: &str) -> bool {
	host.eq_ignore_ascii_case("localhost")
		|| host
			.trim_matches(['[', ']'])
			.parse::<IpAddr>()
			.is_ok_and(|address| address.is_loopback())
}

fn listen_address_host(address: &str) -> Option<String> {
	let (host, _) = address.rsplit_once(':')?;

	Some(host.trim_matches(['[', ']']).to_owned())
}

fn listen_address_port(address: &str) -> Option<u16> {
	let (_, port) = address.rsplit_once(':')?;

	port.parse().ok()
}
