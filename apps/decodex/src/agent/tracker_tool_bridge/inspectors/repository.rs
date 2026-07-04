#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::agent::tracker_tool_bridge) struct RepositoryIdentity {
	pub(in crate::agent::tracker_tool_bridge) name: String,
	pub(in crate::agent::tracker_tool_bridge) owner: String,
}

pub(in crate::agent::tracker_tool_bridge) fn parse_remote_head_symref_output(
	stdout: &str,
) -> Option<String> {
	stdout.lines().find_map(|line| {
		let line = line.trim();

		line.strip_prefix("ref: refs/heads/")
			.and_then(|remainder| remainder.strip_suffix("\tHEAD"))
			.map(str::to_owned)
	})
}

pub(in crate::agent::tracker_tool_bridge) fn parse_github_repository_identity(
	remote_url: &str,
) -> std::result::Result<RepositoryIdentity, String> {
	let path = if let Some(path) = remote_url.strip_prefix("git@github.com:") {
		path
	} else {
		parse_github_remote_with_authority(remote_url)?
	};
	let path = path.strip_suffix(".git").unwrap_or(path);
	let mut parts = path.split('/');
	let Some(owner) = parts.next() else {
		return Err(format!("Unsupported GitHub remote URL `{remote_url}`."));
	};
	let Some(name) = parts.next() else {
		return Err(format!("Unsupported GitHub remote URL `{remote_url}`."));
	};

	if owner.is_empty() || name.is_empty() || parts.next().is_some() {
		return Err(format!("Unsupported GitHub remote URL `{remote_url}`."));
	}

	Ok(RepositoryIdentity { name: name.to_owned(), owner: owner.to_owned() })
}

fn parse_github_remote_with_authority(remote_url: &str) -> std::result::Result<&str, String> {
	let rest = remote_url
		.strip_prefix("https://")
		.or_else(|| remote_url.strip_prefix("http://"))
		.or_else(|| remote_url.strip_prefix("ssh://"))
		.ok_or_else(|| format!("Unsupported GitHub remote URL `{remote_url}`."))?;
	let (authority, path) = rest
		.split_once('/')
		.ok_or_else(|| format!("Unsupported GitHub remote URL `{remote_url}`."))?;
	let authority = authority.rsplit('@').next().unwrap_or(authority);
	let host = authority.split_once(':').map(|(host, _)| host).unwrap_or(authority);

	if host != "github.com" {
		return Err(format!("Unsupported GitHub remote URL `{remote_url}`."));
	}

	Ok(path)
}
