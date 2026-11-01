//! Opt-in research connection setup. It never starts model work or alters Codex.

use std::{
	path::PathBuf,
	sync::{Arc, Mutex},
	thread,
	time::{Duration, Instant},
};

use decodex_components::{
	ComponentStore, NativePlugins, PluginAction, PluginComponent, PluginInstallation,
};
use decodex_pro_bridge::{
	Config, Connection, ConnectionState, ProBridge,
	install::{self, Installation},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
	Inspect,
	Connect,
	Disconnect,
	Install,
	Update,
	EnableTools,
	Remove,
	OpenExtension,
	RevealExtension,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Snapshot {
	pub inspected: bool,
	pub connection: Option<Connection>,
	pub installed: Option<Installation>,
	pub tools: Option<PluginInstallation>,
	pub busy: Option<Action>,
	pub error: Option<String>,
}

impl Snapshot {
	pub fn status_text(&self) -> &'static str {
		if self.installed.is_none() {
			return "Not installed";
		}
		match self.connection.as_ref().map(|state| &state.state) {
			Some(ConnectionState::Connected) => "Connected · Pro verification pending",
			Some(ConnectionState::SignInRequired) => "Sign in to ChatGPT in the research tab",
			Some(ConnectionState::VerificationRequired) =>
				"Waiting for ChatGPT security verification",
			Some(ConnectionState::AttentionRequired) => "Open the research tab to continue setup",
			None => "Load the extension in Chrome",
		}
	}
}

#[derive(Clone, Default)]
pub(crate) struct ResearchOperations {
	state: Arc<Mutex<State>>,
	/// Tests supply a private home. Production resolves the shared component store.
	directory: Option<PathBuf>,
}

#[derive(Default)]
struct State {
	shown: Snapshot,
	reading: bool,
	last_read: Option<Instant>,
	revision: u64,
}

impl ResearchOperations {
	pub fn snapshot(&self) -> Snapshot {
		self.state.lock().expect("research setup").shown.clone()
	}

	#[cfg(not(test))]
	pub fn inspect(&self) {
		self.start(Action::Inspect);
	}

	pub fn start(&self, action: Action) {
		let mut state = self.state.lock().expect("research setup");
		if state.shown.busy.is_some() {
			return;
		}
		if action == Action::Inspect {
			if (state.shown.inspected && state.shown.installed.is_none())
				|| state.reading
				|| state.last_read.is_some_and(|at| at.elapsed() < Duration::from_secs(3))
			{
				return;
			}
			state.reading = true;
		} else {
			state.shown.busy = Some(action);
			state.shown.error = None;
		}
		state.revision += 1;
		let revision = state.revision;
		drop(state);
		let owned = self.clone();
		let spawned =
			thread::Builder::new().name("decodex-research-setup".into()).spawn(move || {
				let result = owned.apply(action);
				let mut state = owned.state.lock().expect("research setup");
				if action == Action::Inspect {
					state.reading = false;
				}
				state.last_read = Some(Instant::now());
				// A passive read must not overwrite a newer user action.
				if state.revision != revision {
					return;
				}
				state.shown.busy = None;
				state.shown.inspected = true;
				match result {
					Ok(mut shown) => {
						shown.inspected = true;
						shown.busy = None;
						shown.error = None;
						state.shown = shown;
					},
					Err(error) => state.shown.error = Some(error),
				}
			});
		if let Err(error) = spawned {
			let mut state = self.state.lock().expect("research setup");
			if action == Action::Inspect {
				state.reading = false;
			}
			if state.revision == revision {
				state.shown.busy = None;
				state.shown.error = Some(error.to_string());
			}
		}
	}

