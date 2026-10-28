//! Native Codex plugin operations. Codex owns all installed files and configuration.

use std::{
	env, fs,
	path::{Path, PathBuf},
	process::{Command, Stdio},
};

use serde::Deserialize;

use crate::{ComponentStore, Result, RuntimeLock};

/// One optional component distributed by a native Codex marketplace.
#[derive(Clone, Debug)]
pub struct PluginComponent {
	/// Native plugin name.
	pub name: String,
	/// Dedicated native marketplace name.
	pub marketplace: String,
	/// Canonical Git URL or local directory accepted by Codex.
	pub source: String,
}
impl PluginComponent {
	fn selector(&self) -> String {
		format!("{}@{}", self.name, self.marketplace)
	}
}

/// Installed state read from Codex, never from a Decodex registry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct PluginInstallation {
	/// Version reported by the native catalog, which may fall back to source metadata.
	pub version: String,
	/// Native configuration's enabled flag; not proof of installation integrity.
	pub enabled: bool,
}

/// User actions on optional components. Refresh does not change installed files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginAction {
	/// Read the native installed state.
	Refresh,
	/// Add the marketplace and install the complete native plugin.
	Install,
	/// Refresh the marketplace and replace the installed plugin from that source.
	Update,
	/// Remove the plugin and its dedicated marketplace, preserving user artifacts.
	Remove,
}

/// Narrow CLI adapter; it has no model calls, credentials, registry, or package cache.
#[derive(Clone, Debug)]
pub struct NativePlugins {
	executable: PathBuf,
	codex_home: PathBuf,
}
impl NativePlugins {
	/// Bind an already selected native executable to an explicit Codex home.
	pub fn new(executable: PathBuf, codex_home: PathBuf) -> Self {
		Self { executable, codex_home }
	}

	/// Use this build's verified runtime and the same native home as local conversations.
	pub fn for_user() -> Result<Self> {
		let runtime = ComponentStore::for_user()?
			.installed(&RuntimeLock::embedded()?)?
			.ok_or("Prepare Codex before managing optional components.")?;
		let home = env::var_os("HOME").filter(|v| !v.is_empty()).ok_or("Home is unavailable.")?;

		Ok(Self::new(runtime.executable, PathBuf::from(home).join(".codex")))
	}

	/// Perform one user action, then read the actual installed state back from Codex.
	pub fn apply(
		&self,
		component: &PluginComponent,
		action: PluginAction,
	) -> Result<Option<PluginInstallation>> {
		let marketplace = self.marketplace(component)?;

		if let Some(marketplace) = &marketplace {
			marketplace.check_source(component)?;
		}

		let selector = component.selector();

		match action {
			PluginAction::Refresh => {},
			PluginAction::Install => {
				self.run(&["marketplace", "add", &component.source, "--json"])?;
				self.run(&["add", &selector, "--json"])?;
			},
			PluginAction::Update => {
				let Some(marketplace) = marketplace.as_ref() else {
					return Err("Install the component before updating it.".into());
				};

				if marketplace
					.marketplace_source
					.as_ref()
					.is_some_and(|source| source.source_type == "git")
				{
					self.run(&["marketplace", "upgrade", &component.marketplace, "--json"])?;
				}

				self.run(&["add", &selector, "--json"])?;
			},
			PluginAction::Remove => {
				let remove_marketplace = marketplace.is_some()
					&& !self
						.list(component)?
						.installed
						.iter()
						.any(|entry| entry.plugin_id != selector);

				// Native uninstall also clears files left after external marketplace removal.
				self.run(&["remove", &selector, "--json"])?;
				if remove_marketplace {
					self.run(&["marketplace", "remove", &component.marketplace, "--json"])?;
				}
			},
		}

		let installation = self
			.list(component)?
			.installed
			.into_iter()
			.find(|plugin| plugin.plugin_id == selector)
			.map(|plugin| plugin.installation);

		match action {
			PluginAction::Install | PluginAction::Update
				if !installation.as_ref().is_some_and(|plugin| plugin.enabled) =>
				Err("Codex did not enable the component. Refresh to inspect its state.".into()),
			PluginAction::Remove if installation.is_some() =>
				Err("Codex still reports the component as installed.".into()),
			_ => Ok(installation),
		}
	}

	fn list(&self, component: &PluginComponent) -> Result<PluginList> {
		serde_json::from_slice(&self.run(&[
			"list",
			"--marketplace",
			&component.marketplace,
			"--json",
		])?)
		.map_err(|error| format!("Cannot read the native plugin state: {error}"))
	}

	fn marketplace(&self, component: &PluginComponent) -> Result<Option<Marketplace>> {
		let listed: Marketplaces =
			serde_json::from_slice(&self.run(&["marketplace", "list", "--json"])?)
				.map_err(|error| format!("Cannot read native marketplaces: {error}"))?;

		Ok(listed.marketplaces.into_iter().find(|entry| entry.name == component.marketplace))
	}

