use std::{
	fmt::{Debug, Display, Formatter},
	io::ErrorKind,
	str,
};

use serde::{Deserialize, Deserializer, de::Error};

use crate::{ConfigError, DecodexPaths, PathError, paths};

const MAX_IDENTITY_BYTES: usize = 64;

/// Standard RFC 9562 UUID version 4 server identity.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct ServerIdentity(String);
impl ServerIdentity {
	/// Parse the canonical lowercase UUID version 4 representation.
	pub fn parse(value: impl Into<String>) -> Result<Self, ConfigError> {
		let value = value.into();

		if !valid_uuid_v4(&value) {
			return Err(ConfigError::InvalidServerIdentity);
		}

		Ok(Self(value))
	}

	/// Generate a standard UUID version 4 using the operating-system random source.
	pub fn generate() -> Result<Self, ConfigError> {
		let mut bytes = [0_u8; 16];

		getrandom::fill(&mut bytes).map_err(|_| ConfigError::RandomnessUnavailable)?;

		bytes[6] = (bytes[6] & 0x0f) | 0x40;
		bytes[8] = (bytes[8] & 0x3f) | 0x80;

		let encoded = format!(
			"{}-{}-{}-{}-{}",
			hex(&bytes[0..4]),
			hex(&bytes[4..6]),
			hex(&bytes[6..8]),
			hex(&bytes[8..10]),
			hex(&bytes[10..16]),
		);

		Self::parse(encoded)
	}

	/// Load the stable server-host identity or atomically create it exactly once.
	pub fn load_or_create(paths: &DecodexPaths) -> Result<Self, ConfigError> {
		paths.ensure_layout()?;

		let path = paths.server_identity_file();

		match Self::load(paths) {
			Ok(identity) => return Ok(identity),
			Err(ConfigError::Path(PathError::Io { kind: ErrorKind::NotFound, .. })) => {},
			Err(error) => return Err(error),
		}

		let generated = Self::generate()?;
		let body = format!("{generated}\n");

		match paths::atomic_write_new(paths, &path, body.as_bytes(), MAX_IDENTITY_BYTES) {
			Ok(()) => Ok(generated),
			Err(PathError::AlreadyExists) => Self::load(paths),
			Err(error) => Err(error.into()),
		}
	}

	/// Load and validate an existing stable identity.
	pub fn load(paths: &DecodexPaths) -> Result<Self, ConfigError> {
		let bytes =
			paths::read_private_file(paths, &paths.server_identity_file(), MAX_IDENTITY_BYTES)?;
		let value = match bytes.as_slice() {
			bytes if bytes.len() == 36 => bytes,
			bytes if bytes.len() == 37 && bytes.last() == Some(&b'\n') => &bytes[..36],
			_ => return Err(ConfigError::InvalidServerIdentity),
		};
		let value = str::from_utf8(value).map_err(|_| ConfigError::InvalidServerIdentity)?;

		Self::parse(value.to_owned())
	}

	/// Canonical UUID text.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Debug for ServerIdentity {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_tuple("ServerIdentity").field(&self.0).finish()
	}
}

impl Display for ServerIdentity {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

impl<'de> Deserialize<'de> for ServerIdentity {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = String::deserialize(deserializer)?;

		Self::parse(value).map_err(Error::custom)
	}
}

fn valid_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();

	if bytes.len() != 36
		|| bytes[8] != b'-'
		|| bytes[13] != b'-'
		|| bytes[18] != b'-'
		|| bytes[23] != b'-'
		|| bytes[14] != b'4'
		|| !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
	{
		return false;
	}

	bytes.iter().enumerate().all(|(index, byte)| {
		matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
	})
}

fn hex(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";

	let mut encoded = String::with_capacity(bytes.len() * 2);

	for &byte in bytes {
		encoded.push(char::from(HEX[usize::from(byte >> 4)]));
		encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}

	encoded
}
