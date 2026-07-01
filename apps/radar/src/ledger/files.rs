use std::{
	collections::BTreeSet,
	env, fs,
	path::{Path, PathBuf},
};

use sha2::{Digest as _, Sha256};

use crate::prelude::eyre;

pub(super) fn file_digest(path: &Path) -> crate::prelude::Result<(String, i64)> {
	let payload = fs::read(path)?;
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

pub(super) fn path_for_storage(path: &Path) -> crate::prelude::Result<String> {
	let resolved = path.canonicalize()?;
	let cwd = env::current_dir()?.canonicalize()?;

	Ok(resolved
		.strip_prefix(&cwd)
		.map_or_else(|_| resolved.display().to_string(), |path| path.display().to_string()))
}

pub(super) fn json_files_in_directory(directory: &Path) -> crate::prelude::Result<Vec<PathBuf>> {
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

pub(super) fn linked_signal_paths(
	bundles_dir: &Path,
	signals_dir: &Path,
) -> crate::prelude::Result<BTreeSet<PathBuf>> {
	let mut paths = BTreeSet::new();

	for bundle_path in json_files_in_directory(bundles_dir)? {
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

pub(super) fn existing_path(path: &Path) -> Option<&Path> {
	path.exists().then_some(path)
}
