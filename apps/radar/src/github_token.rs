use std::{env, process::Command};

pub(super) fn github_token(token_env: Option<&str>) -> Option<String> {
	if let Some(token_env) = token_env {
		return env_token(token_env);
	}

	routed_token_env()
		.and_then(|token_env| env_token(&token_env))
		.or_else(|| env_token("GITHUB_TOKEN"))
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

	match String::from_utf8_lossy(&output.stdout).trim() {
		"x" => Some("GITHUB_PAT_X".into()),
		"y" => Some("GITHUB_PAT_Y".into()),
		_ => Some("GITHUB_TOKEN".into()),
	}
}
