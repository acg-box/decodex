//! Materialize the browser extension, native registration and opt-in MCP package.
//! Installation does not load Chrome extensions, enable a Codex plugin, or send input.

use std::{
	fs,
	io::Write as _,
	os::unix::fs::PermissionsExt as _,
	path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::json;

use crate::{
	Result,
	native_host::{EXTENSION_ID, HOST_NAME},
	store::Store,
};

/// Files to review before loading the extension or enabling the plugin.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
	/// Stable extension identity used by Chrome's native-host allowlist.
	pub extension_id: &'static str,
	/// Load this folder with Chrome's ordinary extension installation UI.
	pub extension_directory: PathBuf,
	/// Opt-in native Codex plugin. No shared Codex configuration is changed.
	pub plugin_directory: PathBuf,
	/// Local catalog for the existing native Component plugin installer.
	pub marketplace_directory: PathBuf,
	/// User-level Native Messaging registration file.
	pub registration: PathBuf,
}

/// Install into explicit directories. The executable must be the built component.
pub fn install(
	root: &Path,
	registration_directory: &Path,
	executable: &Path,
) -> Result<Installation> {
	Store::open(root)?;
	if !registration_directory.is_absolute() || !executable.is_absolute() || !executable.is_file() {
		return Err("Absolute registration and existing executable paths are required.".into());
	}
	let launcher = root.join("native-host");
	let script = format!(
		"#!/bin/sh\nexec {} --state-dir {} native-host \"$@\"\n",
		quote(executable)?,
		quote(root)?
	);
	atomic_file(&launcher, script.as_bytes())?;
	fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
	let extension_directory = root.join("extension");
	let extension = [
		("manifest.json", include_bytes!("../extension/manifest.json").as_slice()),
		("background.js", include_bytes!("../extension/background.js").as_slice()),
	];
	publish_directory(root, &extension_directory, &extension)?;
	let plugin_directory = root.join("plugin");
	let manifest = serde_json::to_vec_pretty(&json!({"name":"decodex-research","version":"0.1.0","description":"Pro research through your existing ChatGPT browser session","skills":"./skills","mcpServers":"./.mcp.json"})).map_err(|e| e.to_string())?;
	let mcp = serde_json::to_vec_pretty(&json!({"mcpServers":{"decodex_research":{"command":executable,"args":["--state-dir",root,"mcp"],"startup_timeout_sec":10,"tool_timeout_sec":30}}})).map_err(|e| e.to_string())?;
	let package = [
		(".codex-plugin/plugin.json", manifest.as_slice()),
		(".mcp.json", mcp.as_slice()),
		("skills/pro-research/SKILL.md", include_bytes!("../assets/pro-research.md").as_slice()),
	];
	publish_directory(root, &plugin_directory, &package)?;
	let catalog = root.join(".agents/plugins");
	fs::create_dir_all(&catalog).map_err(|e| e.to_string())?;
	atomic_file(
		&catalog.join("marketplace.json"),
		&serde_json::to_vec_pretty(
			&json!({"name":"decodex-research","plugins":[{"name":"decodex-research","source":"./plugin"}]}),
		)
		.map_err(|e| e.to_string())?,
	)?;
	fs::create_dir_all(registration_directory).map_err(|e| e.to_string())?;
	let registration = registration_directory.join(format!("{HOST_NAME}.json"));
	if registration.exists() {
		check_registration(&registration, &launcher)?;
	}
	let value = json!({"name":HOST_NAME,"description":"Decodex Research","path":launcher,"type":"stdio","allowed_origins":[format!("chrome-extension://{EXTENSION_ID}/")]});
	atomic_file(&registration, &serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?)?;
	Ok(Installation {
		extension_id: EXTENSION_ID,
		extension_directory,
		plugin_directory,
		marketplace_directory: root.to_owned(),
		registration,
	})
}

