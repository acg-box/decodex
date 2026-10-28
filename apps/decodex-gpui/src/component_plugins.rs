//! Optional component operations retain native installed state across UI refreshes.

use std::{
	sync::{Arc, Mutex},
	thread,
};

use decodex_components::{NativePlugins, PluginAction, PluginComponent, PluginInstallation};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Snapshot {
	pub(crate) inspected: bool,
	pub(crate) installed: Option<PluginInstallation>,
	pub(crate) busy: Option<PluginAction>,
	pub(crate) error: Option<String>,
}

pub(crate) struct PluginOperations {
	state: Arc<Mutex<Snapshot>>,
	component: PluginComponent,
	// Explicit native homes make acceptance fixtures use the same operation path.
	native: Option<NativePlugins>,
}
impl PluginOperations {
	pub(crate) fn new(component: PluginComponent, native: Option<NativePlugins>) -> Self {
		Self { state: Arc::default(), component, native }
	}

	pub(crate) fn snapshot(&self) -> Snapshot {
		self.state.lock().expect("component operation").clone()
	}

	pub(crate) fn start(&self, action: PluginAction) {
		let mut state = self.state.lock().expect("component operation");

		if state.busy.is_some() {
			return;
		}

		state.busy = Some(action);
		state.error = None;
		drop(state);

		let shared = self.state.clone();
		let component = self.component.clone();
		let explicit = self.native.clone();
		let spawned = thread::Builder::new().name("decodex-plugin-setup".into()).spawn(move || {
			let native = explicit.map_or_else(NativePlugins::for_user, Ok);
			let result = native
				.as_ref()
				.map_err(Clone::clone)
				.and_then(|host| host.apply(&component, action));
			// A failed update can leave the old installation intact. Report the native
			// readback separately from the failed action instead of claiming removal.
			let observed = if result.is_err() {
				native.and_then(|host| host.apply(&component, PluginAction::Refresh))
			} else {
				result.clone()
			};
			let mut state = shared.lock().expect("component operation");

			state.busy = None;
			state.error = result.err();
			if let Ok(installed) = observed {
				state.installed = installed;
				state.inspected = true;
			}
		});

		if let Err(error) = spawned {
			let mut state = self.state.lock().expect("component operation");

			state.busy = None;
			state.error = Some(error.to_string());
		}
	}
}

#[cfg(test)]
mod tests {
	use std::{
		env,
		path::PathBuf,
		time::{Duration, Instant},
	};

	use super::{NativePlugins, PluginAction, PluginOperations};
	use decodex_components::egm_component;

	fn wait(operations: &PluginOperations) -> super::Snapshot {
		let deadline = Instant::now() + Duration::from_secs(120);

		loop {
			let state = operations.snapshot();

			if state.busy.is_none() {
				return state;
			}
			assert!(Instant::now() < deadline, "native component operation completes");
			std::thread::sleep(Duration::from_millis(10));
		}
	}

	#[test]
	#[ignore = "requires DECODEX_TEST_CODEX_EXECUTABLE; installs original EGM in an isolated native home"]
	fn optional_component_worker_uses_the_official_native_lifecycle() {
		let executable = PathBuf::from(
			env::var_os("DECODEX_TEST_CODEX_EXECUTABLE").expect("qualified executable"),
		);
		let home = tempfile::tempdir().expect("isolated native home");
		let native = NativePlugins::new(executable, home.path().join(".codex"));
		let operations = PluginOperations::new(egm_component(), Some(native));

		operations.start(PluginAction::Refresh);
		let state = wait(&operations);

		assert!(state.inspected && state.error.is_none() && state.installed.is_none());
		operations.start(PluginAction::Install);
		operations.start(PluginAction::Remove);
		let state = wait(&operations);

		assert!(state.error.is_none(), "native install: {:?}", state.error);
		assert!(
			state.installed.expect("native install readback").enabled,
			"busy clicks cannot queue removal"
		);
		operations.start(PluginAction::Update);
		let state = wait(&operations);

		assert!(state.error.is_none(), "native update: {:?}", state.error);
		assert!(state.installed.is_some());
		operations.start(PluginAction::Remove);
		let state = wait(&operations);

		assert!(state.inspected && state.error.is_none() && state.installed.is_none());
		assert!(!home.path().join(".codex/plugins/cache/egm/egm").exists());
		assert!(!home.path().join(".codex/.tmp/marketplaces/egm").exists());
	}
}
