use std::{
	collections::{BTreeMap, BTreeSet},
	env, fs,
	path::{Path, PathBuf},
};

use rusqlite::{self, Connection, OptionalExtension as _};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::prelude::eyre;

use super::{
	ARTIFACT_KINDS, BUNDLE_SCHEMA, DEFAULT_LEDGER_PATH, REVIEW_STATUSES, RecentCommit,
	SCHEMA_VERSION, SIGNAL_CONFIDENCE, SIGNAL_SCHEMA, UPSTREAM_SUBJECT_KINDS, load_json,
	non_empty_array, object_value, optional_string,
	requests::{
		RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
	},
	require_member, required_string, utc_now_iso, validate_artifact,
};

#[derive(Debug)]
pub(crate) struct RadarLedger {
	connection: Connection,
}
impl RadarLedger {
	pub(crate) fn open(path: &Path) -> crate::prelude::Result<Self> {
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent)?;
		}

		let connection = Connection::open(path)?;

		initialize_ledger(&connection)?;

		connection.execute_batch("BEGIN IMMEDIATE")?;

		Ok(Self { connection })
	}

	pub(crate) fn record_commit(
		&mut self,
		repo: &str,
		commit: &RecentCommit,
		pr_number: Option<u64>,
	) -> crate::prelude::Result<()> {
		let timestamp = utc_now_iso()?;

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
	) -> crate::prelude::Result<()> {
		let timestamp = utc_now_iso()?;

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

	pub(crate) fn commit(&mut self) -> crate::prelude::Result<()> {
		self.connection.execute_batch("COMMIT")?;

		Ok(())
	}
}

struct CommitInput<'a> {
	repo: &'a str,
	sha: &'a str,
	title: &'a str,
	url: &'a str,
	committed_at: Option<&'a str>,
	pr_number: Option<i64>,
}

struct ReviewInput<'a> {
	repo: &'a str,
	subject_kind: &'a str,
	subject_id: &'a str,
	status: &'a str,
	reason: &'a str,
	confidence: Option<&'a str>,
}

struct ArtifactLinkInput<'a> {
	repo: &'a str,
	subject_kind: &'a str,
	subject_id: &'a str,
	artifact_kind: &'a str,
	path: &'a Path,
}

#[derive(Debug, Eq, PartialEq)]
struct RadarSubject {
	repo: String,
	subject_kind: String,
	subject_id: String,
}

/// Return the default local Radar ledger path.
pub(crate) fn default_ledger_path() -> PathBuf {
	PathBuf::from(DEFAULT_LEDGER_PATH)
}

/// Initialize the local Radar ledger schema.
pub(crate) fn ledger_bootstrap(
	request: &RadarLedgerBootstrapRequest,
) -> crate::prelude::Result<PathBuf> {
	let connection = open_ledger(&request.db_path)?;

	connection.close().map_err(|(_, error)| error)?;

	Ok(request.db_path.clone())
}

/// Ingest one bundle and optional derived artifacts into the local Radar ledger.
pub(crate) fn ledger_ingest(
	request: &RadarLedgerIngestRequest,
) -> crate::prelude::Result<BTreeMap<String, i64>> {
	let connection = open_ledger(&request.db_path)?;

	ingest_artifact_set(
		&connection,
		&request.bundle_path,
		request.analysis_path.as_deref(),
		request.signal_path.as_deref(),
	)?;

	summary_counts(&connection)
}

