//! Transactional Radar ledger writer.

use crate::{
	ledger::{self, CommitInput, Path, RadarLedgerConnection, RecentCommit, ReviewInput},
	prelude::Result,
};

#[derive(Debug)]
pub(crate) struct RadarLedger {
	connection: RadarLedgerConnection,
}
impl RadarLedger {
	pub(crate) fn open(path: &Path) -> Result<Self> {
		let connection = ledger::open_ledger(path)?;

		connection.execute_batch("BEGIN IMMEDIATE")?;

		Ok(Self { connection })
	}

	pub(crate) fn record_commit(
		&mut self,
		repo: &str,
		commit: &RecentCommit,
		pr_number: Option<u64>,
	) -> Result<()> {
		ledger::record_commit(
			&self.connection,
			CommitInput {
				repo,
				sha: &commit.sha,
				title: &commit.title,
				url: &commit.url,
				committed_at: commit.committed_at.as_deref(),
				pr_number: pr_number.and_then(|number| i64::try_from(number).ok()),
			},
		)
	}

	pub(crate) fn record_review(
		&mut self,
		repo: &str,
		subject_kind: &str,
		subject_id: &str,
		status: &str,
		reason: &str,
		confidence: Option<&str>,
	) -> Result<()> {
		ledger::record_review(
			&self.connection,
			ReviewInput { repo, subject_kind, subject_id, status, reason, confidence },
		)
	}

	pub(crate) fn commit(self) -> Result<()> {
		self.connection.execute_batch("COMMIT")?;
		self.connection.close()?;

		Ok(())
	}
}
