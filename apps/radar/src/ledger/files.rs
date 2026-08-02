use std::{
	collections::BTreeSet,
	env, fs,
	path::{Path, PathBuf},
};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	prelude::{Result, eyre},
	private_fs::RadarCacheLock,
};

const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

pub(super) struct LedgerArtifactReader<'a> {
	lock: &'a RadarCacheLock,
}
impl<'a> LedgerArtifactReader<'a> {
	pub(super) fn new(lock: &'a RadarCacheLock) -> Self {
		Self { lock }
	}

	pub(super) fn load_json(&self, path: &Path) -> Result<Value> {
		let payload = self.read(path)?;

		serde_json::from_slice(&payload)
			.map_err(|error| eyre::eyre!("Failed to parse JSON from {}: {error}", path.display()))
	}

	pub(super) fn file_digest(&self, path: &Path) -> Result<(String, i64)> {
		let payload = self.read(path)?;
		let size_bytes = i64::try_from(payload.len())
			.map_err(|error| eyre::eyre!("File is too large to record in ledger: {error}"))?;
		let digest = Sha256::digest(&payload);
		let digest_bytes: &[u8] = digest.as_ref();
		let mut sha256 = String::with_capacity(64);

		for &byte in digest_bytes {
			sha256.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
			sha256.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
		}

		Ok((sha256, size_bytes))
	}

	pub(super) fn json_files_in_directory(&self, directory: &Path) -> Result<Vec<PathBuf>> {
		if crate::is_radar_cache_path(directory) {
			return crate::private_fs::collect_private_json_files_under_lock_if_present(
				self.lock, directory,
			);
		}
		if !directory.exists() {
			return Ok(Vec::new());
		}
		if !directory.is_dir() {
			eyre::bail!("Radar artifact directory is not a directory: {}", directory.display());
		}

		let mut files = fs::read_dir(directory)?
			.map(|entry| entry.map(|entry| entry.path()))
			.collect::<std::result::Result<Vec<_>, _>>()?
			.into_iter()
			.filter(|path| path.extension().is_some_and(|extension| extension == "json"))
			.collect::<Vec<_>>();

		files.sort();

		Ok(files)
	}

	pub(super) fn existing_path<'b>(&self, path: &'b Path) -> Result<Option<&'b Path>> {
		self.exists(path).map(|exists| exists.then_some(path))
	}

	fn exists(&self, path: &Path) -> Result<bool> {
		if crate::is_radar_cache_path(path) {
			crate::private_fs::private_file_exists_under_lock(self.lock, path)
		} else {
			Ok(path.exists())
		}
	}

	fn read(&self, path: &Path) -> Result<Vec<u8>> {
		if crate::is_radar_cache_path(path) {
			crate::private_fs::read_private_file_under_lock(self.lock, path)
		} else {
			read_regular_artifact(path)
		}
	}
}

fn read_regular_artifact(path: &Path) -> Result<Vec<u8>> {
	crate::read_regular_file_bounded(path, MAX_ARTIFACT_BYTES, "Radar artifact")
}

pub(super) fn path_for_storage(path: &Path) -> crate::prelude::Result<String> {
	if crate::is_radar_cache_path(path) {
		if path.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
			eyre::bail!("Radar cache artifact path must not contain '..'");
		}
		let cwd = env::current_dir()?;

		return Ok(path.strip_prefix(&cwd).unwrap_or(path).display().to_string());
	}

	let resolved = path.canonicalize()?;
	let cwd = env::current_dir()?.canonicalize()?;

	Ok(resolved
		.strip_prefix(&cwd)
		.map_or_else(|_| resolved.display().to_string(), |path| path.display().to_string()))
}

pub(super) fn linked_signal_paths(
	reader: &LedgerArtifactReader<'_>,
	bundles_dir: &Path,
	signals_dir: &Path,
) -> crate::prelude::Result<BTreeSet<PathBuf>> {
	let mut paths = BTreeSet::new();

	for bundle_path in reader.json_files_in_directory(bundles_dir)? {
		let stem = file_stem(&bundle_path)?;

		paths.insert(signals_dir.join(format!("{stem}.json")));
	}

	Ok(paths)
}

pub(super) fn file_stem(path: &Path) -> crate::prelude::Result<String> {
	path.file_stem()
		.map(|stem| stem.to_string_lossy().into_owned())
		.ok_or_else(|| eyre::eyre!("Path has no file stem: {}", path.display()))
}