/// Ingest existing checked-in Radar artifacts into the local Radar ledger.
pub(crate) fn ledger_ingest_existing(
	request: &RadarLedgerIngestExistingRequest,
) -> crate::prelude::Result<BTreeMap<String, i64>> {
	let connection = open_ledger(&request.db_path)?;
	let mut ingested = 0_i64;

	for bundle_path in json_files_in_directory(&request.bundles_dir)? {
		let stem = file_stem(&bundle_path)?;
		let candidate_analysis = request.analysis_dir.join(format!("{stem}.analysis.json"));
		let candidate_signal = request.signals_dir.join(format!("{stem}.json"));

		ingest_artifact_set(
			&connection,
			&bundle_path,
			existing_path(&candidate_analysis),
			existing_path(&candidate_signal),
		)?;

		ingested += 1;
	}

	let linked_signal_paths = linked_signal_paths(&request.bundles_dir, &request.signals_dir)?;

	for signal_path in json_files_in_directory(&request.signals_dir)? {
		if linked_signal_paths.contains(&signal_path) {
			continue;
		}

		record_signal_artifact(&connection, &signal_path)?;
	}

	let mut summary = summary_counts(&connection)?;

	summary.insert("bundles_ingested".into(), ingested);

	Ok(summary)
}

/// Link one artifact path to a Radar subject in the local ledger.
pub(crate) fn ledger_artifact_link(
	request: &RadarLedgerArtifactLinkRequest,
) -> crate::prelude::Result<BTreeMap<String, i64>> {
	let connection = open_ledger(&request.db_path)?;

	record_artifact(
		&connection,
		ArtifactLinkInput {
			repo: &request.repo,
			subject_kind: &request.subject_kind,
			subject_id: &request.subject_id,
			artifact_kind: &request.artifact_kind,
			path: &request.path,
		},
	)?;

	summary_counts(&connection)
}

/// Read local Radar ledger summary counts.
pub(crate) fn ledger_summary(
	request: &RadarLedgerSummaryRequest,
) -> crate::prelude::Result<BTreeMap<String, i64>> {
	let connection = open_ledger(&request.db_path)?;

	summary_counts(&connection)
}

fn open_ledger(path: &Path) -> crate::prelude::Result<Connection> {
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}

	let connection = Connection::open(path)?;

	initialize_ledger(&connection)?;

	Ok(connection)
}

fn initialize_ledger(connection: &Connection) -> crate::prelude::Result<()> {
	connection.execute_batch(
		"
		PRAGMA foreign_keys = ON;

		CREATE TABLE IF NOT EXISTS metadata (
		  key TEXT PRIMARY KEY,
		  value TEXT NOT NULL
		);

		CREATE TABLE IF NOT EXISTS upstream_commit (
		  repo TEXT NOT NULL,
		  sha TEXT NOT NULL,
		  title TEXT NOT NULL,
		  url TEXT NOT NULL,
		  committed_at TEXT,
		  pr_number INTEGER,
		  first_seen_at TEXT NOT NULL,
		  last_seen_at TEXT NOT NULL,
		  PRIMARY KEY (repo, sha)
		);

		CREATE TABLE IF NOT EXISTS radar_review (
		  repo TEXT NOT NULL,
		  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
		  subject_id TEXT NOT NULL,
		  status TEXT NOT NULL CHECK (
		    status IN (
		      'seen',
		      'skipped',
		      'watch',
		      'signal',
		      'control_plane',
		      'deprecated',
		      'archived'
		    )
		  ),
		  reason TEXT NOT NULL DEFAULT '',
		  confidence TEXT CHECK (confidence IN ('confirmed', 'likely', 'weak')),
		  reviewed_at TEXT NOT NULL,
		  updated_at TEXT NOT NULL,
		  PRIMARY KEY (repo, subject_kind, subject_id)
		);

		CREATE TABLE IF NOT EXISTS artifact_link (
		  repo TEXT NOT NULL,
		  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
		  subject_id TEXT NOT NULL,
		  artifact_kind TEXT NOT NULL CHECK (
		    artifact_kind IN (
		      'bundle',
		      'analysis',
		      'signal',
		      'upstream_impact',
		      'control_plane_upgrade_candidate',
		      'release_delta',
		      'archive_manifest',
		      'ledger_export'
		    )
		  ),
		  path TEXT NOT NULL,
		  sha256 TEXT NOT NULL,
		  size_bytes INTEGER NOT NULL,
		  created_at TEXT NOT NULL,
		  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
		);

		CREATE TABLE IF NOT EXISTS source_cache (
		  url TEXT PRIMARY KEY,
		  etag TEXT,
		  body_sha256 TEXT NOT NULL,
		  fetched_at TEXT NOT NULL,
		  cache_path TEXT
		);

		CREATE INDEX IF NOT EXISTS idx_upstream_commit_pr
		  ON upstream_commit (repo, pr_number);

		CREATE INDEX IF NOT EXISTS idx_radar_review_status
		  ON radar_review (status, reviewed_at);
		",
	)?;

	migrate_radar_review_statuses(connection)?;
	migrate_artifact_link_kinds(connection)?;

	connection.execute(
		"
		INSERT INTO metadata (key, value)
		VALUES ('schema_version', ?1)
		ON CONFLICT(key) DO UPDATE SET value = excluded.value
		",
		rusqlite::params![SCHEMA_VERSION.to_string()],
	)?;

	Ok(())
}

