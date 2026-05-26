use std::path::{Component, Path};

const SENSITIVE_PHRASES: &[&str] = &[
	"account=",
	"account_fingerprint",
	"account fingerprint",
	"api key",
	"auth token",
	"codex.github-identity",
	"codex.linear-workspace",
	"credential",
	"github identity",
	"github-identity",
	"linear workspace",
	"linear-workspace",
	"process_start_identity",
	"routed identity",
	"selected account",
	"token=",
];
const HOST_PATH_PREFIXES: &[&str] = &[
	"/home/",
	"/private/",
	"/root/",
	"/tmp/",
	"/users/",
	"/var/folders/",
	"/volumes/",
	"file:///",
];
const CREDENTIAL_MARKERS: &[&str] = &[
	"API_KEY",
	"AUTH_JSON",
	"CREDENTIAL",
	"GITHUB_PAT",
	"LINEAR_API_KEY",
	"PASSWD",
	"PASSWORD",
	"SECRET",
	"TOKEN",
];
const PRIVATE_CONFIG_FILES: &[&str] = &["auth.json", "accounts.jsonl"];
const PRIVATE_ENV_VAR_TOKENS: &[&str] = &["CODEX_HOME", "CODEX_SQLITE_HOME"];

pub(crate) fn validate_public_text_field(field_name: &str, value: &str) -> Result<(), String> {
	if let Some(reason) = public_text_violation(value) {
		return Err(format!("`{field_name}` must be public/team-visible text; {reason}."));
	}

	Ok(())
}

pub(crate) fn validate_public_text_items(
	field_name: &str,
	values: &[String],
) -> Result<(), String> {
	for value in values {
		validate_public_text_field(field_name, value)?;
	}

	Ok(())
}

pub(crate) fn validate_public_comment_body(body: &str) -> Result<(), String> {
	validate_public_text_field("body", body)?;

	for line in body.lines() {
		let Some((field_name, value)) = extract_structured_field(line) else {
			continue;
		};

		if field_name == "worktree_path" {
			validate_repo_relative_path(value, field_name)?;

			continue;
		}
		if field_name.ends_with("_path") {
			return Err(format!(
				"Unsupported structured field `{field_name}` in public issue comments."
			));
		}
	}

	Ok(())
}

fn public_text_violation(value: &str) -> Option<&'static str> {
	if contains_host_path(value) {
		return Some("host-local paths are not allowed");
	}
	if contains_credential_like_name(value) {
		return Some("credential-like names are not allowed");
	}
	if contains_private_env_var_token(value) {
		return Some("private environment variable names are not allowed");
	}
	if contains_email_address(value) {
		return Some("private identity details are not allowed");
	}
	if contains_sensitive_phrase(value) {
		return Some("local identity or account-routing details are not allowed");
	}

	None
}

fn contains_host_path(value: &str) -> bool {
	let lower = value.to_ascii_lowercase();

	lower.contains("~/")
		|| lower.contains("~\\")
		|| HOST_PATH_PREFIXES.iter().any(|prefix| lower.contains(prefix))
		|| contains_windows_absolute_path(value)
}

fn contains_windows_absolute_path(value: &str) -> bool {
	value.as_bytes().windows(3).enumerate().any(|(index, window)| {
		let preceded_by_separator =
			index == 0 || !value.as_bytes()[index - 1].is_ascii_alphanumeric();

		preceded_by_separator
			&& window[0].is_ascii_alphabetic()
			&& window[1] == b':'
			&& matches!(window[2], b'\\' | b'/')
	})
}

fn contains_credential_like_name(value: &str) -> bool {
	value.split(is_token_separator).any(is_credential_like_token)
}

