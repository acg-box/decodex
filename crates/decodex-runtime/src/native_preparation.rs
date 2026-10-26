//! One daemon-owned native preflight, independent of account HTTP observations.
use std::{
	future::Future,
	path::PathBuf,
	sync::{Arc, Mutex, PoisonError},
	time::Duration,
};

use tokio::sync::watch;

use crate::{account_launch::AttestedAppServerProfile, account_service::AccountService};

#[derive(Clone)]
pub(crate) enum PreparationState {
	Pending,
	Ready(Arc<AttestedAppServerProfile>),
	Unavailable,
}

struct Inputs {
	root: PathBuf,
	accounts: Arc<AccountService>,
}

struct Inner {
	state: watch::Sender<PreparationState>,
	inputs: Mutex<Option<Inputs>>,
}

/// Shared immutable evidence after the owned preparation task completes.
#[derive(Clone)]
pub(crate) struct NativePreparation(Arc<Inner>);

impl NativePreparation {
	pub(crate) fn pending(root: PathBuf, accounts: Arc<AccountService>) -> Self {
		Self(Arc::new(Inner {
			state: watch::channel(PreparationState::Pending).0,
			inputs: Mutex::new(Some(Inputs { root, accounts })),
		}))
	}

	pub(crate) fn state(&self) -> PreparationState {
		self.0.state.borrow().clone()
	}

	pub(crate) async fn profile(&self) -> Option<AttestedAppServerProfile> {
		let mut state = self.0.state.subscribe();
		let completed =
			state.wait_for(|state| !matches!(state, PreparationState::Pending)).await.ok()?;
		match &*completed {
			PreparationState::Ready(profile) => Some(profile.as_ref().clone()),
			PreparationState::Pending | PreparationState::Unavailable => None,
		}
	}

	pub(crate) async fn daemon_service(self, mut stop: watch::Receiver<bool>) {
		if *stop.borrow() {
			self.0.state.send_replace(PreparationState::Unavailable);
			return;
		}
		let inputs = self.0.inputs.lock().unwrap_or_else(PoisonError::into_inner).take();
		if let Some(Inputs { root, accounts }) = inputs {
			self.prepare_until_stop(stop, async move {
				let profile = tokio::task::spawn_blocking(move || {
					let _phase = crate::startup_trace::Phase::new("executable_attestation");
					AttestedAppServerProfile::attest(root, Duration::from_secs(30)).ok()
				})
				.await
				.ok()
				.flatten()?;
				if !accounts
					.attest_callback_capability(profile.account_callback_attestation())
					.await
					.ok()?
				{
					return None;
				}
				Some(profile)
			})
			.await;
		} else {
			let _ = stop.wait_for(|stop| *stop).await;
		}
	}

	async fn prepare_until_stop(
		self,
		mut stop: watch::Receiver<bool>,
		prepare: impl Future<Output = Option<AttestedAppServerProfile>>,
	) {
		// Drain the bounded preflight even when shutdown starts. The existing daemon
		// task owner must retain its child and snapshot cleanup until preparation ends.
		let profile = prepare.await;
		let ready = if *stop.borrow() {
			PreparationState::Unavailable
		} else {
			profile.map_or(PreparationState::Unavailable, |profile| {
				PreparationState::Ready(Arc::new(profile))
			})
		};
		self.0.state.send_replace(ready);
		let _ = stop.wait_for(|stop| *stop).await;
	}
}

impl From<AttestedAppServerProfile> for NativePreparation {
	fn from(profile: AttestedAppServerProfile) -> Self {
		Self(Arc::new(Inner {
			state: watch::channel(PreparationState::Ready(Arc::new(profile))).0,
			inputs: Mutex::new(None),
		}))
	}
}

#[cfg(test)]
mod tests {
	use super::{Inner, NativePreparation, PreparationState};
	use std::{
		future::poll_fn,
		sync::{Arc, Mutex},
		task::Poll,
	};
	use tokio::sync::{oneshot, watch};

	fn pending() -> NativePreparation {
		NativePreparation(Arc::new(Inner {
			state: watch::channel(PreparationState::Pending).0,
			inputs: Mutex::new(None),
		}))
	}

	#[tokio::test]
	async fn pending_native_preparation_allows_other_startup_work_and_stays_owned_until_stop() {
		let temp = tempfile::tempdir().unwrap();
		let profile =
			crate::account_launch::process::tests::ordinary_runtime_fixture_profile(temp.path());
		let preparation = pending();
		let mut changed = preparation.0.state.subscribe();
		let (stop, stopped) = watch::channel(false);
		let (release, receive) = oneshot::channel();
		let owner = preparation.clone();
		let task = tokio::spawn(owner.prepare_until_stop(stopped, async move {
			receive.await.unwrap();
			Some(profile)
		}));
		let mut native = std::pin::pin!(preparation.profile());
		poll_fn(|cx| {
			assert!(native.as_mut().poll(cx).is_pending());
			Poll::Ready(())
		})
		.await;
		// Independent account observation can finish while the native proof is pending.
		let (observation, observed) = oneshot::channel();
		tokio::spawn(async move {
			observation.send(()).unwrap();
		});
		observed.await.unwrap();
		assert!(matches!(preparation.state(), PreparationState::Pending));
		release.send(()).unwrap();
		changed.changed().await.unwrap();
		assert!(native.await.is_some());
		assert!(!task.is_finished(), "a completed preflight must not end the daemon service");
		stop.send_replace(true);
		task.await.unwrap();
	}

	#[tokio::test]
	async fn failed_preparation_releases_waiters_without_admitting_a_native_process() {
		let preparation = pending();
		let mut changed = preparation.0.state.subscribe();
		let (stop, stopped) = watch::channel(false);
		let task = tokio::spawn(preparation.clone().prepare_until_stop(stopped, async { None }));
		changed.changed().await.unwrap();
		assert!(preparation.profile().await.is_none());
		assert!(!task.is_finished());
		stop.send_replace(true);
		task.await.unwrap();
	}

	#[tokio::test]
	async fn shutdown_drains_preflight_and_never_publishes_it_as_ready() {
		let temp = tempfile::tempdir().unwrap();
		let profile =
			crate::account_launch::process::tests::ordinary_runtime_fixture_profile(temp.path());
		let preparation = pending();
		let (stop, stopped) = watch::channel(false);
		let (release, receive) = oneshot::channel();
		let task = tokio::spawn(preparation.clone().prepare_until_stop(stopped, async move {
			receive.await.unwrap();
			Some(profile)
		}));
		stop.send_replace(true);
		tokio::task::yield_now().await;
		assert!(!task.is_finished(), "shutdown must retain the unfinished preflight");
		release.send(()).unwrap();
		task.await.unwrap();
		assert!(preparation.profile().await.is_none());
	}
}
