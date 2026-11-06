//! One application-owned runtime preparation shared by startup and Settings.

use std::{
	sync::{Arc, Mutex},
	thread,
};

use decodex_components::{ComponentStore, InstallProgress, RuntimeLock};
use gpui::{App, Global};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum PreparationState {
	#[default]
	Unchecked,
	Preparing(InstallProgress),
	Ready,
	Failed(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
	Install,
	Reinstall,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Snapshot {
	pub(crate) state: PreparationState,
	pub(crate) action: Option<Action>,
}

#[derive(Clone)]
pub(crate) struct RuntimePreparation {
	state: Arc<Mutex<Snapshot>>,
	requirement: Result<(ComponentStore, RuntimeLock), String>,
}

impl Default for RuntimePreparation {
	fn default() -> Self {
		Self {
			state: Arc::default(),
			requirement: RuntimeLock::embedded()
				.and_then(|lock| Ok((ComponentStore::for_user()?, lock))),
		}
	}
}

impl RuntimePreparation {
	pub(crate) fn snapshot(&self) -> Snapshot {
		self.state.lock().expect("runtime preparation").clone()
	}

	pub(crate) fn prepare(&self) {
		self.run(Action::Install);
	}

	pub(crate) fn reinstall(&self) {
		self.run(Action::Reinstall);
	}

	fn run(&self, action: Action) {
		let requirement = self.requirement.clone();
		let shared = self.state.clone();

		self.start(action, move |progress| {
			let prepared = requirement.and_then(|(store, lock)| {
				let paths = store.paths(&lock)?;

				Ok((store, lock, paths))
			});
			let (store, lock, paths) = match prepared {
				Ok(prepared) => prepared,
				Err(error) => {
					shared.lock().expect("runtime preparation").action = None;

					return Err(error);
				},
			};

			shared.lock().expect("runtime preparation").action =
				Some(if action == Action::Reinstall || paths.root.symlink_metadata().is_ok() {
					Action::Reinstall
				} else {
					Action::Install
				});

			match action {
				Action::Install => store.ensure(&lock, progress),
				Action::Reinstall => store.reinstall(&lock, progress),
			}
			.map(|_| ())
		});
	}

	fn start(
		&self,
		action: Action,
		prepare: impl FnOnce(&mut dyn FnMut(InstallProgress)) -> Result<(), String> + Send + 'static,
	) {
		let mut state = self.state.lock().expect("runtime preparation");

		if matches!(state.state, PreparationState::Preparing(_)) {
			return;
		}

		state.state = PreparationState::Preparing(InstallProgress::Waiting);
		state.action = Some(action);

		drop(state);

		let shared = Arc::clone(&self.state);
		let spawned =
			thread::Builder::new().name("decodex-runtime-setup".into()).spawn(move || {
				let result = prepare(&mut |progress| {
					shared.lock().expect("runtime preparation").state =
						PreparationState::Preparing(progress);
				});
				let mut state = shared.lock().expect("runtime preparation");

				state.state = match result {
					Ok(()) => {
						state.action = Some(Action::Reinstall);

						PreparationState::Ready
					},
					Err(error) => PreparationState::Failed(error),
				};
			});

		if let Err(error) = spawned {
			self.state.lock().expect("runtime preparation").state =
				PreparationState::Failed(error.to_string());
		}
	}

	#[cfg(any(test, feature = "visual-capture"))]
	pub(crate) fn fixture(state: PreparationState) -> Self {
		let value = Self {
			state: Arc::default(),
			requirement: Err("No runtime is available in this UI fixture.".into()),
		};

		*value.state.lock().expect("fixture preparation") = Snapshot {
			action: match state {
				PreparationState::Ready => Some(Action::Reinstall),
				PreparationState::Preparing(_) | PreparationState::Failed(_) =>
					Some(Action::Install),
				PreparationState::Unchecked => None,
			},
			state,
		};

		value
	}

	#[cfg(test)]
	pub(crate) fn finish_fixture(&self, result: Result<(), String>) {
		let mut state = self.state.lock().expect("fixture preparation");

		state.state = match result {
			Ok(()) => {
				state.action = Some(Action::Reinstall);

				PreparationState::Ready
			},
			Err(error) => PreparationState::Failed(error),
		};
	}
}

struct PreparationGlobal(RuntimePreparation);
impl Global for PreparationGlobal {}

pub(crate) fn shared(cx: &mut App) -> RuntimePreparation {
	if !cx.has_global::<PreparationGlobal>() {
		cx.set_global(PreparationGlobal(RuntimePreparation::default()));
	}

	cx.global::<PreparationGlobal>().0.clone()
}

#[cfg(test)]
mod tests {
	use std::{
		fs,
		sync::{
			Arc,
			atomic::{AtomicUsize, Ordering},
			mpsc,
		},
		thread,
		time::Duration,
	};

	use super::{Action, PreparationState, RuntimePreparation};
	use decodex_components::{ComponentStore, RuntimeLock};

	#[test]
	#[ignore = "requires the qualified native artifact directory in DECODEX_TEST_COMPONENT_ARTIFACT"]
	fn first_install_prepares_an_empty_isolated_store_from_the_qualified_artifact() {
		let artifact = std::path::PathBuf::from(
			std::env::var_os("DECODEX_TEST_COMPONENT_ARTIFACT")
				.expect("qualified artifact directory"),
		);
		let temporary = tempfile::tempdir().expect("isolated component root");
		let store = ComponentStore::new(temporary.path().join("components"));
		let lock = RuntimeLock::embedded().expect("runtime pin");
		let resolved =
			RuntimeLock::read(&artifact.join("artifact.json")).expect("qualified receipt");
		let platform = lock.target().expect("supported platform").0;
		let directory = temporary
			.path()
			.join("components/artifacts")
			.join(platform)
			.join(&lock.upstream_commit);

		fs::create_dir_all(&directory).expect("private artifact cache");
		fs::copy(artifact.join("artifact.json"), directory.join("artifact.json")).expect("receipt");
		let digest = &resolved.targets[platform].sha256;

		fs::copy(
			artifact.join(format!("{digest}.tar.gz")),
			directory.join(format!("{digest}.tar.gz")),
		)
		.expect("qualified archive");
		assert!(store.installed(&lock).expect("empty store").is_none());
		let preparation = RuntimePreparation {
			state: Arc::default(),
			requirement: Ok((store.clone(), lock.clone())),
		};

		preparation.prepare();
		let deadline = std::time::Instant::now() + Duration::from_secs(120);

		loop {
			match preparation.snapshot().state {
				PreparationState::Ready => break,
				PreparationState::Failed(error) => panic!("first install failed: {error}"),
				_ => {
					assert!(std::time::Instant::now() < deadline, "first install completes");
					std::thread::sleep(Duration::from_millis(20));
				},
			}
		}

		assert!(store.installed(&lock).expect("verified installation").is_some());

		let installed = store.paths(&lock).expect("installed paths");
		let marker = installed.root.join("previous-installation");
		let user_file = temporary.path().join("saved-conversation");

		fs::write(&marker, "old package").expect("installation marker");
		fs::write(&user_file, "keep").expect("user data");

		store.reinstall(&lock, |_| {}).expect("explicit reinstall of a healthy runtime");

		assert!(!marker.exists(), "Reinstall must replace the package even when it is valid");

		fs::write(&installed.executable, "damaged").expect("damaged private runtime");

		assert!(store.ensure(&lock, |_| {}).is_err(), "startup must not silently repair a runtime");
		assert_eq!(fs::read(&installed.executable).unwrap(), b"damaged");

		store.reinstall(&lock, |_| {}).expect("explicit repair");

		assert!(store.installed(&lock).expect("repaired installation").is_some());

		fs::write(directory.join(format!("{digest}.tar.gz")), "incomplete").unwrap();

		assert!(store.reinstall(&lock, |_| {}).is_err());
		assert!(
			store
				.installed(&lock)
				.expect("failed replacement preserves the installation")
				.is_some()
		);
		assert_eq!(fs::read_to_string(user_file).unwrap(), "keep");
	}

	#[test]
	fn preparation_starts_once_and_failed_work_can_be_retried() {
		let preparation = RuntimePreparation::default();
		let count = Arc::new(AtomicUsize::new(0));
		let (entered, started) = mpsc::channel();
		let (release, waiting) = mpsc::channel();
		let calls = Arc::clone(&count);

		preparation.start(Action::Install, move |_| {
			calls.fetch_add(1, Ordering::SeqCst);
			entered.send(()).expect("started");
			waiting.recv().expect("released");

			Err("fixture download failed".into())
		});
		started.recv_timeout(Duration::from_secs(2)).expect("startup begins preparation");
		preparation.start(Action::Reinstall, |_| {
			panic!("another click must not duplicate active preparation")
		});
		release.send(()).expect("release preparation");

		let deadline = std::time::Instant::now() + Duration::from_secs(2);

		while !matches!(preparation.snapshot().state, PreparationState::Failed(_)) {
			assert!(std::time::Instant::now() < deadline);

			thread::yield_now();
		}

		assert_eq!(count.load(Ordering::SeqCst), 1);
		assert_eq!(preparation.snapshot().action, Some(Action::Install));

		let (done, completed) = mpsc::channel();

		preparation.start(Action::Install, move |_| {
			done.send(()).expect("retry");

			Ok(())
		});
		completed.recv_timeout(Duration::from_secs(2)).expect("retry starts");
		let deadline = std::time::Instant::now() + Duration::from_secs(2);

		while preparation.snapshot().state != PreparationState::Ready {
			assert!(std::time::Instant::now() < deadline);

			thread::yield_now();
		}

		assert_eq!(preparation.snapshot().action, Some(Action::Reinstall));
	}
}