fn migrate_artifact_link_kinds(connection: &Connection) -> crate::prelude::Result<()> {
	let table_sql = connection
		.query_row(
			"
			SELECT sql
			FROM sqlite_master
			WHERE type = 'table' AND name = 'artifact_link'
			",
			[],
			|row| row.get::<_, String>(0),
		)
		.optional()?;
	if table_sql.is_none() {
		return Ok(());
	};

	connection.execute_batch(
		"
		ALTER TABLE artifact_link RENAME TO artifact_link_old;

		CREATE TABLE artifact_link (
		  repo TEXT NOT NULL,
		  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
		  subject_id TEXT NOT NULL,
		  artifact_kind TEXT NOT NULL CHECK (
		    artifact_kind IN (
		      'bundle',
		      'analysis',
		      'signal',
		      'upstream_impact',
		      'control_plane_upgrade_candidate',
		      'release_delta',
		      'archive_manifest',
		      'ledger_export'
		    )
		  ),
		  path TEXT NOT NULL,
		  sha256 TEXT NOT NULL,
		  size_bytes INTEGER NOT NULL,
		  created_at TEXT NOT NULL,
		  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
		);

		INSERT OR REPLACE INTO artifact_link (
		  repo,
		  subject_kind,
		  subject_id,
		  artifact_kind,
		  path,
		  sha256,
		  size_bytes,
		  created_at
		)
		SELECT
		  repo,
		  subject_kind,
		  subject_id,
		  artifact_kind,
		  path,
		  sha256,
		  size_bytes,
		  created_at
		FROM artifact_link_old
		WHERE artifact_kind IN (
		  'bundle',
		  'analysis',
		  'signal',
		  'upstream_impact',
		  'control_plane_upgrade_candidate',
		  'release_delta',
		  'archive_manifest',
		  'ledger_export'
		);

		DROP TABLE artifact_link_old;
		",
	)?;

	Ok(())
}

fn migrate_radar_review_statuses(connection: &Connection) -> crate::prelude::Result<()> {
	let table_sql = connection
		.query_row(
			"
			SELECT sql
			FROM sqlite_master
			WHERE type = 'table' AND name = 'radar_review'
			",
			[],
			|row| row.get::<_, String>(0),
		)
		.optional()?;
	if table_sql.is_none() {
		return Ok(());
	};

	connection.execute_batch(
		"
		ALTER TABLE radar_review RENAME TO radar_review_old;

		CREATE TABLE radar_review (
		  repo TEXT NOT NULL,
		  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
		  subject_id TEXT NOT NULL,
		  status TEXT NOT NULL CHECK (
		    status IN (
		      'seen',
		      'skipped',
		      'watch',
		      'signal',
		      'control_plane',
		      'deprecated',
		      'archived'
		    )
		  ),
		  reason TEXT NOT NULL DEFAULT '',
		  confidence TEXT CHECK (confidence IN ('confirmed', 'likely', 'weak')),
		  reviewed_at TEXT NOT NULL,
		  updated_at TEXT NOT NULL,
		  PRIMARY KEY (repo, subject_kind, subject_id)
		);

		INSERT OR REPLACE INTO radar_review (
		  repo,
		  subject_kind,
		  subject_id,
		  status,
		  reason,
		  confidence,
		  reviewed_at,
		  updated_at
		)
		SELECT
		  repo,
		  subject_kind,
		  subject_id,
		  status,
		  reason,
		  confidence,
		  reviewed_at,
		  updated_at
		FROM radar_review_old
		WHERE status IN (
		  'seen',
		  'skipped',
		  'watch',
		  'signal',
		  'control_plane',
		  'deprecated',
		  'archived'
		);

		DROP TABLE radar_review_old;
		",
	)?;

	Ok(())
}

