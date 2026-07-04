use crate::{
	prelude::Result,
	state::{
		ReviewLifecycleRecord, StateStore,
		runtime_records::{ReviewLifecycleKey, ReviewLifecycleRuntimeRecord},
	},
};

impl StateStore {
	/// Read the runtime-owned review lifecycle record for one retained issue branch.
	pub(crate) fn review_lifecycle_record(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
	) -> Result<Option<ReviewLifecycleRecord>> {
		let state = self.lock()?;
		let key = ReviewLifecycleKey::new(project_id, issue_id, branch_name);

		Ok(state.review_lifecycle_records.get(&key).map(ReviewLifecycleRuntimeRecord::as_public))
	}

	/// Return whether any retained review lifecycle row owns this issue.
	pub(crate) fn issue_has_review_lifecycle_record(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<bool> {
		let state = self.lock()?;

		Ok(state
			.review_lifecycle_records
			.values()
			.any(|record| record.project_id == project_id && record.issue_id == issue_id))
	}
}
