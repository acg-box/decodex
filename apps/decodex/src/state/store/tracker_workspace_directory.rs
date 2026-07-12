use crate::{
	prelude::Result,
	state::StateStore,
	tracker::{
		TrackerCredentialAttestation, TrackerWorkspaceDirectory, TrackerWorkspacePublishOutcome,
	},
};

impl StateStore {
	pub(crate) fn publish_tracker_credential_attestation(
		&self,
		attestation: TrackerCredentialAttestation,
	) -> Result<TrackerWorkspacePublishOutcome> {
		let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		let mut next = inner.tracker_workspace_directory.clone();
		let outcome = next.publish(attestation)?;

		if let Some(sqlite) = &self.sqlite {
			sqlite
				.lock()
				.unwrap_or_else(|poisoned| poisoned.into_inner())
				.persist_tracker_workspace_directory(&next)?;
		}
		inner.tracker_workspace_directory = next;

		Ok(outcome)
	}

	pub(crate) fn tracker_workspace_directory(&self) -> TrackerWorkspaceDirectory {
		self.inner
			.lock()
			.unwrap_or_else(|poisoned| poisoned.into_inner())
			.tracker_workspace_directory
			.clone()
	}
}

#[cfg(test)]
mod tests {
	use tempfile::tempdir;

	use crate::{state::StateStore, tracker::TrackerCredentialAttestation};

	#[test]
	fn workspace_directory_persists_and_reopens_without_project_authority() {
		let temp = tempdir().expect("tempdir");
		let path = temp.path().join("runtime.sqlite3");
		let store = StateStore::open(&path).expect("store");
		store
			.publish_tracker_credential_attestation(
				TrackerCredentialAttestation::linear(
					"credential-ref-1",
					"account-1",
					"workspace-1",
					"capability-1",
				)
				.expect("attestation"),
			)
			.expect("publish");
		drop(store);

		let reopened = StateStore::open(&path).expect("reopen");
		let directory = reopened.tracker_workspace_directory();
		assert_eq!(directory.epoch(), 1);
		assert_eq!(directory.quarantine_count(), 0);
	}
}
