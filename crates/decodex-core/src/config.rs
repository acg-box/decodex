use std::{
	collections::BTreeMap,
	fmt::{Debug, Display, Formatter},
	net::IpAddr,
	str,
};

use serde::{Deserialize, Deserializer, de::IgnoredAny};

use crate::{CacheLimits, DecodexPaths, PathError, ServerIdentity, paths};

/// Maximum accepted UTF-8 configuration input.
pub const MAX_CONFIG_BYTES: usize = 64 * 1_024;

const CONFIG_VERSION: u32 = 1;
const MAX_NAME_BYTES: usize = 64;
const MAX_HOST_BYTES: usize = 253;

/// Fully validated Decodex vNext configuration.
#[derive(Clone)]
pub struct DecodexConfig {
	version: u32,
	active_profile: ProfileName,
	profiles: BTreeMap<ProfileName, ServerProfile>,
	cache: CacheConfig,
}
impl DecodexConfig {
	/// Parse bounded UTF-8 TOML. Parser details and input excerpts are deliberately
	/// discarded so secret-bearing malformed input cannot enter errors or logs.
	pub fn parse(bytes: &[u8]) -> Result<Self, ConfigError> {
		if bytes.len() > MAX_CONFIG_BYTES {
			return Err(ConfigError::Oversized { limit: MAX_CONFIG_BYTES });
		}

		let input = str::from_utf8(bytes).map_err(|_| ConfigError::Malformed)?;
		let raw: RawConfig = toml::from_str(input).map_err(|_| ConfigError::Malformed)?;

		if raw.version != CONFIG_VERSION {
			return Err(ConfigError::UnsupportedVersion);
		}

		let profiles = validate_profiles(raw.profiles)?;

		if !profiles.contains_key(&raw.active_profile) {
			return Err(ConfigError::MissingActiveProfile);
		}

		Ok(Self {
			version: raw.version,
			active_profile: raw.active_profile,
			profiles,
			cache: raw.cache.validate()?,
		})
	}

	/// Read and parse the private root configuration file through the bounded,
	/// no-symlink path owner.
	pub fn load(paths: &DecodexPaths) -> Result<Self, ConfigError> {
		let bytes = paths::read_private_file(paths, &paths.config_file(), MAX_CONFIG_BYTES)?;

		Self::parse(&bytes)
	}

	/// Schema version accepted by this build.
	pub const fn version(&self) -> u32 {
		self.version
	}

	/// Explicitly selected profile name.
	pub fn active_profile_name(&self) -> &ProfileName {
		&self.active_profile
	}

	/// Explicitly selected local or remote profile.
	pub fn active_profile(&self) -> &ServerProfile {
		self.profiles.get(&self.active_profile).expect("validated active profile is present")
	}

	/// All explicit profiles.
	pub fn profiles(&self) -> &BTreeMap<ProfileName, ServerProfile> {
		&self.profiles
	}

	/// Disposable cache bounds.
	pub const fn cache(&self) -> CacheConfig {
		self.cache
	}
}

impl Debug for DecodexConfig {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("DecodexConfig")
			.field("version", &self.version)
			.field("active_profile", &self.active_profile)
			.field("profile_count", &self.profiles.len())
			.field("cache", &self.cache)
			.finish()
	}
}

/// Client-visible projection of the global configuration.
///
/// Retired PostgreSQL data and cache policy are consumed as opaque TOML values. A
/// client validates only profile authority and cannot reinterpret server-owned state.
#[derive(Clone)]
pub struct DecodexClientConfig {
	version: u32,
	active_profile: ProfileName,
	profiles: BTreeMap<ProfileName, ServerProfile>,
}
impl DecodexClientConfig {
	/// Parse the bounded client projection without validating server-host-only data.
	pub fn parse(bytes: &[u8]) -> Result<Self, ConfigError> {
		if bytes.len() > MAX_CONFIG_BYTES {
			return Err(ConfigError::Oversized { limit: MAX_CONFIG_BYTES });
		}

		let input = str::from_utf8(bytes).map_err(|_| ConfigError::Malformed)?;
		let raw: RawClientConfig = toml::from_str(input).map_err(|_| ConfigError::Malformed)?;

		if raw.version != CONFIG_VERSION {
			return Err(ConfigError::UnsupportedVersion);
		}

		let profiles = validate_profiles(raw.profiles)?;

		if !profiles.contains_key(&raw.active_profile) {
			return Err(ConfigError::MissingActiveProfile);
		}

		Ok(Self { version: raw.version, active_profile: raw.active_profile, profiles })
	}

	/// Read and parse the private client configuration projection.
	pub fn load(paths: &DecodexPaths) -> Result<Self, ConfigError> {
		let bytes = paths::read_private_file(paths, &paths.config_file(), MAX_CONFIG_BYTES)?;

		Self::parse(&bytes)
	}