	fn run(&self, arguments: &[&str]) -> Result<Vec<u8>> {
		// Match the conversation host. In particular, do not inherit API keys,
		// CODEX_HOME overrides, shell helpers, or a project's configuration directory.
		let home = self.codex_home.parent().ok_or("Invalid native Codex home.")?;

		fs::create_dir_all(&self.codex_home).map_err(|error| error.to_string())?;

		let output = Command::new(&self.executable)
			.arg("plugin")
			.args(arguments)
			.env_clear()
			.env("HOME", home)
			.env("CODEX_HOME", &self.codex_home)
			.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
			.current_dir(home)
			.stdin(Stdio::null())
			.output()
			.map_err(|error| format!("Cannot start Codex component management: {error}"))?;

		if !output.status.success() {
			return Err(format!(
				"Codex component operation failed: {}",
				String::from_utf8_lossy(&output.stderr).trim()
			));
		}

		Ok(output.stdout)
	}
}

#[derive(Deserialize)]
struct PluginList {
	installed: Vec<InstalledEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledEntry {
	plugin_id: String,
	#[serde(flatten)]
	installation: PluginInstallation,
}

#[derive(Deserialize)]
struct Marketplaces {
	marketplaces: Vec<Marketplace>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Marketplace {
	name: String,
	marketplace_source: Option<MarketplaceSource>,
}
impl Marketplace {
	fn check_source(&self, component: &PluginComponent) -> Result<()> {
		if self.marketplace_source.as_ref().is_some_and(|source| {
			source.source == component.source
				|| (Path::new(&component.source).is_absolute()
					&& fs::canonicalize(&component.source).ok().is_some_and(|expected| {
						fs::canonicalize(&source.source).ok().as_ref() == Some(&expected)
					}))
		}) {
			Ok(())
		} else {
			Err(format!(
				"Marketplace {} uses another source. Manage it in Codex before using this component.",
				self.name
			))
		}
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketplaceSource {
	source_type: String,
	source: String,
}

#[cfg(test)]
mod tests {
	use std::{fs, path::PathBuf};

	use super::{NativePlugins, PluginAction, PluginComponent, PluginInstallation};

	fn fixture() -> (tempfile::TempDir, NativePlugins, PluginComponent) {
		let binary = PathBuf::from(
			std::env::var_os("DECODEX_TEST_CODEX_EXECUTABLE").expect("qualified native runtime"),
		);
		let root =
			tempfile::Builder::new().prefix("decodex component ").tempdir().expect("fixture");
		let home = root.path().join("home/.codex");
		let source = root.path().join("source");

		fs::create_dir_all(&home).expect("native home");
		fs::write(
			home.join("config.toml"),
			"model = \"fixture-model\"\n[analytics]\nenabled = false\n",
		)
		.expect("unrelated configuration");
		fs::write(root.path().join("work.md"), "Keep user work.").expect("user artifact");
		fs::create_dir_all(source.join(".agents/plugins")).expect("marketplace");
		fs::create_dir_all(source.join(".codex-plugin")).expect("manifest directory");
		fs::create_dir_all(source.join("skills/fixture")).expect("skill directory");
		fs::write(
			source.join(".agents/plugins/marketplace.json"),
			r#"{"name":"fixture","plugins":[{"name":"fixture","source":"./"}]}"#,
		)
		.expect("catalog");
		fs::write(
			source.join(".codex-plugin/plugin.json"),
			r#"{"name":"fixture","version":"1.0.0","skills":"./skills"}"#,
		)
		.expect("manifest");
		let skill = source.join("skills/fixture/SKILL.md");

		fs::write(
			&skill,
			"---\nname: fixture\ndescription: Native lifecycle fixture\n---\nFirst body\n",
		)
		.expect("skill");
		let component = PluginComponent {
			name: "fixture".into(),
			marketplace: "fixture".into(),
			source: source.to_str().expect("fixture path").into(),
		};
		let native = NativePlugins::new(binary, home.clone());

		(root, native, component)
	}

	#[test]
	#[ignore = "requires the qualified runtime in DECODEX_TEST_CODEX_EXECUTABLE"]
	fn native_lifecycle_installs_updates_and_removes_only_the_selected_component() {
		let (root, native, component) = fixture();
		let home = &native.codex_home;
		let source = PathBuf::from(&component.source);
		let skill = source.join("skills/fixture/SKILL.md");
		let enabled = Some(PluginInstallation { version: "1.0.0".into(), enabled: true });

		assert_eq!(native.apply(&component, PluginAction::Refresh), Ok(None));
		assert_eq!(native.apply(&component, PluginAction::Install), Ok(enabled.clone()));
		let mut foreign = component.clone();

		foreign.source.push_str("-other-source");
		assert!(native.apply(&foreign, PluginAction::Remove).is_err());

		let installed = home.join("plugins/cache/fixture/fixture/1.0.0/skills/fixture/SKILL.md");

		assert_eq!(
			fs::read_to_string(&installed).expect("installed whole plugin"),
			fs::read_to_string(&skill).expect("source skill")
		);
		fs::write(
			&skill,
			"---\nname: fixture\ndescription: Native lifecycle fixture\n---\nSecond body\n",
		)
		.expect("same-version upstream change");
		assert_eq!(native.apply(&component, PluginAction::Update), Ok(enabled.clone()));
		assert!(fs::read_to_string(&installed).expect("updated payload").contains("Second body"));
		fs::write(source.join(".codex-plugin/plugin.json"), "invalid").expect("broken upstream");
		assert!(native.apply(&component, PluginAction::Update).is_err());
		assert_eq!(native.apply(&component, PluginAction::Refresh), Ok(enabled));
		assert!(
			fs::read_to_string(&installed).expect("retained installation").contains("Second body")
		);
		assert_eq!(native.apply(&component, PluginAction::Remove), Ok(None));
		assert_eq!(native.apply(&component, PluginAction::Remove), Ok(None));
		assert!(!home.join("plugins/cache/fixture/fixture").exists());
		let config = fs::read_to_string(home.join("config.toml")).expect("native config");

		assert!(config.contains("fixture-model"));
		assert!(!config.contains("[marketplaces.fixture]"));
		assert!(!config.contains("fixture@fixture"));
		assert_eq!(
			fs::read_to_string(root.path().join("work.md")).expect("retained work"),
			"Keep user work."
		);
	}
	#[test]
	#[ignore = "requires the qualified runtime in DECODEX_TEST_CODEX_EXECUTABLE"]
	fn removal_without_a_marketplace_cleans_only_the_selected_plugin_and_can_retry() {
		for incomplete in [false, true] {
			let (root, native, component) = fixture();
			let source = PathBuf::from(&component.source);

			fs::create_dir_all(source.join("peer/.codex-plugin")).expect("peer source");
			fs::write(
				source.join("peer/.codex-plugin/plugin.json"),
				r#"{"name":"peer","version":"1.0.0"}"#,
			)
			.expect("peer manifest");
			fs::write(source.join(".agents/plugins/marketplace.json"), r#"{"name":"fixture","plugins":[{"name":"fixture","source":"./"},{"name":"peer","source":"./peer"}]}"#).expect("shared catalog");
			native.apply(&component, PluginAction::Install).expect("component installation");
			native.run(&["add", "peer@fixture", "--json"]).expect("peer installation");
			let selected = native.codex_home.join("plugins/cache/fixture/fixture");
			let peer = native
				.codex_home
				.join("plugins/cache/fixture/peer/1.0.0/.codex-plugin/plugin.json");
			let peer_manifest = fs::read(&peer).expect("installed peer");

			if incomplete {
				fs::remove_file(selected.join("1.0.0/.codex-plugin/plugin.json"))
					.expect("incomplete installation");
			}
			native
				.run(&["marketplace", "remove", "fixture", "--json"])
				.expect("independent native marketplace removal");
			assert!(native.marketplace(&component).expect("marketplaces").is_none());
			assert!(selected.exists(), "marketplace removal leaves the plugin cache");

			for _ in 0..2 {
				assert_eq!(native.apply(&component, PluginAction::Remove), Ok(None));
				assert!(!selected.exists(), "successful removal must clean the orphan cache");
				assert_eq!(fs::read(&peer).expect("retained peer"), peer_manifest);
				let config = fs::read_to_string(native.codex_home.join("config.toml"))
					.expect("native config");

				assert!(!config.contains("fixture@fixture"));
				assert!(config.contains("peer@fixture"));
				assert!(config.contains("fixture-model"));
				assert_eq!(
					fs::read_to_string(root.path().join("work.md")).unwrap(),
					"Keep user work."
				);
			}
		}
	}
	#[test]
	#[ignore = "requires the qualified runtime in DECODEX_TEST_CODEX_EXECUTABLE"]
	fn removing_a_component_keeps_another_installed_plugin_in_the_same_marketplace() {
		let (_root, native, component) = fixture();
		let source = PathBuf::from(&component.source);

		fs::create_dir_all(source.join("peer/.codex-plugin")).expect("peer source");
		fs::write(
			source.join("peer/.codex-plugin/plugin.json"),
			r#"{"name":"peer","version":"1.0.0"}"#,
		)
		.expect("peer manifest");
		fs::write(source.join(".agents/plugins/marketplace.json"), r#"{"name":"fixture","plugins":[{"name":"fixture","source":"./"},{"name":"peer","source":"./peer"}]}"#).expect("shared catalog");
		native.apply(&component, PluginAction::Install).expect("component installation");
		native.run(&["add", "peer@fixture", "--json"]).expect("independent peer installation");
		assert_eq!(native.apply(&component, PluginAction::Remove), Ok(None));
		assert!(native.marketplace(&component).expect("native catalog").is_some());
		assert!(
			native
				.list(&component)
				.expect("native installations")
				.installed
				.iter()
				.any(|entry| entry.plugin_id == "peer@fixture")
		);
		assert!(
			native
				.codex_home
				.join("plugins/cache/fixture/peer/1.0.0/.codex-plugin/plugin.json")
				.is_file()
		);
	}
}
