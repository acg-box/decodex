//! Explicit development artifact preparation. The running app never compiles Codex.

use crate::{
	ComponentStore, InstallProgress, Result, RuntimeLock,
	install::{self, Archive},
};
use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

pub(crate) const PREPARE_REQUIRED: &str = "The pinned development component is not prepared. Run cargo make prepare-codex in this checkout, then retry.";

impl ComponentStore {
	pub(crate) fn development_directory(&self, lock: &RuntimeLock) -> Result<PathBuf> {
		Ok(self.root.join("artifacts").join(lock.target()?.0).join(&lock.upstream_commit))
	}

	pub(crate) fn resolve(&self, lock: &RuntimeLock) -> Result<Option<RuntimeLock>> {
		lock.validate()?;
		if lock.source.is_none() {
			return Ok(Some(lock.clone()));
		}
		let manifest = self.development_directory(lock)?.join("artifact.json");
		if !manifest.exists() {
			return Ok(None);
		}
		let resolved = RuntimeLock::read(&manifest)?;
		if resolved.upstream_commit != lock.upstream_commit
			|| resolved.source != lock.source
			|| resolved.version != lock.version
			|| resolved.target()?.1.schema_sha256 != lock.target()?.1.schema_sha256
			|| resolved.target()?.1.sha256.is_empty()
		{
			return Err("The prepared development artifact does not match this build's pin. Run cargo make prepare-codex, then retry.".into());
		}
		Ok(Some(resolved))
	}

	/// Import an explicitly built and qualified runtime; apply the common package checks.
	/// Archive hashes are local build evidence, while source commit and protocol remain pinned.
	pub fn import_development(
		&self,
		lock: &RuntimeLock,
		runtime: &Path,
		mut progress: impl FnMut(InstallProgress),
	) -> Result<()> {
		lock.validate()?;
		if lock.source.is_none() {
			return Err("Only development pins accept a locally prepared artifact.".into());
		}
		let directory = self.development_directory(lock)?;
		fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
		let stage = tempfile::tempdir_in(&directory).map_err(|e| e.to_string())?;
		let archive = stage.path().join("runtime.tar.gz");
		install::run(
			Command::new("/usr/bin/tar").arg("-czf").arg(&archive).arg("-C").arg(runtime).arg("."),
		)?;
		let hash = install::digest(&archive)?;
		let (executable_sha256, schema_sha256) = install::prepare_package(
			lock,
			&Archive::Local(archive.clone()),
			&hash,
			stage.path(),
			&mut progress,
		)?;
		if schema_sha256 != lock.target()?.1.schema_sha256 {
			return Err("The development artifact protocol does not match the reviewed pin.".into());
		}
		let mut resolved = lock.clone();
		let target = resolved.targets.get_mut(crate::platform()).ok_or("Missing platform.")?;
		target.sha256.clone_from(&hash);
		target.executable_sha256 = executable_sha256;
		resolved.validate()?;
		fs::rename(&archive, directory.join(format!("{hash}.tar.gz")))
			.map_err(|e| e.to_string())?;
		// Publish the selection only after the immutable installation is ready.
		// A failed import leaves the previous qualified artifact selectable.
		self.ensure_resolved(&resolved, progress)?;
		write_lock(&directory.join("artifact.json"), &resolved)?;
		Ok(())
	}
}

/// Fetch a pinned source archive through the same hash and retry path as release packages.
/// This is used by the explicit developer command, never by application startup.
pub fn prepare_source(
	lock: &RuntimeLock,
	output: &Path,
	mut progress: impl FnMut(InstallProgress),
) -> Result<()> {
	lock.validate()?;
	let source = lock.source.as_ref().ok_or("A development source pin is required.")?;
	if output.is_file() && install::digest(output)? == source.sha256 {
		return Ok(());
	}
	let parent = output.parent().ok_or("The source cache needs a directory.")?;
	fs::create_dir_all(parent).map_err(|e| e.to_string())?;
	let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
	let archive = stage.path().join("source.tar.gz");
	install::download(&source.url, &source.sha256, &archive, &mut progress)?;
	fs::rename(archive, output).map_err(|e| e.to_string())
}

pub(crate) fn write_lock(path: &Path, lock: &RuntimeLock) -> Result<()> {
	let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
	let mut output = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
	serde_json::to_writer_pretty(&mut output, lock).map_err(|e| e.to_string())?;
	output.as_file().sync_all().map_err(|e| e.to_string())?;
	output.persist(path).map_err(|e| e.to_string())?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn development_receipt_must_match_commit_source_and_schema() {
		let root = tempfile::tempdir().expect("store");
		let store = ComponentStore::new(root.path().into());
		let lock = RuntimeLock::embedded().expect("development pin");
		assert!(store.installed(&lock).expect("not prepared").is_none());
		let directory = store.development_directory(&lock).expect("directory");
		fs::create_dir_all(&directory).expect("create directory");
		let mut resolved = lock.clone();
		let target = resolved.targets.get_mut(crate::platform()).expect("platform");
		target.sha256 = "a".repeat(64);
		target.executable_sha256 = "b".repeat(64);
		write_lock(&directory.join("artifact.json"), &resolved).expect("receipt");
		assert_eq!(store.resolve(&lock).expect("resolve"), Some(resolved.clone()));
		resolved.upstream_commit = "c".repeat(40);
		resolved.source.as_mut().expect("source").url =
			format!("https://codeload.github.com/openai/codex/tar.gz/{}", resolved.upstream_commit);
		write_lock(&directory.join("artifact.json"), &resolved).expect("wrong receipt");
		assert!(store.resolve(&lock).is_err());
	}
}