	/// Schema version accepted by this build.
	pub const fn version(&self) -> u32 {
		self.version
	}

	/// Explicitly selected profile name.
	pub fn active_profile_name(&self) -> &ProfileName {
		&self.active_profile
	}

	/// Resolve either an explicit named profile or the configured active profile.
	pub fn selected_profile(
		&self,
		name: Option<&str>,
	) -> Result<(&ProfileName, &ServerProfile), ConfigError> {
		let selected = match name {
			Some(name) => self
				.profiles
				.iter()
				.find(|(candidate, _)| candidate.as_str() == name)
				.ok_or(ConfigError::MissingProfile)?,
			None => {
				let profile = self
					.profiles
					.get(&self.active_profile)
					.expect("validated active profile is present");

				(&self.active_profile, profile)
			},
		};

		Ok(selected)
	}
}

impl Debug for DecodexClientConfig {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("DecodexClientConfig")
			.field("version", &self.version)
			.field("active_profile", &self.active_profile)
			.field("profile_count", &self.profiles.len())
			.finish()
	}
}

/// Validated profile-map key.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProfileName(String);
impl ProfileName {
	/// Validated profile name text.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Debug for ProfileName {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ProfileName(<redacted>)")
	}
}

impl<'de> Deserialize<'de> for ProfileName {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = String::deserialize(deserializer)?;

		Self::try_from(value).map_err(serde::de::Error::custom)
	}
}

impl TryFrom<String> for ProfileName {
	type Error = &'static str;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		validate_name(&value).map_err(|_| "invalid profile name")?;

		Ok(Self(value))
	}
}

/// Closed local transport policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LocalTrustPolicy {
	/// Do not create or connect to a local product endpoint.
	Disabled,
	/// Admit only kernel-authenticated peers with the exact service-owner effective UID.
	SameUid,
}

/// Validated local profile. The fixed endpoint is derived from the Decodex root.
#[derive(Clone, Eq, PartialEq)]
pub struct LocalProfile {
	policy: LocalTrustPolicy,
	service_owner_uid: Option<u32>,
	expected_server_identity: Option<ServerIdentity>,
}
impl LocalProfile {
	/// Closed local transport policy.
	pub const fn policy(&self) -> LocalTrustPolicy {
		self.policy
	}

	/// Effective UID that owns the local service namespace.
	pub const fn service_owner_uid(&self) -> Option<u32> {
		self.service_owner_uid
	}

	/// Optional identity pin for a known local server.
	pub fn expected_server_identity(&self) -> Option<&ServerIdentity> {
		self.expected_server_identity.as_ref()
	}
}

impl Debug for LocalProfile {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("LocalProfile")
			.field("policy", &self.policy)
			.field("service_owner_uid", &self.service_owner_uid.map(|_| "<configured>"))
			.field("identity_pinned", &self.expected_server_identity.is_some())
			.finish()
	}
}

/// Validated remote profile. Host and port are data only; this type does not enable
/// remote transport, authentication, or TLS.
#[derive(Clone, Eq, PartialEq)]
pub struct RemoteProfile {
	host: String,
	port: u16,
	expected_server_identity: ServerIdentity,
}
impl RemoteProfile {
	/// Remote DNS name or IP literal, stored separately from credentials and paths.
	pub fn host(&self) -> &str {
		&self.host
	}

	/// Remote service port.
	pub const fn port(&self) -> u16 {
		self.port
	}

	/// Required stable identity pin for the server host.
	pub fn expected_server_identity(&self) -> &ServerIdentity {
		&self.expected_server_identity
	}
}

impl Debug for RemoteProfile {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("RemoteProfile")
			.field("host", &"<redacted>")
			.field("port", &self.port)
			.field("expected_server_identity", &self.expected_server_identity)
			.finish()
	}
}

