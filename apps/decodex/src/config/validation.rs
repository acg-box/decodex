#[cfg(target_os = "macos")]
use std::process::Command;
use std::{env, path::Path};

use crate::prelude::{Result, eyre};

pub(super) fn validate_nonempty_path(field_name: &str, value: &Path) -> Result<()> {
	if value.as_os_str().is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}

	Ok(())
}

pub(super) fn validate_optional_nonempty_string(
	field_name: &str,
	value: Option<&str>,
) -> Result<()> {
	let Some(value) = value else {
		return Ok(());
	};

	if value.trim().is_empty() {
		eyre::bail!("`{field_name}` must not be empty when configured.");
	}

	Ok(())
}

pub(super) fn validate_required_config_string(field_name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}

	Ok(())
}

pub(super) fn validate_service_id(field_name: &str, value: &str) -> Result<()> {
	let trimmed = value.trim();

	if trimmed.is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}
	if trimmed != value {
		eyre::bail!("`{field_name}` must not include surrounding whitespace.");
	}

	let mut chars = trimmed.chars();
	let Some(first) = chars.next() else {
		eyre::bail!("`{field_name}` must not be empty.");
	};

	if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
		eyre::bail!("`{field_name}` must start with a lowercase ASCII letter or digit.");
	}
	if chars.any(|character| {
		!(character.is_ascii_lowercase()
			|| character.is_ascii_digit()
			|| matches!(character, '-' | '_'))
	}) {
		eyre::bail!(
			"`{field_name}` must contain only lowercase ASCII letters, digits, hyphens, or underscores."
		);
	}

	Ok(())
}

pub(super) fn validate_env_var_name(field_name: &str, value: &str) -> Result<()> {
	let trimmed = value.trim();

	if trimmed.is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}
	if trimmed != value {
		eyre::bail!("`{field_name}` must not include surrounding whitespace.");
	}
	if trimmed.starts_with('$') {
		eyre::bail!(
			"`{field_name}` must name the environment variable directly, without a `$` prefix."
		);
	}

	let mut chars = trimmed.chars();
	let Some(first) = chars.next() else {
		eyre::bail!("`{field_name}` must not be empty.");
	};

	if !(first == '_' || first.is_ascii_alphabetic()) {
		eyre::bail!(
			"`{field_name}` must start with an ASCII letter or underscore and contain only ASCII letters, digits, or underscores."
		);
	}
	if chars.any(|character| !(character == '_' || character.is_ascii_alphanumeric())) {
		eyre::bail!("`{field_name}` must contain only ASCII letters, digits, or underscores.");
	}

	Ok(())
}

pub(super) fn resolve_secret_env_var(field_name: &str, env_var: &str) -> Result<String> {
	validate_env_var_name(field_name, env_var)?;

	let value = match env::var(env_var) {
		Ok(value) if !value.trim().is_empty() => value,
		Ok(_) => {
			if let Some(value) = resolve_secret_launchd_env_var(env_var) {
				value
			} else {
				eyre::bail!(
					"Environment variable `{env_var}` referenced by `{field_name}` must not be blank."
				);
			}
		},
		Err(error) => {
			if let Some(value) = resolve_secret_launchd_env_var(env_var) {
				value
			} else {
				return Err(eyre::eyre!(
					"Failed to read environment variable `{env_var}` referenced by `{field_name}`: {error}"
				));
			}
		},
	};

	if value.trim().is_empty() {
		eyre::bail!(
			"Environment variable `{env_var}` referenced by `{field_name}` must not be blank."
		);
	}

	Ok(value)
}

#[cfg(target_os = "macos")]
fn resolve_secret_launchd_env_var(env_var: &str) -> Option<String> {
	let output = Command::new("/bin/launchctl").args(["getenv", env_var]).output().ok()?;

	if !output.status.success() {
		return None;
	}

	let value = String::from_utf8(output.stdout).ok()?.trim().to_owned();

	if value.is_empty() { None } else { Some(value) }
}

#[cfg(not(target_os = "macos"))]
fn resolve_secret_launchd_env_var(_env_var: &str) -> Option<String> {
	None
}
