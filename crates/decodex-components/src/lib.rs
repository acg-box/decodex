//! Verified Codex components for development builds and official releases.

mod development;
mod install;
mod pin;

pub use self::{
	development::prepare_source,
	install::{ComponentStore, InstallProgress, InstalledRuntime},
	pin::pin_release,
};

use std::{
	collections::BTreeMap,
	fs,
	path::{Path, PathBuf},
};

use semver::Version;
use serde::{Deserialize, Serialize};

use development::PREPARE_REQUIRED;

/// Result of component acquisition or validation, with a user-readable failure.
pub type Result<T> = std::result::Result<T, String>;

/// The exact upstream revision and protocol required by a Decodex build.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeLock {
	/// Exact reported CLI version.
	pub version: String,
	/// Official release tag. Absent for a qualified development commit.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tag: Option<String>,
	/// Full upstream commit, never a branch or mutable tag.
	pub upstream_commit: String,
	/// Reviewed source archive used only by the explicit developer preparation command.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source: Option<SourceArchive>,
	/// Development baseline proved to be an ancestor when pinning an official release.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub preserves_upstream_commit: Option<String>,
	/// Platform-specific package and protocol evidence.
	pub targets: BTreeMap<String, RuntimeTarget>,
}
impl RuntimeLock {
	/// Read the requirement embedded at build time. Both app and helper use this input.
	pub fn embedded() -> Result<Self> {
		Self::parse(include_bytes!(concat!(env!("OUT_DIR"), "/codex-runtime.lock.json")))
	}

	/// Read a checked-in requirement or a verified local artifact receipt.
	pub fn read(path: &Path) -> Result<Self> {
		Self::parse(&fs::read(path).map_err(|e| e.to_string())?)
	}

	/// Atomically write a validated requirement or artifact receipt.
	pub fn write(&self, path: &Path) -> Result<()> {
		self.validate()?;

		development::write_lock(path, self)
	}

	/// Parse and validate all identities before using any supplied URL or path.
	pub fn parse(bytes: &[u8]) -> Result<Self> {
		let lock: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;

		lock.validate()?;

		Ok(lock)
	}

	/// Reject mutable upstream inputs and incomplete official release evidence.
	pub fn validate(&self) -> Result<()> {
		Version::parse(&self.version).map_err(|e| e.to_string())?;

		if !hex(&self.upstream_commit, 40) || self.targets.is_empty() {
			return Err("The Codex requirement needs a full upstream commit and a platform.".into());
		}

		if let Some(source) = &self.source {
			if self.tag.is_some()
				|| self.preserves_upstream_commit.is_some()
				|| !hex(&source.sha256, 64)
				|| source.url
					!= format!(
						"https://codeload.github.com/openai/codex/tar.gz/{}",
						self.upstream_commit
					) {
				return Err(
					"Development requires an exact official source commit and SHA-256.".into()
				);
			}
		} else if self.tag.as_deref() != Some(&format!("rust-v{}", self.version)) {
			return Err("The Codex requirement must select an exact official release.".into());
		}

		if self.preserves_upstream_commit.as_ref().is_some_and(|commit| !hex(commit, 40)) {
			return Err("The release coverage baseline is invalid.".into());
		}

		for (platform, target) in &self.targets {
			if !matches!(platform.as_str(), "aarch64-apple-darwin" | "x86_64-apple-darwin")
				|| !hex(&target.schema_sha256, 64)
			{
				return Err("Unsupported Codex platform or invalid protocol fingerprint.".into());
			}
			if self.source.is_some() {
				if !target.url.is_empty()
					|| (!target.sha256.is_empty() && !hex(&target.sha256, 64))
					|| (!target.executable_sha256.is_empty() && !hex(&target.executable_sha256, 64))
					|| target.sha256.is_empty() != target.executable_sha256.is_empty()
				{
					return Err("The development artifact hashes are invalid.".into());
				}
			} else {
				let url = format!(
					"https://github.com/openai/codex/releases/download/{}/codex-provisioned-package-{platform}.tar.gz",
					self.tag.as_deref().unwrap_or_default()
				);

				if target.url != url
					|| !hex(&target.sha256, 64)
					|| !hex(&target.executable_sha256, 64)
				{
					return Err("The official archive or its pinned hashes are invalid.".into());
				}
			}
		}

		Ok(())
	}