fn ingest_artifact_set(
	connection: &Connection,
	bundle_path: &Path,
	analysis_path: Option<&Path>,
	signal_path: Option<&Path>,
) -> crate::prelude::Result<()> {
	let bundle = load_json(bundle_path)?;
	let signal_exists = signal_path.is_some_and(Path::exists);
	let (repo, subject_kind, subject_id) = record_bundle(
		connection,
		&bundle,
		bundle_path,
		if signal_exists { "signal" } else { "watch" },
		"Imported from generated Radar artifacts.",
	)?;

	if let Some(path) = analysis_path.filter(|path| path.exists()) {
		record_artifact(
			connection,
			ArtifactLinkInput {
				repo: &repo,
				subject_kind: &subject_kind,
				subject_id: &subject_id,
				artifact_kind: "analysis",
				path,
			},
		)?;
	}
	if let Some(path) = signal_path.filter(|path| path.exists()) {
		let signal_subjects = record_signal_artifact(connection, path)?;

		if !signal_subjects.iter().any(|subject| {
			subject.repo == repo
				&& subject.subject_kind == subject_kind
				&& subject.subject_id == subject_id
		}) {
			record_artifact(
				connection,
				ArtifactLinkInput {
					repo: &repo,
					subject_kind: &subject_kind,
					subject_id: &subject_id,
					artifact_kind: "signal",
					path,
				},
			)?;
		}
	}

	Ok(())
}

fn record_bundle(
	connection: &Connection,
	bundle: &Value,
	bundle_path: &Path,
	status: &str,
	reason: &str,
) -> crate::prelude::Result<(String, String, String)> {
	let validation = validate_artifact(bundle);

	if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) || !validation.errors.is_empty() {
		let mut errors = validation.errors;

		if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) {
			errors.insert(0, format!("schema must be {BUNDLE_SCHEMA}"));
		}

		eyre::bail!("Bundle validation failed:\n- {}", errors.join("\n- "));
	}

	let (repo, subject_kind, subject_id) = subject_for_bundle(bundle)?;
	let bundle = object_value(bundle, "bundle")?;
	let pr_number = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| primary_pr.get("number"))
		.and_then(Value::as_i64);
	let commits = non_empty_array(bundle.get("commits"))
		.ok_or_else(|| eyre::eyre!("commits must be a non-empty list"))?;

	for commit in commits {
		let commit = object_value(commit, "commit")?;

		record_commit(
			connection,
			CommitInput {
				repo: &repo,
				sha: required_string(commit, "sha", "commit.sha")?,
				title: required_string(commit, "message", "commit.message")?,
				url: required_string(commit, "url", "commit.url")?,
				committed_at: optional_string(commit, "committed_at"),
				pr_number,
			},
		)?;
	}

	record_review(
		connection,
		ReviewInput {
			repo: &repo,
			subject_kind: &subject_kind,
			subject_id: &subject_id,
			status,
			reason,
			confidence: if status == "signal" { Some("confirmed") } else { None },
		},
	)?;
	record_artifact(
		connection,
		ArtifactLinkInput {
			repo: &repo,
			subject_kind: &subject_kind,
			subject_id: &subject_id,
			artifact_kind: "bundle",
			path: bundle_path,
		},
	)?;

	Ok((repo, subject_kind, subject_id))
}