	fn apply(&self, action: Action) -> Result<Snapshot, String> {
		let directory = self.directory.clone().map_or_else(
			|| ComponentStore::for_user().map(|store| store.research_directory()),
			Ok,
		)?;
		let registration = if self.directory.is_some() {
			directory.join("test-registration")
		} else {
			registration_directory()?
		};
		let mut shown = self.snapshot();
		shown.installed = install::inspect(&directory, &registration)?;
		let component = PluginComponent {
			name: "decodex-research".into(),
			marketplace: "decodex-research".into(),
			source: directory.to_string_lossy().into_owned(),
		};
		match action {
			Action::Install | Action::Update => {
				shown.installed =
					Some(install::install(&directory, &registration, &bundled_helper()?)?);
				if shown.tools.is_some() {
					shown.tools =
						NativePlugins::for_user()?.apply(&component, PluginAction::Update)?;
				}
				if action == Action::Update && directory.join("extension.sock").exists() {
					let bridge = ProBridge::new(Config::default(), &directory)?;
					research_runtime()?.block_on(bridge.reload_extension())?;
				}
				shown.connection = None;
			},
			Action::EnableTools => {
				shown.tools =
					NativePlugins::for_user()?.apply(&component, PluginAction::Install)?;
			},
			Action::Remove => {
				let bridge = ProBridge::new(Config::default(), &directory)?;
				research_runtime()?.block_on(bridge.remove_extension())?;
				install::removal_ready(&directory)?;
				let native = NativePlugins::for_user()?;
				if native.apply(&component, PluginAction::Refresh)?.is_some() {
					native.apply(&component, PluginAction::Remove)?;
				}
				install::uninstall(&directory, &registration)?;
				shown.installed = None;
				shown.tools = None;
				shown.connection = None;
			},
			Action::OpenExtension => {
				open("chrome://extensions/?id=appokabijkobnaennkffkjldbikangai", true)?;
			},
			Action::RevealExtension => {
				open(
					directory.join("extension").to_str().ok_or("Invalid extension directory")?,
					false,
				)?;
			},
			Action::Inspect | Action::Connect | Action::Disconnect => {
				// Inspect and Disconnect must not create component data on a fresh install.
				if shown.installed.is_none() {
					return Ok(shown);
				}
				let bridge = ProBridge::new(Config::default(), &directory)?;
				shown.connection = research_runtime()?.block_on(async {
					match action {
						Action::Inspect => bridge.check().await,
						Action::Connect => bridge.connect().await.map(Some),
						Action::Disconnect => {
							bridge.disconnect().await?;
							Ok(None)
						},
						_ => unreachable!(),
					}
				})?;
				if !shown.inspected && self.directory.is_none() {
					shown.tools = NativePlugins::for_user()
						.and_then(|host| host.apply(&component, PluginAction::Refresh))
						.ok()
						.flatten();
				}
			},
		}
		Ok(shown)
	}
}

fn research_runtime() -> Result<tokio::runtime::Runtime, String> {
	tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())
}

fn bundled_helper() -> Result<PathBuf, String> {
	let executable = std::env::current_exe().map_err(|e| e.to_string())?;
	let parent = executable.parent().ok_or("App location is unavailable")?;
	let helper = if parent.file_name().is_some_and(|name| name == "MacOS") {
		parent.parent().ok_or("App bundle is unavailable")?.join("Helpers/decodex-pro-bridge")
	} else {
		parent.join("decodex-pro-bridge")
	};
	if !helper.is_file() {
		return Err("This app build does not include the research component.".into());
	}
	Ok(helper)
}

fn registration_directory() -> Result<PathBuf, String> {
	let home = std::env::var_os("HOME").ok_or("Home is unavailable")?;
	Ok(PathBuf::from(home).join("Library/Application Support/Google/Chrome/NativeMessagingHosts"))
}

fn open(target: &str, chrome: bool) -> Result<(), String> {
	let mut command = std::process::Command::new("/usr/bin/open");
	if chrome {
		command.args(["-a", "Google Chrome"]);
	}
	let status = command.arg(target).status().map_err(|e| e.to_string())?;
	if !status.success() {
		return Err("The setup location could not be opened.".into());
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn visiting_components_does_not_create_component_data() {
		let root = tempfile::tempdir().unwrap();
		let directory = root.path().join("pro-research");
		let setup = ResearchOperations { directory: Some(directory.clone()), ..Default::default() };
		assert!(setup.apply(Action::Inspect).unwrap().installed.is_none());
		assert!(setup.apply(Action::Disconnect).unwrap().installed.is_none());
		assert!(!directory.exists());
	}

	#[test]
	fn a_ready_page_does_not_claim_a_verified_pro_model() {
		let state = Snapshot {
			installed: Some(Installation {
				extension_id: "fixture",
				extension_directory: "/fixture".into(),
				plugin_directory: "/fixture".into(),
				marketplace_directory: "/fixture".into(),
				registration: "/fixture".into(),
			}),
			connection: Some(Connection {
				state: ConnectionState::Connected,
				selected_model: Some("Pro".into()),
			}),
			..Default::default()
		};
		assert_eq!(state.status_text(), "Connected · Pro verification pending");
		let blocked = Snapshot {
			installed: state.installed.clone(),
			connection: Some(Connection {
				state: ConnectionState::VerificationRequired,
				selected_model: None,
			}),
			..Default::default()
		};
		assert_eq!(blocked.status_text(), "Waiting for ChatGPT security verification");
	}
}