/// Remove only this registration and generated package. Preserve research records.
/// Chrome extension removal and native Codex plugin removal use their own installers.
pub fn uninstall(root: &Path, registration_directory: &Path) -> Result<()> {
	let store = Store::open(root)?;
	let _lease = store
		.browser_lease()?
		.ok_or("Turn off Decodex Research in Chrome before removing the component.")?;
	let launcher = root.join("native-host");
	let registration = registration_directory.join(format!("{HOST_NAME}.json"));
	if registration.exists() {
		check_registration(&registration, &launcher)?;
		fs::remove_file(registration).map_err(|e| e.to_string())?;
	}
	for name in ["extension", "plugin"] {
		let path = root.join(name);
		if path.exists() {
			fs::remove_dir_all(path).map_err(|e| e.to_string())?;
		}
	}
	if launcher.exists() {
		fs::remove_file(launcher).map_err(|e| e.to_string())?;
	}
	let catalog = root.join(".agents/plugins/marketplace.json");
	if catalog.exists() {
		fs::remove_file(catalog).map_err(|e| e.to_string())?;
	}
	Ok(())
}

fn check_registration(path: &Path, launcher: &Path) -> Result<()> {
	let value: serde_json::Value =
		serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
			.map_err(|e| e.to_string())?;
	if value["name"] != HOST_NAME || value["path"] != json!(launcher) {
		return Err("Another component owns this native-host registration.".into());
	}
	Ok(())
}

fn quote(path: &Path) -> Result<String> {
	let text = path.to_str().ok_or("Component paths must be UTF-8")?;
	Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

fn atomic_file(path: &Path, bytes: &[u8]) -> Result<()> {
	let mut file =
		tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing parent directory")?)
			.map_err(|e| e.to_string())?;
	file.write_all(bytes).map_err(|e| e.to_string())?;
	file.as_file().sync_all().map_err(|e| e.to_string())?;
	file.persist(path).map_err(|e| e.to_string())?;
	Ok(())
}

fn publish_directory(root: &Path, destination: &Path, files: &[(&str, &[u8])]) -> Result<()> {
	let staged = tempfile::tempdir_in(root).map_err(|e| e.to_string())?;
	for (name, bytes) in files {
		let path = staged.path().join(name);
		fs::create_dir_all(path.parent().ok_or("Missing package directory")?)
			.map_err(|e| e.to_string())?;
		atomic_file(&path, bytes)?;
	}
	let previous = tempfile::tempdir_in(root).map_err(|e| e.to_string())?;
	let saved = previous.path().join("package");
	if destination.exists() {
		fs::rename(destination, &saved).map_err(|e| e.to_string())?;
	}
	if let Err(error) = fs::rename(staged.path(), destination) {
		if saved.exists()
			&& let Err(restore) = fs::rename(&saved, destination)
		{
			let retained = previous.keep();
			return Err(format!(
				"Cannot publish component: {error}; cannot restore it: {restore}; previous package retained at {}",
				retained.display()
			));
		}
		return Err(error.to_string());
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn installation_updates_only_owned_artifacts_and_uninstall_keeps_results() {
		let home = tempfile::tempdir().unwrap();
		let root = home.path().join("research ' $(literal)");
		let registrations = home.path().join("native");
		let binary = std::env::current_exe().unwrap();
		let first = install(&root, &registrations, &binary).unwrap();
		fs::write(root.join("saved-result.json"), "keep").unwrap();
		assert!(first.extension_directory.join("background.js").is_file());
		let wire: serde_json::Value =
			serde_json::from_slice(&fs::read(first.plugin_directory.join(".mcp.json")).unwrap())
				.unwrap();
		assert_eq!(wire["mcpServers"]["decodex_research"]["command"], json!(binary));
		let script = fs::read_to_string(root.join("native-host")).unwrap();
		assert!(script.contains("'\\''"));
		assert!(script.contains("exec "));
		install(&root, &registrations, &binary).unwrap();
		let active = Store::open(&root).unwrap().browser_lease().unwrap().unwrap();
		assert!(uninstall(&root, &registrations).unwrap_err().contains("Turn off"));
		assert!(first.registration.exists());
		assert!(first.extension_directory.exists());
		drop(active);
		uninstall(&root, &registrations).unwrap();
		assert!(!first.registration.exists());
		assert!(!first.extension_directory.exists());
		assert_eq!(fs::read_to_string(root.join("saved-result.json")).unwrap(), "keep");
	}
}
