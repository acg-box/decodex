//! Transactional Radar ledger writer.

use crate::{
	ledger::{self, Connection, Path, RecentCommit, fs, rusqlite},
	prelude::Result,
};

#[derive(Debug)]
pub(crate) struct RadarLedger {
	connection: Connection,
}
impl RadarLedger {
	pub(crate) fn open(path: &Path) -> Result<Self> {
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent)?;
		}

		let connection = Connection::open(path)?;

		ledger::initialize_ledger(&connection)?;

		connection.execute_batch("BEGIN IMMEDIATE")?;

		Ok(Self { connection })
	}

	pub(crate) fn record_commit(
		&mut self,
		repo: &str,
		commit: &RecentCommit,
		pr_number: Option<u64>,
	) -> Result<()> {
		let timestamp = ledger::utc_now_iso()?;

		self.connection.execute(
			"
			INSERT INTO upstream_commit (
			  repo,
			  sha,
			  title,
			  url,
			  committed_at,
			  pr_number,
			  first_seen_at,
			  last_seen_at
			)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?)
			ON CONFLICT(repo, sha) DO UPDATE SET
			  title = excluded.title,
			  url = excluded.url,
			  committed_at = COALESCE(excluded.committed_at, upstream_commit.committed_at),
			  pr_number = COALESCE(excluded.pr_number, upstream_commit.pr_number),
			  last_seen_at = excluded.last_seen_at
			",
			rusqlite::params![
				repo,
				&commit.sha,
				&commit.title,
				&commit.url,
				&commit.committed_at,
				pr_number.and_then(|number| i64::try_from(number).ok()),
				timestamp,
				timestamp,
			],
		)?;

		Ok(())
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
		let timestamp = ledger::utc_now_iso()?;

		self.connection.execute(
			"
			INSERT INTO radar_review (
			  repo,
			  subject_kind,
			  subject_id,
			  status,
			  reason,
			  confidence,
			  reviewed_at,
			  updated_at
			)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?)
			ON CONFLICT(repo, subject_kind, subject_id) DO UPDATE SET
			  status = excluded.status,
			  reason = excluded.reason,
			  confidence = excluded.confidence,
			  reviewed_at = excluded.reviewed_at,
			  updated_at = excluded.updated_at
			",
			rusqlite::params![
				repo,
				subject_kind,
				subject_id,
				status,
				reason,
				confidence,
				&timestamp,
				&timestamp,
			],
		)?;

		Ok(())
	}

	pub(crate) fn commit(&mut self) -> Result<()> {
		self.connection.execute_batch("COMMIT")?;

		Ok(())
	}
}
