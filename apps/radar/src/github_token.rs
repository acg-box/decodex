use std::{env, process::Command};

use crate::prelude::{Result, eyre};

pub(super) fn github_token(token_env: Option<&str>) -> Result<Option<String>> {
	if let Some(token_env) = token_env {
		return env_token(token_env).map(Some).ok_or_else(|| {
			eyre::eyre!("GitHub token environment variable {token_env} is missing or empty")
		});
	}

	Ok(routed_token_env()
		.and_then(|token_env| env_token(&token_env))
		.or_else(|| env_token("GH_TOKEN"))
		.or_else(|| env_token("GITHUB_TOKEN")))
}

fn env_token(token_env: &str) -> Option<String> {
	env::var(token_env).ok().filter(|token| !token.is_empty())
}

fn routed_token_env() -> Option<String> {
	let output =
		Command::new("git").args(["config", "--get", "codex.github-identity"]).output().ok()?;

	if !output.status.success() {
		return None;
	}

	token_env_for_identity(String::from_utf8_lossy(&output.stdout).trim()).map(str::to_owned)
}

fn token_env_for_identity(identity: &str) -> Option<&'static str> {
	match identity {
		"x" => Some("GITHUB_PAT_X"),
		"y" => Some("GITHUB_PAT_Y"),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::token_env_for_identity;

	#[test]
	fn maps_known_repository_identities_without_overriding_default_fallbacks() {
		assert_eq!(token_env_for_identity("x"), Some("GITHUB_PAT_X"));
		assert_eq!(token_env_for_identity("y"), Some("GITHUB_PAT_Y"));
		assert_eq!(token_env_for_identity("default"), None);
		assert_eq!(token_env_for_identity(""), None);
	}
}
