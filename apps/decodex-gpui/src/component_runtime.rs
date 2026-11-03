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

#[derive(Clone)]
pub(crate) struct RuntimePreparation {
	state: Arc<Mutex<PreparationState>>,
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
	pub(crate) fn snapshot(&self) -> PreparationState {
		self.state.lock().expect("runtime preparation").clone()
	}

	pub(crate) fn prepare(&self) {
		let requirement = self.requirement.clone();

		self.start(move |progress| {
			let (store, lock) = requirement?;

			store.ensure(&lock, progress).map(|_| ())
		});
	}

	fn start(
		&self,
		prepare: impl FnOnce(&mut dyn FnMut(InstallProgress)) -> Result<(), String> + Send + 'static,
	) {
		let mut state = self.state.lock().expect("runtime preparation");

		if matches!(*state, PreparationState::Preparing(_)) {
			return;
		}

		*state = PreparationState::Preparing(InstallProgress::Waiting);
		drop(state);

		let shared = Arc::clone(&self.state);
		let spawned =
			thread::Builder::new().name("decodex-runtime-setup".into()).spawn(move || {
				let result = prepare(&mut |progress| {
					*shared.lock().expect("runtime preparation") =
						PreparationState::Preparing(progress);
				});

				*shared.lock().expect("runtime preparation") = match result {
					Ok(()) => PreparationState::Ready,
					Err(error) => PreparationState::Failed(error),
				};
			});

		if let Err(error) = spawned {
			*self.state.lock().expect("runtime preparation") =
				PreparationState::Failed(error.to_string());
		}
	}

	#[cfg(test)]
	pub(crate) fn fixture(state: PreparationState) -> Self {
		let value = Self::default();

		*value.state.lock().expect("fixture preparation") = state;

		value
	}

	#[cfg(test)]
	pub(crate) fn finish_fixture(&self, result: Result<(), String>) {
		*self.state.lock().expect("fixture preparation") = match result {
			Ok(()) => PreparationState::Ready,
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
		sync::{
			Arc,
			atomic::{AtomicUsize, Ordering},
			mpsc,
		},
		time::Duration,
	};

	use super::{PreparationState, RuntimePreparation};
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

		std::fs::create_dir_all(&directory).expect("private artifact cache");
		std::fs::copy(artifact.join("artifact.json"), directory.join("artifact.json"))
			.expect("receipt");
		let digest = &resolved.targets[platform].sha256;

		std::fs::copy(
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
			match preparation.snapshot() {
				PreparationState::Ready => break,
				PreparationState::Failed(error) => panic!("first install failed: {error}"),
				_ => {
					assert!(std::time::Instant::now() < deadline, "first install completes");
					std::thread::sleep(Duration::from_millis(20));
				},
			}
		}

		assert!(store.installed(&lock).expect("verified installation").is_some());
	}

	#[test]
	fn preparation_starts_once_and_failed_work_can_be_retried() {
		let preparation = RuntimePreparation::default();
		let count = Arc::new(AtomicUsize::new(0));
		let (entered, started) = mpsc::channel();
		let (release, waiting) = mpsc::channel();
		let calls = Arc::clone(&count);

		preparation.start(move |_| {
			calls.fetch_add(1, Ordering::SeqCst);
			entered.send(()).expect("started");
			waiting.recv().expect("released");

			Err("fixture download failed".into())
		});
		started.recv_timeout(Duration::from_secs(2)).expect("startup begins preparation");
		preparation.start(|_| panic!("another click must not duplicate active preparation"));
		release.send(()).expect("release preparation");
		let deadline = std::time::Instant::now() + Duration::from_secs(2);

		while !matches!(preparation.snapshot(), PreparationState::Failed(_)) {
			assert!(std::time::Instant::now() < deadline);
			std::thread::yield_now();
		}

		assert_eq!(count.load(Ordering::SeqCst), 1);
		let (done, completed) = mpsc::channel();

		preparation.start(move |_| {
			done.send(()).expect("retry");
			Ok(())
		});
		completed.recv_timeout(Duration::from_secs(2)).expect("retry starts");
		let deadline = std::time::Instant::now() + Duration::from_secs(2);

		while preparation.snapshot() != PreparationState::Ready {
			assert!(std::time::Instant::now() < deadline);
			std::thread::yield_now();
		}
	}
}
