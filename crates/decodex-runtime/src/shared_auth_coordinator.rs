//! Single daemon-owned coordinator for stable shared Codex auth observation and cutover.

use std::sync::{Arc, Mutex};

use crate::{
	auth_projection::{
		CodexAuthProjectionError, SharedCodexAuthFileStamp, SharedCodexAuthSnapshot,
		SharedCodexAuthVersion, project_shared_codex_auth_cas, read_shared_codex_auth_snapshot,
		read_shared_codex_auth_stamp,
	},
	host_credentials::CredentialSecretBundle,
};

const REQUIRED_STABLE_POLLS: u8 = 2;

pub(crate) trait SharedAuthFilePort: Send + Sync {
	fn stamp(&self) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError>;

	fn read(
		&self,
		expected: &SharedCodexAuthFileStamp,
	) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError>;

	fn project(
		&self,
		bundle: &CredentialSecretBundle,
		provider_account_id: &str,
		expected: &SharedCodexAuthVersion,
	) -> Result<(), CodexAuthProjectionError>;
}

struct ProductionSharedAuthFile;
impl SharedAuthFilePort for ProductionSharedAuthFile {
	fn stamp(&self) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
		read_shared_codex_auth_stamp()
	}

	fn read(
		&self,
		expected: &SharedCodexAuthFileStamp,
	) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError> {
		read_shared_codex_auth_snapshot(expected)
	}

	fn project(
		&self,
		bundle: &CredentialSecretBundle,
		provider_account_id: &str,
		expected: &SharedCodexAuthVersion,
	) -> Result<(), CodexAuthProjectionError> {
		project_shared_codex_auth_cas(bundle, provider_account_id, expected)
	}
}

#[derive(Default)]
struct StableReadState {
	candidate: Option<SharedCodexAuthFileStamp>,
	consecutive: u8,
	handled: Option<SharedCodexAuthFileStamp>,
}

pub(crate) enum StableSharedAuthPoll {
	Changed(Box<SharedCodexAuthSnapshot>),
	Unchanged,
	Waiting,
	Unavailable,
}

pub(crate) enum StableSharedAuthRead {
	Ready(Box<SharedCodexAuthSnapshot>),
	Waiting,
	Unavailable,
}

/// One coordinator instance owns all shared-auth polling and projection decisions in `decodexd`.
pub(crate) struct SharedAuthCoordinator {
	file: Arc<dyn SharedAuthFilePort>,
	state: Mutex<StableReadState>,
}

impl SharedAuthCoordinator {
	pub(crate) fn production() -> Self {
		Self {
			file: Arc::new(ProductionSharedAuthFile),
			state: Mutex::new(StableReadState::default()),
		}
	}

	#[cfg(test)]
	pub(crate) fn with_file(file: Arc<dyn SharedAuthFilePort>) -> Self {
		Self { file, state: Mutex::new(StableReadState::default()) }
	}

	pub(crate) fn poll_stable_change(&self) -> StableSharedAuthPoll {
		let stamp = match self.file.stamp() {
			Ok(stamp) => stamp,
			Err(_) => return StableSharedAuthPoll::Unavailable,
		};
		let mut state = match self.state.lock() {
			Ok(state) => state,
			Err(_) => return StableSharedAuthPoll::Unavailable,
		};
		if state.candidate.as_ref() != Some(&stamp) {
			state.candidate = Some(stamp);
			state.consecutive = 1;
			state.handled = None;
			return StableSharedAuthPoll::Waiting;
		}
		state.consecutive = state.consecutive.saturating_add(1);
		if state.consecutive < REQUIRED_STABLE_POLLS {
			return StableSharedAuthPoll::Waiting;
		}
		if state.handled.as_ref() == Some(&stamp) {
			return StableSharedAuthPoll::Unchanged;
		}
		drop(state);
		match self.file.read(&stamp) {
			Ok(snapshot) => {
				if let Ok(mut state) = self.state.lock()
					&& state.candidate.as_ref() == Some(&stamp)
				{
					state.handled = Some(stamp);
				}
				StableSharedAuthPoll::Changed(Box::new(snapshot))
			},
			Err(_) => StableSharedAuthPoll::Unavailable,
		}
	}

