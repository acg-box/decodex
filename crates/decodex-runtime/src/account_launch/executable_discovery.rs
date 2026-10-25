//! Desktop and development builds select the same verified component cache.
use std::{
	env,
	ffi::OsString,
	path::{Path, PathBuf},
};

pub(super) fn find(requested: &Path) -> Option<PathBuf> {
	#[cfg(target_os = "macos")]
	if requested == Path::new("codex") {
		let lock = decodex_components::RuntimeLock::embedded().ok()?;

		return decodex_components::ComponentStore::for_user()
			.ok()?
			.paths(&lock)
			.ok()
			.map(|runtime| runtime.executable);
	}

	select(requested, env::var_os("PATH"))
}

fn select(requested: &Path, path: Option<OsString>) -> Option<PathBuf> {
	// Explicit executables remain available to callers and native acceptance fixtures.
	if requested.components().count() > 1 {
		return Some(requested.to_owned());
	}

	env::split_paths(&path?)
		.map(|directory| directory.join(requested))
		.find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
	use std::fs;

	use crate::account_launch::process::executable_discovery::{self, Path, env};

	#[cfg(target_os = "macos")]
	#[test]
	fn development_and_packaged_processes_select_the_same_pinned_runtime() {
		let lock = decodex_components::RuntimeLock::embedded().expect("embedded lock");
		let expected = decodex_components::ComponentStore::for_user()
			.expect("store")
			.paths(&lock)
			.ok()
			.map(|runtime| runtime.executable);

		assert_eq!(executable_discovery::find(Path::new("codex")), expected);
	}

	#[test]
	fn named_tools_use_path_and_explicit_paths_are_preserved() {
		let root = tempfile::tempdir().unwrap();
		let binary = root.path().join("tool");

		fs::write(&binary, b"fixture").unwrap();

		let path = env::join_paths([root.path()]).unwrap();

		assert_eq!(
			executable_discovery::select(Path::new("tool"), Some(path)),
			Some(binary.clone())
		);
		assert_eq!(executable_discovery::select(&binary, None), Some(binary));
		assert_eq!(executable_discovery::select(Path::new("tool"), None), None);
	}
}