fn subject_for_bundle(bundle: &Value) -> crate::prelude::Result<(String, String, String)> {
	let bundle = object_value(bundle, "bundle")?;
	let repo = required_string(bundle, "repo", "repo")?.to_owned();

	if let Some(number) = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| primary_pr.get("number"))
		.and_then(Value::as_u64)
	{
		return Ok((repo, "pr".into(), number.to_string()));
	}

	let commits = non_empty_array(bundle.get("commits"))
		.ok_or_else(|| eyre::eyre!("commits must be a non-empty list"))?;
	let first_commit = object_value(&commits[0], "commits[0]")?;
	let sha = required_string(first_commit, "sha", "commits[0].sha")?;

	Ok((repo, "commit".into(), sha.to_owned()))
}

fn record_signal_artifact(
	connection: &Connection,
	signal_path: &Path,
) -> crate::prelude::Result<Vec<RadarSubject>> {
	let signal = load_json(signal_path)?;
	let validation = validate_artifact(&signal);

	if validation.schema.as_deref() != Some(SIGNAL_SCHEMA) || !validation.errors.is_empty() {
		let mut errors = validation.errors;

		if validation.schema.as_deref() != Some(SIGNAL_SCHEMA) {
			errors.insert(0, format!("schema must be {SIGNAL_SCHEMA}"));
		}

		eyre::bail!(
			"Signal validation failed for {}:\n- {}",
			signal_path.display(),
			errors.join("\n- ")
		);
	}

	let signal = object_value(&signal, "signal")?;
	let slug = required_string(signal, "slug", "slug")?;
	let confidence = required_string(signal, "confidence", "confidence")?;
	let subjects = subject_refs_for_signal(signal);

	for subject in &subjects {
		record_review(
			connection,
			ReviewInput {
				repo: &subject.repo,
				subject_kind: &subject.subject_kind,
				subject_id: &subject.subject_id,
				status: "signal",
				reason: &format!("Published signal_entry/v1: {slug}"),
				confidence: Some(confidence),
			},
		)?;
		record_artifact(
			connection,
			ArtifactLinkInput {
				repo: &subject.repo,
				subject_kind: &subject.subject_kind,
				subject_id: &subject.subject_id,
				artifact_kind: "signal",
				path: signal_path,
			},
		)?;
	}

	Ok(subjects)
}

fn subject_refs_for_signal(signal: &Map<String, Value>) -> Vec<RadarSubject> {
	let Some(refs) = signal.get("source_refs").and_then(Value::as_object) else {
		return Vec::new();
	};
	let Some(repo) = refs.get("repo").and_then(Value::as_str) else {
		return Vec::new();
	};
	let mut subjects = Vec::new();

	if let Some(pr_url) = refs.get("pr_url").and_then(Value::as_str)
		&& let Some(subject_id) = parse_pr_url_subject(pr_url)
	{
		subjects.push(RadarSubject { repo: repo.into(), subject_kind: "pr".into(), subject_id });
	}
	if let Some(commit_urls) = refs.get("commit_urls").and_then(Value::as_array) {
		for url in commit_urls.iter().filter_map(Value::as_str) {
			if let Some(subject_id) = parse_commit_url_subject(url) {
				subjects.push(RadarSubject {
					repo: repo.into(),
					subject_kind: "commit".into(),
					subject_id,
				});
			}
		}
	}

	subjects
}

fn parse_pr_url_subject(url: &str) -> Option<String> {
	let (_, number) = url.trim_end_matches('/').rsplit_once("/pull/")?;

	if number.chars().all(|character| character.is_ascii_digit()) {
		Some(number.into())
	} else {
		None
	}
}

fn parse_commit_url_subject(url: &str) -> Option<String> {
	let (_, sha) = url.trim_end_matches('/').rsplit_once("/commit/")?;

	if (7..=40).contains(&sha.len()) && sha.chars().all(|character| character.is_ascii_hexdigit()) {
		Some(sha.into())
	} else {
		None
	}
}

