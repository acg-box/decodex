//! Prefer the installed desktop CLI over a shell PATH entry on macOS.
use std::{
	env,
	ffi::OsString,
	path::{Path, PathBuf},
};

pub(super) fn find(requested: &Path) -> Option<PathBuf> {
	let mut applications = Vec::new();
	#[cfg(target_os = "macos")]
	if requested == Path::new("codex") {
		if let Some(home) = env::var_os("HOME").filter(|home| !home.is_empty()) {
			applications.push(PathBuf::from(home).join("Applications"));
		}
		applications.push(PathBuf::from("/Applications"));
	}
	select(requested, &applications, env::var_os("PATH"))
}

fn select(requested: &Path, applications: &[PathBuf], path: Option<OsString>) -> Option<PathBuf> {
	// An explicit executable path remains an explicit selection, including in native tests.
	if requested.components().count() > 1 {
		return Some(requested.to_owned());
	}
	if requested == Path::new("codex") {
		for directory in applications {
			for relative in [
				"ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
				"Codex.app/Contents/Resources/codex",
				"ChatGPT.app/Contents/Resources/codex",
			] {
				let candidate = directory.join(relative);
				if candidate.is_file() {
					return Some(candidate);
				}
			}
		}
	}
	env::split_paths(&path?)
		.map(|directory| directory.join(requested))
		.find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	#[test]
	fn app_wins_over_path_and_works_without_shell_path() {
		let root = tempfile::tempdir().unwrap();
		let app = root.path().join("Applications");
		let bundled =
			app.join("ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex");
		fs::create_dir_all(bundled.parent().unwrap()).unwrap();
		fs::write(&bundled, b"fixture").unwrap();
		let bin = root.path().join("bin");
		fs::create_dir(&bin).unwrap();
		fs::write(bin.join("codex"), b"fixture").unwrap();
		let path = env::join_paths([bin]).unwrap();
		assert_eq!(select(Path::new("codex"), &[app.clone()], Some(path)), Some(bundled.clone()));
		assert_eq!(select(Path::new("codex"), &[app], None), Some(bundled));
	}

	#[test]
	fn missing_app_uses_path_and_explicit_paths_are_preserved() {
		let root = tempfile::tempdir().unwrap();
		let binary = root.path().join("codex");
		fs::write(&binary, b"fixture").unwrap();
		let path = env::join_paths([root.path()]).unwrap();
		assert_eq!(
			select(Path::new("codex"), &[root.path().join("Applications")], Some(path)),
			Some(binary.clone())
		);
		assert_eq!(select(&binary, &[], None), Some(binary));
		assert_eq!(select(Path::new("codex"), &[], None), None);
	}

	#[test]
	fn standalone_codex_app_is_supported_without_changing_other_programs() {
		let root = tempfile::tempdir().unwrap();
		let binary = root.path().join("Codex.app/Contents/Resources/codex");
		fs::create_dir_all(binary.parent().unwrap()).unwrap();
		fs::write(&binary, b"fixture").unwrap();
		assert_eq!(select(Path::new("codex"), &[root.path().to_owned()], None), Some(binary));
		assert_eq!(select(Path::new("python3"), &[root.path().to_owned()], None), None);
	}
}
