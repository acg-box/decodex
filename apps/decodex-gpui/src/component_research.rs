//! Opt-in research connection setup. It never starts model work or alters Codex.

use std::{
	path::PathBuf,
	sync::{Arc, Mutex},
	thread,
	time::{Duration, Instant},
};

use decodex_components::ComponentStore;
use decodex_pro_bridge::{Config, Connection, ConnectionState, ProBridge};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
	Inspect,
	Connect,
	Disconnect,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Snapshot {
	pub inspected: bool,
	pub connection: Option<Connection>,
	pub busy: Option<Action>,
	pub error: Option<String>,
}

impl Snapshot {
	pub fn status_text(&self) -> &'static str {
		match self.connection.as_ref().map(|state| &state.state) {
			Some(ConnectionState::Connected) => "Connected · Pro verification pending",
			Some(ConnectionState::SignInRequired) => "Sign in to ChatGPT in the research tab",
			Some(ConnectionState::VerificationRequired) =>
				"Waiting for ChatGPT security verification",
			Some(ConnectionState::AttentionRequired) => "Open the research tab to continue setup",
			None => "Not connected",
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
			if state.shown.error.is_some()
				|| (state.shown.inspected && state.shown.connection.is_none())
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
					Ok(connection) => {
						state.shown.connection = connection;
						state.shown.error = None;
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

	fn apply(&self, action: Action) -> Result<Option<Connection>, String> {
		let directory = self.directory.clone().map_or_else(
			|| ComponentStore::for_user().map(|store| store.research_directory()),
			Ok,
		)?;
		// Merely visiting Settings must not create component data or launch Chrome.
		if action != Action::Connect && !directory.exists() {
			return Ok(None);
		}
		let bridge = ProBridge::new(Config::default(), directory)?;
		let runtime = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.map_err(|error| error.to_string())?;
		runtime.block_on(async {
			match action {
				Action::Inspect => bridge.check().await,
				Action::Connect => bridge.connect().await.map(Some),
				Action::Disconnect => {
					bridge.disconnect().await?;
					Ok(None)
				},
			}
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn visiting_components_does_not_create_component_data() {
		let root = tempfile::tempdir().unwrap();
		let directory = root.path().join("pro-research");
		let setup = ResearchOperations { directory: Some(directory.clone()), ..Default::default() };
		assert!(setup.apply(Action::Inspect).unwrap().is_none());
		assert!(setup.apply(Action::Disconnect).unwrap().is_none());
		assert!(!directory.exists());
	}

	#[test]
	fn a_ready_page_does_not_claim_a_verified_pro_model() {
		let state = Snapshot {
			connection: Some(Connection {
				state: ConnectionState::Connected,
				selected_model: Some("Pro".into()),
			}),
			..Default::default()
		};
		assert_eq!(state.status_text(), "Connected · Pro verification pending");
		let blocked = Snapshot {
			connection: Some(Connection {
				state: ConnectionState::VerificationRequired,
				selected_model: None,
			}),
			..Default::default()
		};
		assert_eq!(blocked.status_text(), "Waiting for ChatGPT security verification");
	}
}