	pub(crate) fn read_current_stable(&self) -> StableSharedAuthRead {
		let stamp = match self.file.stamp() {
			Ok(stamp) => stamp,
			Err(_) => return StableSharedAuthRead::Unavailable,
		};
		let mut state = match self.state.lock() {
			Ok(state) => state,
			Err(_) => return StableSharedAuthRead::Unavailable,
		};
		if state.candidate.as_ref() != Some(&stamp) {
			state.candidate = Some(stamp);
			state.consecutive = 1;
			state.handled = None;
			return StableSharedAuthRead::Waiting;
		}
		state.consecutive = state.consecutive.saturating_add(1);
		if state.consecutive < REQUIRED_STABLE_POLLS {
			return StableSharedAuthRead::Waiting;
		}
		drop(state);
		match self.file.read(&stamp) {
			Ok(snapshot) => StableSharedAuthRead::Ready(Box::new(snapshot)),
			Err(_) => StableSharedAuthRead::Unavailable,
		}
	}

	pub(crate) fn read_current_exact(
		&self,
	) -> Result<Box<SharedCodexAuthSnapshot>, CodexAuthProjectionError> {
		let stamp = self.file.stamp()?;
		self.file.read(&stamp).map(Box::new)
	}

	pub(crate) fn project_exact_source(
		&self,
		bundle: &CredentialSecretBundle,
		provider_account_id: &str,
		expected: &SharedCodexAuthVersion,
	) -> Result<(), CodexAuthProjectionError> {
		self.file.project(bundle, provider_account_id, expected)
	}
}

#[cfg(test)]
mod tests {
	use std::{
		collections::VecDeque,
		sync::atomic::{AtomicUsize, Ordering},
	};

	use super::*;

	struct ScriptedFile {
		stamps: Mutex<VecDeque<Result<SharedCodexAuthFileStamp, CodexAuthProjectionError>>>,
		reads: AtomicUsize,
		failed_reads: AtomicUsize,
	}

	impl SharedAuthFilePort for ScriptedFile {
		fn stamp(&self) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
			self.stamps
				.lock()
				.expect("script lock")
				.pop_front()
				.unwrap_or(Ok(SharedCodexAuthFileStamp::Absent))
		}

		fn read(
			&self,
			expected: &SharedCodexAuthFileStamp,
		) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError> {
			self.reads.fetch_add(1, Ordering::Relaxed);
			if self
				.failed_reads
				.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
					remaining.checked_sub(1)
				})
				.is_ok()
			{
				return Err(CodexAuthProjectionError::Unavailable);
			}
			Ok(SharedCodexAuthSnapshot::Unmanaged {
				version: SharedCodexAuthVersion { stamp: expected.clone(), sha256: None },
			})
		}

		fn project(
			&self,
			_bundle: &CredentialSecretBundle,
			_provider_account_id: &str,
			_expected: &SharedCodexAuthVersion,
		) -> Result<(), CodexAuthProjectionError> {
			Ok(())
		}
	}

	#[test]
	fn exact_read_does_not_wait_for_passive_stability_polls() {
		let file = Arc::new(ScriptedFile {
			stamps: Mutex::new(VecDeque::from([Ok(SharedCodexAuthFileStamp::Absent)])),
			reads: AtomicUsize::new(0),
			failed_reads: AtomicUsize::new(0),
		});
		let coordinator = SharedAuthCoordinator::with_file(file.clone());

		assert!(matches!(
			*coordinator.read_current_exact().expect("read exact shared auth"),
			SharedCodexAuthSnapshot::Unmanaged { .. }
		));
		assert_eq!(file.reads.load(Ordering::Relaxed), 1);
	}

	#[test]
	fn stable_poll_requires_two_equal_metadata_observations_and_coalesces_reads() {
		let file = Arc::new(ScriptedFile {
			stamps: Mutex::new(VecDeque::from([
				Ok(SharedCodexAuthFileStamp::Absent),
				Ok(SharedCodexAuthFileStamp::Absent),
				Ok(SharedCodexAuthFileStamp::Absent),
			])),
			reads: AtomicUsize::new(0),
			failed_reads: AtomicUsize::new(0),
		});
		let coordinator = SharedAuthCoordinator::with_file(file.clone());
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Waiting));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Changed(_)));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Unchanged));
		assert_eq!(file.reads.load(Ordering::Relaxed), 1);
	}

	#[test]
	fn unavailable_stable_read_is_retried_without_a_metadata_change() {
		let file = Arc::new(ScriptedFile {
			stamps: Mutex::new(VecDeque::from([
				Ok(SharedCodexAuthFileStamp::Absent),
				Ok(SharedCodexAuthFileStamp::Absent),
				Ok(SharedCodexAuthFileStamp::Absent),
			])),
			reads: AtomicUsize::new(0),
			failed_reads: AtomicUsize::new(1),
		});
		let coordinator = SharedAuthCoordinator::with_file(file.clone());
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Waiting));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Unavailable));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Changed(_)));
		assert_eq!(file.reads.load(Ordering::Relaxed), 2);
	}
}