	/// Require an official release whose qualification covers the current development pin.
	pub fn validate_release(&self, baseline: &Self) -> Result<()> {
		self.validate()?;
		baseline.validate()?;

		if self.source.is_some()
			|| self.preserves_upstream_commit.as_deref() != Some(&baseline.upstream_commit)
		{
			return Err("Pin an official release that preserves the current upstream baseline before release packaging.".into());
		}

		Ok(())
	}

	/// Select the package for the current supported macOS architecture.
	pub fn target(&self) -> Result<(&'static str, &RuntimeTarget)> {
		let platform = platform();

		self.targets.get(platform).map(|target| (platform, target)).ok_or_else(|| {
			"This Decodex build has no Codex component for this architecture.".into()
		})
	}

	/// Content-addressed directory. A new artifact never overwrites another version.
	pub fn directory(&self, root: &Path) -> Result<PathBuf> {
		let (platform, target) = self.target()?;

		if !hex(&target.sha256, 64) {
			return Err(PREPARE_REQUIRED.into());
		}

		Ok(root
			.join("codex")
			.join(platform)
			.join(format!("{}-{}", self.upstream_commit, target.sha256)))
	}
}

/// Immutable official source input for a development artifact.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceArchive {
	/// Official archive URL containing the full commit.
	pub url: String,
	/// SHA-256 of that exact source archive.
	pub sha256: String,
}

/// Package hashes. Development preparation records local artifact hashes separately.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeTarget {
	/// Canonical official release asset URL; absent for local development artifacts.
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub url: String,
	/// Archive SHA-256, filled by development artifact preparation or release pinning.
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub sha256: String,
	/// Signed native executable SHA-256, filled during artifact preparation.
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub executable_sha256: String,
	/// Fingerprint of schemas generated from this executable.
	pub schema_sha256: String,
}

pub(crate) fn platform() -> &'static str {
	if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" }
}

fn hex(value: &str, length: usize) -> bool {
	value.len() == length && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
	use crate::{BTreeMap, Path, RuntimeLock, RuntimeTarget};

	pub(super) fn release() -> RuntimeLock {
		RuntimeLock {
			version: "0.162.1".into(),
			tag: Some("rust-v0.162.1".into()),
			upstream_commit: "a".repeat(40),
			source: None,
			preserves_upstream_commit: None,
			targets: BTreeMap::from([(
				crate::platform().into(),
				RuntimeTarget {
					url: format!(
						"https://github.com/openai/codex/releases/download/rust-v0.162.1/codex-provisioned-package-{}.tar.gz",
						crate::platform()
					),
					sha256: "b".repeat(64),
					executable_sha256: "c".repeat(64),
					schema_sha256: "d".repeat(64),
				},
			)]),
		}
	}

	#[test]
	fn only_exact_official_sources_are_accepted() {
		let good = release();

		assert!(good.validate().is_ok());

		for url in [
			"https://codeload.github.com/openai/codex/tar.gz/main",
			"https://example.org/codex.tar.gz",
		] {
			let mut bad = good.clone();

			bad.targets.values_mut().next().expect("target").url = url.into();

			assert!(bad.validate().is_err());
		}

		let mut bad = good;

		bad.tag = Some("latest".into());

		assert!(bad.validate().is_err());

		let mut development = RuntimeLock::embedded().expect("development requirement");

		development.upstream_commit = "main".into();

		assert!(development.validate().is_err());
	}

	#[test]
	fn new_pin_keeps_previous_runtime_and_release_needs_explicit_coverage() {
		let first = release();
		let mut second = first.clone();

		second.targets.values_mut().next().expect("target").sha256 = "e".repeat(64);

		assert_ne!(first.directory(Path::new("/cache")), second.directory(Path::new("/cache")));

		let baseline = RuntimeLock::embedded().expect("baseline");

		assert!(second.validate_release(&baseline).is_err());

		second.preserves_upstream_commit = Some(baseline.upstream_commit.clone());

		assert!(second.validate_release(&baseline).is_ok());
		assert!(baseline.validate_release(&baseline).is_err());
	}
}
