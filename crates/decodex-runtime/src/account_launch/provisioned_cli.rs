//! Resolve the official provisioned CLI launcher without executing a shell.
//! Layout and launcher from openai/codex 595cc91e8cbb1c2ca822d0311dcf12709410c582.
use super::SupervisionError;
use std::{
	fs,
	path::{Path, PathBuf},
};

const LAUNCHER: &str = r#"#!/bin/sh
set -eu
entry="$0"
while [ -L "$entry" ]; do
  parent=$(CDPATH= cd -P -- "$(dirname -- "$entry")" && pwd)
  entry=$(readlink "$entry")
  case "$entry" in /*) ;; *) entry="$parent/$entry" ;; esac
done
bin_dir=$(CDPATH= cd -P -- "$(dirname -- "$entry")" && pwd)
exec "$bin_dir/../CodexCLI.app/Contents/MacOS/codex" "$@"
"#;

pub(super) fn native_entrypoint(path: PathBuf) -> Result<PathBuf, SupervisionError> {
	let unavailable = || SupervisionError::ExecutableUnavailable;

	if path.file_name().is_none_or(|name| name != "codex")
		|| path.parent().and_then(Path::file_name).is_none_or(|name| name != "bin")
	{
		return Ok(path);
	}
	// Ordinary native packages retain their existing path. Only this exact
	// upstream launcher denotes a provisioned bundle; arbitrary scripts still fail.
	if fs::metadata(&path).map_err(|_| unavailable())?.len() != LAUNCHER.len() as u64
		|| fs::read(&path).map_err(|_| unavailable())? != LAUNCHER.as_bytes()
	{
		return Ok(path);
	}

	let package = path.parent().and_then(Path::parent).ok_or_else(unavailable)?;
	let metadata_path = package.join("codex-package.json");

	if fs::metadata(&metadata_path).map_err(|_| unavailable())?.len() > 65_536 {
		return Err(unavailable());
	}

	let metadata: serde_json::Value =
		serde_json::from_slice(&fs::read(metadata_path).map_err(|_| unavailable())?)
			.map_err(|_| unavailable())?;

	if metadata["variant"] != "codex"
		|| metadata["layoutVersion"] != 1
		|| metadata["entrypoint"] != "bin/codex"
		|| !matches!(
			metadata["target"].as_str(),
			Some("aarch64-apple-darwin" | "x86_64-apple-darwin")
		) {
		return Err(unavailable());
	}
	// Keep the executable inside its original bundle so native resource lookup
	// and provisioning remain intact. The caller snapshots and attests this image.
	package.join("CodexCLI.app/Contents/MacOS/codex").canonicalize().map_err(|_| unavailable())
}

#[cfg(test)]
mod tests {
	use super::{LAUNCHER, Path, PathBuf, fs};
	use std::os::unix::fs::{PermissionsExt as _, symlink};

	fn package(root: &Path) -> PathBuf {
		let entry = root.join("bin/codex");

		fs::create_dir_all(entry.parent().expect("provisioned package fixture"))
			.expect("provisioned package fixture");
		fs::write(&entry, LAUNCHER).expect("provisioned package fixture");
		fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))
			.expect("provisioned package fixture");
		fs::write(root.join("codex-package.json"), r#"{"variant":"codex","layoutVersion":1,"entrypoint":"bin/codex","target":"aarch64-apple-darwin"}"#).expect("provisioned package fixture");

		let native = root.join("CodexCLI.app/Contents/MacOS/codex");

		fs::create_dir_all(native.parent().expect("provisioned package fixture"))
			.expect("provisioned package fixture");
		fs::copy("/bin/echo", &native).expect("provisioned package fixture");

		let contents = root.join("CodexCLI.app/Contents");

		fs::create_dir_all(contents.join("_CodeSignature")).expect("fixture signature directory");

		for relative in ["Info.plist", "embedded.provisionprofile", "_CodeSignature/CodeResources"]
		{
			fs::write(contents.join(relative), "fixture").expect("fixture bundle context");
		}

		entry
	}

	#[test]
	fn relocated_package_and_symlink_resolve_the_native_image() {
		let home = tempfile::tempdir().expect("provisioned package fixture");

		package(&home.path().join("original"));

		let root = home.path().join("relocated package");

		fs::rename(home.path().join("original"), &root).expect("provisioned package fixture");

		let link = home.path().join("installed-codex");

		symlink("relocated package/bin/codex", &link).expect("provisioned package fixture");

		let (resolved, _, digest) = super::super::resolve_executable(link.as_os_str())
			.expect("provisioned package fixture");

		assert_eq!(
			resolved,
			root.join("CodexCLI.app/Contents/MacOS/codex")
				.canonicalize()
				.expect("provisioned package fixture")
		);
		assert_ne!(digest, [0; 32]);
		// A direct binary selection must retain the same native identity.
		assert_eq!(
			super::super::resolve_executable(resolved.as_os_str())
				.expect("provisioned package fixture")
				.2,
			digest
		);
	}

	#[test]
	fn altered_launcher_or_layout_does_not_gain_script_execution() {
		let home = tempfile::tempdir().expect("provisioned package fixture");
		let entry = package(home.path());

		fs::write(&entry, format!("{LAUNCHER}echo unexpected\n"))
			.expect("provisioned package fixture");

		assert!(super::super::resolve_executable(entry.as_os_str()).is_err());

		fs::write(&entry, LAUNCHER).expect("provisioned package fixture");
		fs::write(home.path().join("codex-package.json"), "{}")
			.expect("provisioned package fixture");

		assert!(super::super::resolve_executable(entry.as_os_str()).is_err());
	}

	#[test]
	fn bundled_target_still_requires_native_executable_validation() {
		let home = tempfile::tempdir().expect("provisioned package fixture");
		let entry = package(home.path());

		fs::write(home.path().join("CodexCLI.app/Contents/MacOS/codex"), "#!/bin/sh\nexit 0\n")
			.expect("provisioned package fixture");

		assert!(super::super::resolve_executable(entry.as_os_str()).is_err());
	}
}