/// Validated disposable-cache configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheConfig {
	limits: CacheLimits,
}
impl CacheConfig {
	/// Mechanical byte and entry bounds.
	pub const fn limits(self) -> CacheLimits {
		self.limits
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
	version: u32,
	active_profile: ProfileName,
	profiles: BTreeMap<ProfileName, RawProfile>,
	/// Accepted only so an existing local config can be replaced without a flag day.
	/// No runtime code can inspect or use this retired section.
	#[serde(default, rename = "postgres")]
	_legacy_postgres: Option<IgnoredAny>,
	cache: RawCacheConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawClientConfig {
	version: u32,
	active_profile: ProfileName,
	profiles: BTreeMap<ProfileName, RawProfile>,
	#[serde(default, rename = "postgres")]
	_postgres: Option<IgnoredAny>,
	#[serde(rename = "cache")]
	_cache: IgnoredAny,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCacheConfig {
	max_entries: usize,
	max_bytes: usize,
	max_entry_bytes: usize,
}
impl RawCacheConfig {
	fn validate(self) -> Result<CacheConfig, ConfigError> {
		let limits = CacheLimits::new(self.max_entries, self.max_bytes, self.max_entry_bytes)
			.map_err(|_| ConfigError::InvalidCache)?;

		Ok(CacheConfig { limits })
	}
}

/// Explicit local or remote server selection.
#[derive(Clone)]
pub enum ServerProfile {
	/// Owner-only client and server live under one effective UID on the same host.
	Local(LocalProfile),
	/// Client and server live on different hosts.
	Remote(RemoteProfile),
}
impl Debug for ServerProfile {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Local(profile) => profile.fmt(formatter),
			Self::Remote(profile) => profile.fmt(formatter),
		}
	}
}

/// Typed redacted configuration failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigError {
	/// Input exceeded the parser limit.
	Oversized {
		/// Maximum accepted configuration bytes.
		limit: usize,
	},
	/// TOML, UTF-8, unknown fields, or shape was invalid.
	Malformed,
	/// Configuration schema version is unsupported.
	UnsupportedVersion,
	/// The selected profile was not declared.
	MissingActiveProfile,
	/// An explicitly selected profile was not declared.
	MissingProfile,
	/// A local or remote profile violated its closed contract.
	InvalidProfile,
	/// A server identity was not canonical UUID version 4 text.
	InvalidServerIdentity,
	/// Cache bounds were invalid or exceeded hard limits.
	InvalidCache,
	/// The operating-system random source failed.
	RandomnessUnavailable,
	/// Private owned-path validation or I/O failed.
	Path(PathError),
}
impl Display for ConfigError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Oversized { limit } => write!(formatter, "configuration exceeds {limit} bytes"),
			Self::Malformed => formatter.write_str("configuration is malformed"),
			Self::UnsupportedVersion => formatter.write_str("configuration version is unsupported"),
			Self::MissingActiveProfile => formatter.write_str("active profile is not declared"),
			Self::MissingProfile => formatter.write_str("selected profile is not declared"),
			Self::InvalidProfile => formatter.write_str("server profile is invalid"),
			Self::InvalidServerIdentity => formatter.write_str("server identity is invalid"),
			Self::InvalidCache => formatter.write_str("cache configuration is invalid"),
			Self::RandomnessUnavailable =>
				formatter.write_str("operating-system randomness is unavailable"),
			Self::Path(error) => Display::fmt(error, formatter),
		}
	}
}

impl std::error::Error for ConfigError {}

impl From<PathError> for ConfigError {
	fn from(error: PathError) -> Self {
		Self::Path(error)
	}
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RawProfile {
	Local {
		policy: LocalTrustPolicy,
		service_owner_uid: Option<u32>,
		expected_server_identity: Option<ServerIdentity>,
	},
	Remote {
		host: String,
		port: u16,
		expected_server_identity: ServerIdentity,
	},
}
impl RawProfile {
	fn validate(self) -> Result<ServerProfile, ConfigError> {
		match self {
			Self::Local { policy, service_owner_uid, expected_server_identity } => {
				if !matches!(
					(policy, service_owner_uid),
					(LocalTrustPolicy::Disabled, None) | (LocalTrustPolicy::SameUid, Some(_))
				) {
					return Err(ConfigError::InvalidProfile);
				}

				Ok(ServerProfile::Local(LocalProfile {
					policy,
					service_owner_uid,
					expected_server_identity,
				}))
			},
			Self::Remote { host, port, expected_server_identity } => {
				if !valid_remote_host(&host) || port == 0 {
					return Err(ConfigError::InvalidProfile);
				}

				Ok(ServerProfile::Remote(RemoteProfile { host, port, expected_server_identity }))
			},
		}
	}
}

fn validate_profiles(
	profiles: BTreeMap<ProfileName, RawProfile>,
) -> Result<BTreeMap<ProfileName, ServerProfile>, ConfigError> {
	profiles
		.into_iter()
		.map(|(name, profile)| profile.validate().map(|profile| (name, profile)))
		.collect()
}

fn validate_name(value: &str) -> Result<(), ConfigError> {
	if value.is_empty()
		|| value.len() > MAX_NAME_BYTES
		|| !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
	{
		return Err(ConfigError::Malformed);
	}

	Ok(())
}

fn valid_remote_host(host: &str) -> bool {
	if host.is_empty()
		|| host.len() > MAX_HOST_BYTES
		|| host
			.bytes()
			.any(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'\\' | b'@'))
	{
		return false;
	}

	if let Ok(address) = host.parse::<IpAddr>() {
		return !address.is_loopback();
	}

	if host.eq_ignore_ascii_case("localhost") || !host.is_ascii() {
		return false;
	}

	host.split('.').all(|label| {
		!label.is_empty()
			&& label.len() <= 63
			&& !label.starts_with('-')
			&& !label.ends_with('-')
			&& label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
	})
}
