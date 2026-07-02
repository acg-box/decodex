use std::{
	env, fs,
	path::{Path, PathBuf},
	process,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
	accounts::file_security,
	prelude::{Result, eyre},
};

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct AuthDotJson {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) email: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
	pub(super) openai_api_key: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) tokens: Option<CodexTokenData>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) last_refresh: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct CodexTokenData {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) email: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) id_token: Option<String>,
	pub(super) access_token: String,
	pub(super) refresh_token: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) account_id: Option<String>,
}

pub(super) fn default_codex_auth_json_path() -> Result<PathBuf> {
	if let Some(codex_home) =
		env::var_os("CODEX_HOME").map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
	{
		return Ok(codex_home.join("auth.json"));
	}

	let Some(home) = env::var_os("HOME") else {
		eyre::bail!("Failed to resolve `$HOME` for the Codex auth JSON path.");
	};

	Ok(PathBuf::from(home).join(".codex").join("auth.json"))
}

pub(super) fn write_auth_json_atomically(path: &Path, auth: &AuthDotJson) -> Result<()> {
	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Codex auth JSON path `{}` must have a parent directory.", path.display())
	})?;
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Codex auth JSON path must end in a valid file name."))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let mut output = serde_json::to_string_pretty(auth)?;

	output.push('\n');

	fs::create_dir_all(parent)?;
	fs::write(&temp_path, output)?;
	file_security::secure_account_file(&temp_path)?;
	fs::rename(temp_path, path)?;
	file_security::secure_account_file(path)?;

	Ok(())
}

pub(super) fn first_nonblank_string(left: Option<String>, right: Option<String>) -> Option<String> {
	left.filter(|value| !value.trim().is_empty())
		.or_else(|| right.filter(|value| !value.trim().is_empty()))
}

pub(super) fn nonblank_string(value: Option<&str>) -> Option<String> {
	value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

pub(super) fn jwt_email_claim(id_token: Option<&str>) -> Option<String> {
	let payload = id_token?.split('.').nth(1)?;
	let payload_bytes = parse_base64_url(payload)?;
	let claims = serde_json::from_slice::<Value>(&payload_bytes).ok()?;

	claims.get("email").and_then(json_scalar_to_string)
}

pub(super) fn jwt_expiration_unix_epoch(jwt: &str) -> Option<i64> {
	let payload = jwt.split('.').nth(1)?;
	let payload_bytes = parse_base64_url(payload)?;
	let claims = serde_json::from_slice::<Value>(&payload_bytes).ok()?;

	claims.get("exp").and_then(number_as_i64)
}

fn parse_base64_url(input: &str) -> Option<Vec<u8>> {
	let mut output = Vec::with_capacity(input.len() * 3 / 4);
	let mut accumulator = 0_u32;
	let mut bits = 0_u32;

	for byte in input.bytes().take_while(|byte| *byte != b'=') {
		accumulator = (accumulator << 6) | u32::from(base64_url_value(byte)?);
		bits += 6;

		if bits >= 8 {
			bits -= 8;

			output.push(((accumulator >> bits) & 0xff) as u8);
		}
	}

	Some(output)
}

const fn base64_url_value(byte: u8) -> Option<u8> {
	match byte {
		b'A'..=b'Z' => Some(byte - b'A'),
		b'a'..=b'z' => Some(byte - b'a' + 26),
		b'0'..=b'9' => Some(byte - b'0' + 52),
		b'-' => Some(62),
		b'_' => Some(63),
		_ => None,
	}
}

fn json_scalar_to_string(value: &Value) -> Option<String> {
	match value {
		Value::String(text) if !text.is_empty() => Some(text.clone()),
		Value::Number(number) => Some(number.to_string()),
		Value::Bool(value) => Some(value.to_string()),
		_ => None,
	}
}

fn number_as_i64(value: &Value) -> Option<i64> {
	value
		.as_i64()
		.or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
		.or_else(|| value.as_f64().map(|number| number.round() as i64))
}
