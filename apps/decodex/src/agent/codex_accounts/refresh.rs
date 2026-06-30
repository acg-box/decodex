use std::{
	error::Error,
	fmt::{self, Display, Formatter},
};

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::usage::{json_scalar_to_string, number_as_i64};

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct CodexTokenData {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) email: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) id_token: Option<String>,
	pub(crate) access_token: String,
	pub(super) refresh_token: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) account_id: Option<String>,
}

#[derive(Serialize)]
pub(super) struct RefreshRequest {
	pub(super) client_id: &'static str,
	pub(super) grant_type: &'static str,
	pub(super) refresh_token: String,
}

#[derive(Deserialize)]
pub(super) struct RefreshResponse {
	pub(super) id_token: Option<String>,
	pub(super) access_token: Option<String>,
	pub(super) refresh_token: Option<String>,
}

#[derive(Debug)]
pub(super) struct ProactiveRefreshError {
	pub(super) source: ReportableRefreshError,
	pub(super) requires_skip: bool,
	pub(super) auth_failed: bool,
}

#[derive(Debug)]
pub(super) struct ReportableRefreshError {
	message: String,
}
impl ReportableRefreshError {
	pub(super) fn new(message: String) -> Self {
		Self { message }
	}
}

impl Display for ReportableRefreshError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.message)
	}
}

impl Error for ReportableRefreshError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RefreshStatus {
	NotNeeded,
	Succeeded,
	Failed,
}
impl RefreshStatus {
	pub(super) const fn as_str(self) -> &'static str {
		match self {
			Self::NotNeeded => "not_needed",
			Self::Succeeded => "succeeded",
			Self::Failed => "failed",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProactiveRefreshReason {
	AccessTokenExpired,
	LastRefreshStale,
}
impl ProactiveRefreshReason {
	pub(super) const fn requires_valid_token(self) -> bool {
		matches!(self, Self::AccessTokenExpired)
	}
}

impl Display for ProactiveRefreshReason {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Self::AccessTokenExpired => formatter.write_str("expired access token"),
			Self::LastRefreshStale => formatter.write_str("stale refresh timestamp"),
		}
	}
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

pub(super) fn rfc3339_unix_epoch(input: &str) -> Option<i64> {
	OffsetDateTime::parse(input, &Rfc3339).ok().map(|timestamp| timestamp.unix_timestamp())
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

pub(super) fn token_refresh_auth_status(status: StatusCode) -> bool {
	matches!(status, StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}
