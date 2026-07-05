use crate::{
	prelude::{Result, eyre},
	tracker::linear::{
		LinearClient,
		queries::ISSUE_ARCHIVE_MUTATION,
		schema::{IssueArchiveData, IssueArchiveVariables},
	},
};

impl LinearClient {
	pub(crate) fn archive_issue(&self, issue_id: &str) -> Result<()> {
		let data = self.post::<_, IssueArchiveData>(
			ISSUE_ARCHIVE_MUTATION,
			&IssueArchiveVariables { id: issue_id, trash: false },
		)?;

		if !data.issue_archive.success {
			eyre::bail!("Linear did not confirm the issue archive mutation.");
		}

		Ok(())
	}
}
