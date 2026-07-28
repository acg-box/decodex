//! Low-level ledger row upserts.

use crate::{
	ledger::{
		self, ARTIFACT_KINDS, Connection, LedgerArtifactReader, Path, REVIEW_STATUSES,
		SIGNAL_CONFIDENCE, UPSTREAM_SUBJECT_KINDS, rusqlite,
	},
	prelude::{Result, eyre},
};

pub(super) struct CommitInput<'a> {
	pub(super) repo: &'a str,
	pub(super) sha: &'a str,
	pub(super) title: &'a str,
	pub(super) url: &'a str,
	pub(super) committed_at: Option<&'a str>,
	pub(super) pr_number: Option<i64>,
}

pub(super) struct ReviewInput<'a> {
	pub(super) repo: &'a str,
	pub(super) subject_kind: &'a str,
	pub(super) subject_id: &'a str,
	pub(super) status: &'a str,
	pub(super) reason: &'a str,
	pub(super) confidence: Option<&'a str>,
}

pub(super) struct ArtifactLinkInput<'a> {
	pub(super) repo: &'a str,
	pub(super) subject_kind: &'a str,
	pub(super) subject_id: &'a str,
	pub(super) artifact_kind: &'a str,
	pub(super) path: &'a Path,
}
pub(super) fn record_commit(connection: &Connection, input: CommitInput<'_>) -> Result<()> {
	ledger::validate_text(input.repo, "repo", ledger::MAX_IDENTIFIER_BYTES)?;
	ledger::validate_text(input.sha, "sha", ledger::MAX_IDENTIFIER_BYTES)?;
	ledger::validate_text(input.title, "title", ledger::MAX_TITLE_BYTES)?;
	ledger::validate_text(input.url, "url", ledger::MAX_URL_BYTES)?;
	if let Some(committed_at) = input.committed_at {
		ledger::validate_text(committed_at, "committed_at", 64)?;
	}
	let timestamp = ledger::utc_now_iso()?;

	ledger::bounded_write(connection, "upstream_commit", "last_seen_at", || {
		connection.execute(
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
			VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
			ON CONFLICT(repo, sha) DO UPDATE SET
			  title = excluded.title,
			  url = excluded.url,
			  committed_at = COALESCE(excluded.committed_at, upstream_commit.committed_at),
			  pr_number = COALESCE(excluded.pr_number, upstream_commit.pr_number),
			  last_seen_at = excluded.last_seen_at
			",
			rusqlite::params![
				input.repo,
				input.sha,
				input.title,
				input.url,
				input.committed_at,
				input.pr_number,
				timestamp
			],
		)?;

		Ok(())
	})
}

pub(super) fn record_review(connection: &Connection, input: ReviewInput<'_>) -> Result<()> {
	ledger::require_member(input.subject_kind, UPSTREAM_SUBJECT_KINDS, "subject_kind")?;
	ledger::require_member(input.status, REVIEW_STATUSES, "status")?;
	ledger::validate_text(input.repo, "repo", ledger::MAX_IDENTIFIER_BYTES)?;
	ledger::validate_text(input.subject_id, "subject_id", ledger::MAX_IDENTIFIER_BYTES)?;
	if input.reason.len() > ledger::MAX_EVIDENCE_TEXT_BYTES {
		eyre::bail!("reason must not exceed {} bytes", ledger::MAX_EVIDENCE_TEXT_BYTES);
	}

	if let Some(confidence) = input.confidence {
		ledger::require_member(confidence, SIGNAL_CONFIDENCE, "confidence")?;
	}

	let timestamp = ledger::utc_now_iso()?;

	ledger::bounded_write(connection, "radar_review", "updated_at", || {
		connection.execute(
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
			VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
			ON CONFLICT(repo, subject_kind, subject_id) DO UPDATE SET
			  status = excluded.status,
			  reason = excluded.reason,
			  confidence = excluded.confidence,
			  reviewed_at = excluded.reviewed_at,
			  updated_at = excluded.updated_at
			",
			rusqlite::params![
				input.repo,
				input.subject_kind,
				input.subject_id,
				input.status,
				input.reason,
				input.confidence,
				timestamp
			],
		)?;

		Ok(())
	})
}

pub(super) fn record_artifact(
	connection: &Connection,
	reader: &LedgerArtifactReader<'_>,
	input: ArtifactLinkInput<'_>,
) -> Result<()> {
	ledger::require_member(input.subject_kind, UPSTREAM_SUBJECT_KINDS, "subject_kind")?;
	ledger::require_member(input.artifact_kind, ARTIFACT_KINDS, "artifact_kind")?;

	let (sha256, size_bytes) = reader.file_digest(input.path)?;
	let created_at = ledger::utc_now_iso()?;
	let storage_path = ledger::path_for_storage(input.path)?;

	ledger::validate_text(input.repo, "repo", ledger::MAX_IDENTIFIER_BYTES)?;
	ledger::validate_text(input.subject_id, "subject_id", ledger::MAX_IDENTIFIER_BYTES)?;
	ledger::validate_text(&storage_path, "artifact path", ledger::MAX_ARTIFACT_PATH_BYTES)?;

	ledger::bounded_write(connection, "artifact_link", "created_at", || {
		connection.execute(
			"
			INSERT INTO artifact_link (
			  repo,
			  subject_kind,
			  subject_id,
			  artifact_kind,
			  path,
			  sha256,
			  size_bytes,
			  created_at
			)
			VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
			ON CONFLICT(repo, subject_kind, subject_id, artifact_kind, path) DO UPDATE SET
			  sha256 = excluded.sha256,
			  size_bytes = excluded.size_bytes,
			  created_at = excluded.created_at
			",
			rusqlite::params![
				input.repo,
				input.subject_kind,
				input.subject_id,
				input.artifact_kind,
				storage_path,
				sha256,
				size_bytes,
				created_at
			],
		)?;

		Ok(())
	})
}
