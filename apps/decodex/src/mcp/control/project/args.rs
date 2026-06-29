use serde::Deserialize;

use crate::mcp;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProjectControlToolArgs {
	pub(super) action: String,
	pub(super) project_id: Option<String>,
	pub(super) authority: Option<ProjectControlAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProjectControlAuthorityArgs {
	pub(super) reason: Option<String>,
	pub(super) source: Option<String>,
	pub(super) acknowledge_future_dispatch_only: Option<bool>,
}

pub(super) struct ProjectControlAuthority<'a> {
	pub(super) reason: &'a str,
	pub(super) source: &'a str,
	pub(super) acknowledge_future_dispatch_only: bool,
}

pub(super) fn project_control_authority(
	params: &ProjectControlToolArgs,
) -> Option<ProjectControlAuthority<'_>> {
	let authority = params.authority.as_ref()?;

	Some(ProjectControlAuthority {
		reason: mcp::non_empty_string(authority.reason.as_deref())?,
		source: mcp::non_empty_string(authority.source.as_deref())?,
		acknowledge_future_dispatch_only: authority
			.acknowledge_future_dispatch_only
			.unwrap_or(false),
	})
}