fn is_token_separator(character: char) -> bool {
	!(character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

fn is_credential_like_token(token: &str) -> bool {
	if token.is_empty() {
		return false;
	}

	let has_name_shape = token.contains('_') || token.contains('-') || is_all_caps_token(token);

	if !has_name_shape {
		return false;
	}

	let normalized = token.replace('-', "_").to_ascii_uppercase();

	CREDENTIAL_MARKERS.iter().any(|marker| {
		normalized == *marker
			|| normalized.starts_with(&format!("{marker}_"))
			|| normalized.ends_with(&format!("_{marker}"))
			|| normalized.contains(&format!("_{marker}_"))
	})
}

fn is_all_caps_token(token: &str) -> bool {
	let mut has_letter = false;

	for character in token.chars() {
		if character.is_ascii_lowercase() {
			return false;
		}
		if character.is_ascii_alphabetic() {
			has_letter = true;
		}
	}

	has_letter
}

fn contains_private_env_var_token(value: &str) -> bool {
	value.split(is_token_separator).any(|token| {
		let normalized = token.to_ascii_uppercase();

		PRIVATE_ENV_VAR_TOKENS.iter().any(|env_var| normalized == *env_var)
	})
}

fn contains_email_address(value: &str) -> bool {
	value.split_whitespace().any(is_email_like_token)
}

fn is_email_like_token(token: &str) -> bool {
	let token = token.trim_matches(|character: char| {
		matches!(character, '`' | '\'' | '"' | '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';' | '.')
	});
	let Some((local, domain)) = token.split_once('@') else {
		return false;
	};

	!local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn contains_sensitive_phrase(value: &str) -> bool {
	let lower = value.to_ascii_lowercase();

	SENSITIVE_PHRASES.iter().any(|phrase| lower.contains(phrase))
		|| PRIVATE_CONFIG_FILES.iter().any(|file_name| lower.contains(file_name))
}

fn extract_structured_field(line: &str) -> Option<(&str, &str)> {
	let trimmed = line.trim();
	let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed);
	let (key, value) = trimmed.split_once(':')?;

	Some((key.trim(), value.trim().trim_matches('`')))
}

fn validate_repo_relative_path(path: &str, field_name: &str) -> Result<(), String> {
	if path.is_empty() {
		return Err(format!("`{field_name}` must not be empty."));
	}
	if path.starts_with('/') || path.starts_with("~/") || has_drive_root_prefix(path) {
		return Err(format!("`{field_name}` must be repository-relative, not `{path}`."));
	}
	if Path::new(path).components().any(|component| matches!(component, Component::ParentDir)) {
		return Err(format!("`{field_name}` must stay within the repository, not `{path}`."));
	}

	Ok(())
}

fn has_drive_root_prefix(path: &str) -> bool {
	let bytes = path.as_bytes();

	bytes.len() >= 3
		&& bytes[0].is_ascii_alphabetic()
		&& bytes[1] == b':'
		&& matches!(bytes[2], b'\\' | b'/')
}

#[cfg(test)]
mod tests {
	use crate::tracker::public_text::{self};

	#[test]
	fn accepts_public_collaboration_identifiers() {
		for value in [
			"PR https://github.com/hack-ink/decodex/pull/42 is ready.",
			"Branch y/decodex-xy-519 reached commit 0123456789abcdef0123456789abcdef01234567.",
			"Issue XY-519 updated docs/spec/runtime.md and .worktrees/XY-519.",
		] {
			public_text::validate_public_text_field("summary", value)
				.unwrap_or_else(|error| panic!("public value should validate: {error}"));
		}
	}

	#[test]
	fn rejects_leakage_shaped_public_text() {
		for value in [
			"Local checkout was /Users/example/code/repo.",
			"Read ~/.codex/auth.json for the selected account.",
			"Windows path C:\\Users\\example\\repo was present.",
			"Missing GITHUB_PAT_Y blocked the push.",
			"Selected account was account=...e4919e.",
			"Missing API key for tracker writes.",
			"CODEX_HOME pointed at private configuration.",
			"codex.github-identity was routed to a private person.",
			"Selected account user@example.com was active.",
		] {
			let error = public_text::validate_public_text_field("evidence", value)
				.expect_err("leakage-shaped value should be rejected");

			assert!(error.contains("public/team-visible"));
		}
	}

	#[test]
	fn validates_public_comment_structured_paths() {
		public_text::validate_public_comment_body(
			"decodex run failed and will retry\n\n- worktree_path: `.worktrees/DEC-1`",
		)
		.expect("repo-relative worktree path should be public");

		for (body, expected_error) in [
			(
				"decodex run failed and will retry\n\n- worktree_path: `/absolute/path/to/repo/.worktrees/DEC-1`",
				"`worktree_path` must be repository-relative",
			),
			(
				"decodex run failed and will retry\n\n- unexpected_path: `.worktrees/DEC-1`",
				"Unsupported structured field `unexpected_path`",
			),
		] {
			let error = public_text::validate_public_comment_body(body)
				.expect_err("private or unsupported comment path should be rejected");

			assert!(error.contains(expected_error), "{error}");
		}
	}
}