fn record_commit(connection: &Connection, input: CommitInput<'_>) -> crate::prelude::Result<()> {
	let timestamp = utc_now_iso()?;

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
}

fn record_review(connection: &Connection, input: ReviewInput<'_>) -> crate::prelude::Result<()> {
	require_member(input.subject_kind, UPSTREAM_SUBJECT_KINDS, "subject_kind")?;
	require_member(input.status, REVIEW_STATUSES, "status")?;

	if let Some(confidence) = input.confidence {
		require_member(confidence, SIGNAL_CONFIDENCE, "confidence")?;
	}

	let timestamp = utc_now_iso()?;

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
}

fn record_artifact(
	connection: &Connection,
	input: ArtifactLinkInput<'_>,
) -> crate::prelude::Result<()> {
	require_member(input.subject_kind, UPSTREAM_SUBJECT_KINDS, "subject_kind")?;
	require_member(input.artifact_kind, ARTIFACT_KINDS, "artifact_kind")?;

	let (sha256, size_bytes) = file_digest(input.path)?;
	let created_at = utc_now_iso()?;
	let storage_path = path_for_storage(input.path)?;

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
}

fn summary_counts(connection: &Connection) -> crate::prelude::Result<BTreeMap<String, i64>> {
	let mut result = BTreeMap::new();

	for (key, table) in [
		("upstream_commits", "upstream_commit"),
		("radar_reviews", "radar_review"),
		("artifact_links", "artifact_link"),
		("source_cache_entries", "source_cache"),
	] {
		let count =
			connection.query_row(&format!("SELECT COUNT(*) AS count FROM {table}"), [], |row| {
				row.get::<_, i64>(0)
			})?;

		result.insert(key.into(), count);
	}

	Ok(result)
}

fn file_digest(path: &Path) -> crate::prelude::Result<(String, i64)> {
	let payload = fs::read(path)?;
	let size_bytes = i64::try_from(payload.len())
		.map_err(|error| eyre::eyre!("File is too large to record in ledger: {error}"))?;
	let digest = Sha256::digest(&payload);
	let digest_bytes: &[u8] = digest.as_ref();
	let mut sha256 = String::with_capacity(64);

	for &byte in digest_bytes {
		sha256.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		sha256.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	Ok((sha256, size_bytes))
}

fn path_for_storage(path: &Path) -> crate::prelude::Result<String> {
	let resolved = path.canonicalize()?;
	let cwd = env::current_dir()?.canonicalize()?;

	Ok(resolved
		.strip_prefix(&cwd)
		.map_or_else(|_| resolved.display().to_string(), |path| path.display().to_string()))
}

fn json_files_in_directory(directory: &Path) -> crate::prelude::Result<Vec<PathBuf>> {
	if !directory.exists() {
		return Ok(Vec::new());
	}
	if !directory.is_dir() {
		eyre::bail!("Radar artifact directory is not a directory: {}", directory.display());
	}

	let mut files = fs::read_dir(directory)?
		.map(|entry| entry.map(|entry| entry.path()))
		.collect::<std::result::Result<Vec<_>, _>>()?
		.into_iter()
		.filter(|path| path.extension().is_some_and(|extension| extension == "json"))
		.collect::<Vec<_>>();

	files.sort();

	Ok(files)
}

fn linked_signal_paths(
	bundles_dir: &Path,
	signals_dir: &Path,
) -> crate::prelude::Result<BTreeSet<PathBuf>> {
	let mut paths = BTreeSet::new();

	for bundle_path in json_files_in_directory(bundles_dir)? {
		let stem = file_stem(&bundle_path)?;

		paths.insert(signals_dir.join(format!("{stem}.json")));
	}

	Ok(paths)
}

fn file_stem(path: &Path) -> crate::prelude::Result<String> {
	path.file_stem()
		.map(|stem| stem.to_string_lossy().into_owned())
		.ok_or_else(|| eyre::eyre!("Path has no file stem: {}", path.display()))
}

fn existing_path(path: &Path) -> Option<&Path> {
	path.exists().then_some(path)
}
