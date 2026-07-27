use std::{
	io::Cursor,
	ops::Deref,
	path::{Path, PathBuf},
};

use rusqlite::{Connection, MAIN_DB, OptionalExtension as _};

use crate::{
	LEDGER_MAX_BYTES, SCHEMA_VERSION,
	prelude::{Result, eyre},
	private_fs::{PrivateFileIdentity, RadarCacheLock},
};

const MAX_LEDGER_RECOVERY_BYTES: u64 = LEDGER_MAX_BYTES * 2;

const SCHEMA_OBJECTS_SQL: &str = "
	CREATE TABLE IF NOT EXISTS metadata (
	  key TEXT PRIMARY KEY CHECK (length(CAST(key AS BLOB)) BETWEEN 1 AND 64),
	  value TEXT NOT NULL CHECK (length(CAST(value AS BLOB)) BETWEEN 1 AND 256)
	);

	CREATE TABLE IF NOT EXISTS upstream_commit (
	  repo TEXT NOT NULL CHECK (length(CAST(repo AS BLOB)) BETWEEN 1 AND 256),
	  sha TEXT NOT NULL CHECK (length(CAST(sha AS BLOB)) BETWEEN 1 AND 256),
	  title TEXT NOT NULL CHECK (length(CAST(title AS BLOB)) BETWEEN 1 AND 1024),
	  url TEXT NOT NULL CHECK (length(CAST(url AS BLOB)) BETWEEN 1 AND 2048),
	  committed_at TEXT CHECK (length(CAST(committed_at AS BLOB)) BETWEEN 1 AND 64),
	  pr_number INTEGER,
	  first_seen_at TEXT NOT NULL CHECK (
	    length(CAST(first_seen_at AS BLOB)) BETWEEN 1 AND 64
	  ),
	  last_seen_at TEXT NOT NULL CHECK (
	    length(CAST(last_seen_at AS BLOB)) BETWEEN 1 AND 64
	  ),
	  PRIMARY KEY (repo, sha)
	);

	CREATE TABLE IF NOT EXISTS radar_review (
	  repo TEXT NOT NULL CHECK (length(CAST(repo AS BLOB)) BETWEEN 1 AND 256),
	  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
	  subject_id TEXT NOT NULL CHECK (
	    length(CAST(subject_id AS BLOB)) BETWEEN 1 AND 256
	  ),
	  status TEXT NOT NULL CHECK (
	    status IN (
	      'seen',
	      'skipped',
	      'watch',
	      'signal',
	      'control_plane',
	      'deprecated'
	    )
	  ),
	  reason TEXT NOT NULL DEFAULT '' CHECK (
	    length(CAST(reason AS BLOB)) <= 2048
	  ),
	  confidence TEXT CHECK (confidence IN ('confirmed', 'likely', 'weak')),
	  reviewed_at TEXT NOT NULL CHECK (
	    length(CAST(reviewed_at AS BLOB)) BETWEEN 1 AND 64
	  ),
	  updated_at TEXT NOT NULL CHECK (
	    length(CAST(updated_at AS BLOB)) BETWEEN 1 AND 64
	  ),
	  PRIMARY KEY (repo, subject_kind, subject_id)
	);

	CREATE TABLE IF NOT EXISTS artifact_link (
	  repo TEXT NOT NULL CHECK (length(CAST(repo AS BLOB)) BETWEEN 1 AND 256),
	  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
	  subject_id TEXT NOT NULL CHECK (
	    length(CAST(subject_id AS BLOB)) BETWEEN 1 AND 256
	  ),
	  artifact_kind TEXT NOT NULL CHECK (
	    artifact_kind IN (
	      'bundle',
	      'analysis',
	      'signal',
	      'upstream_impact',
	      'control_plane_upgrade_candidate',
	      'release_delta'
	    )
	  ),
	  path TEXT NOT NULL CHECK (length(CAST(path AS BLOB)) BETWEEN 1 AND 4096),
	  sha256 TEXT NOT NULL CHECK (length(CAST(sha256 AS BLOB)) = 64),
	  size_bytes INTEGER NOT NULL CHECK (size_bytes BETWEEN 0 AND 67108864),
	  created_at TEXT NOT NULL CHECK (
	    length(CAST(created_at AS BLOB)) BETWEEN 1 AND 64
	  ),
	  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
	);

	CREATE TABLE IF NOT EXISTS source_cache (
	  url TEXT PRIMARY KEY CHECK (length(CAST(url AS BLOB)) BETWEEN 1 AND 2048),
	  etag TEXT CHECK (length(CAST(etag AS BLOB)) BETWEEN 1 AND 1024),
	  body_sha256 TEXT NOT NULL CHECK (length(CAST(body_sha256 AS BLOB)) = 64),
	  fetched_at TEXT NOT NULL CHECK (
	    length(CAST(fetched_at AS BLOB)) BETWEEN 1 AND 64
	  ),
	  cache_path TEXT CHECK (length(CAST(cache_path AS BLOB)) BETWEEN 1 AND 4096)
	);

	CREATE INDEX IF NOT EXISTS idx_upstream_commit_pr
	  ON upstream_commit (repo, pr_number);

	CREATE INDEX IF NOT EXISTS idx_radar_review_status
	  ON radar_review (status, reviewed_at);
";

#[derive(Debug)]
pub(crate) struct RadarLedgerImage {
	connection: Option<Connection>,
	relative: PathBuf,
	original_identity: Option<PrivateFileIdentity>,
	original_bytes: Option<Vec<u8>>,
}
impl RadarLedgerImage {
	pub(crate) fn persist(
		mut self,
		lock: &RadarCacheLock,
		max_bytes: u64,
	) -> Result<PrivateFileIdentity> {
		let connection = self
			.connection
			.take()
			.ok_or_else(|| eyre::eyre!("Radar ledger connection is already closed"))?;
		crate::ledger::validate_ledger_bounds(&connection)?;
		let payload = {
			let serialized = connection.serialize(MAIN_DB)?;

			serialized.to_vec()
		};
		let payload_bytes = u64::try_from(payload.len())
			.map_err(|_| eyre::eyre!("Radar ledger serialized size is invalid"))?;
		if payload_bytes > max_bytes {
			eyre::bail!(
				"{}: Radar ledger remains above the byte limit after oldest-first retention",
				crate::ledger::bounds::OVERSIZE_INCIDENT
			);
		}

		connection.close().map_err(|(_, error)| error)?;
		if self.original_bytes.as_deref() == Some(payload.as_slice()) {
			let identity = self
				.original_identity
				.ok_or_else(|| eyre::eyre!("unchanged Radar ledger lacks an original identity"))?;

			lock.cache().verify_file(&self.relative, &identity)?;

			return Ok(identity);
		}

		lock.write_atomic_if_matches(&self.relative, self.original_identity.as_ref(), &payload)
	}
}
impl Deref for RadarLedgerImage {
	type Target = Connection;

	fn deref(&self) -> &Self::Target {
		self.connection.as_ref().expect("open Radar ledger connection must be present")
	}
}

#[derive(Debug)]
pub(crate) struct RadarLedgerConnection {
	image: RadarLedgerImage,
	lock: RadarCacheLock,
}
impl RadarLedgerConnection {
	pub(crate) fn cache_lock(&self) -> &RadarCacheLock {
		&self.lock
	}

	pub(crate) fn close(self) -> Result<()> {
		self.image.persist(&self.lock, LEDGER_MAX_BYTES)?;

		Ok(())
	}
}
impl Deref for RadarLedgerConnection {
	type Target = Connection;

	fn deref(&self) -> &Self::Target {
		&self.image
	}
}

pub(crate) fn open_ledger(path: &Path) -> Result<RadarLedgerConnection> {
	let (cache, relative) = crate::private_fs::private_cache_file(path)?;
	let lock = cache.lock()?;
	let image = open_connection_under_lock(&relative, &lock, true)?;

	Ok(RadarLedgerConnection { image, lock })
}

pub(crate) fn open_ledger_under_cache_lock(
	relative: &Path,
	lock: &RadarCacheLock,
) -> Result<RadarLedgerImage> {
	open_connection_under_lock(relative, lock, false)
}

fn open_connection_under_lock(
	relative: &Path,
	lock: &RadarCacheLock,
	validate_bounds: bool,
) -> Result<RadarLedgerImage> {
	let original_identity = lock.cache().metadata(relative)?;
	if original_identity
		.as_ref()
		.is_some_and(|identity| identity.size() > MAX_LEDGER_RECOVERY_BYTES)
	{
		eyre::bail!(
			"{}: Radar ledger exceeds the bounded recovery read limit",
			crate::ledger::bounds::OVERSIZE_INCIDENT
		);
	}
	let original_bytes = match &original_identity {
		Some(identity) => {
			let payload = lock.read_bounded(relative, MAX_LEDGER_RECOVERY_BYTES)?;

			lock.cache().verify_file(relative, identity)?;

			Some(payload)
		},
		None => None,
	};
	let mut connection = Connection::open_in_memory()?;

	if let Some(payload) = original_bytes.as_ref().filter(|payload| !payload.is_empty()) {
		connection.deserialize_read_exact(
			MAIN_DB,
			Cursor::new(payload.as_slice()),
			payload.len(),
			false,
		)?;
	}

	initialize_ledger(&connection)?;
	if validate_bounds {
		crate::ledger::validate_ledger_bounds(&connection)?;
	}

	Ok(RadarLedgerImage {
		connection: Some(connection),
		relative: relative.to_path_buf(),
		original_identity,
		original_bytes,
	})
}

pub(crate) fn initialize_ledger(connection: &Connection) -> Result<()> {
	configure_ledger_storage(connection)?;
	connection.execute_batch("BEGIN IMMEDIATE")?;
	let result = initialize_ledger_transaction(connection, InitFailureBoundary::None);

	match result {
		Ok(()) => connection.execute_batch("COMMIT")?,
		Err(error) => {
			let _ = connection.execute_batch("ROLLBACK");

			return Err(error);
		},
	}

	Ok(())
}

fn initialize_ledger_transaction(
	connection: &Connection,
	failure: InitFailureBoundary,
) -> Result<()> {
	let empty = require_current_schema_or_empty(connection)?;
	fail_initialization(failure, InitFailureBoundary::AfterInventory)?;
	if !empty {
		return Ok(());
	}
	connection.execute_batch(SCHEMA_OBJECTS_SQL)?;
	fail_initialization(failure, InitFailureBoundary::AfterObjects)?;
	connection.execute(
		"
		INSERT INTO metadata (key, value)
		VALUES ('schema_version', ?1)
		ON CONFLICT(key) DO UPDATE SET value = excluded.value
		",
		rusqlite::params![SCHEMA_VERSION.to_string()],
	)?;
	fail_initialization(failure, InitFailureBoundary::AfterVersion)?;
	verify_current_schema(connection)?;
	fail_initialization(failure, InitFailureBoundary::BeforeCommit)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InitFailureBoundary {
	AfterInventory,
	AfterObjects,
	AfterVersion,
	BeforeCommit,
	None,
}

fn fail_initialization(actual: InitFailureBoundary, expected: InitFailureBoundary) -> Result<()> {
	if actual == expected {
		eyre::bail!("injected Radar ledger initialization failure");
	}

	Ok(())
}

fn configure_ledger_storage(connection: &Connection) -> Result<()> {
	connection.execute_batch(
		"
		PRAGMA foreign_keys = ON;
		PRAGMA auto_vacuum = FULL;
		",
	)?;

	Ok(())
}

#[cfg(test)]
pub(crate) fn initialize_ledger_with_failure(
	connection: &Connection,
	boundary: &str,
) -> Result<()> {
	let boundary = match boundary {
		"after_inventory" => InitFailureBoundary::AfterInventory,
		"after_objects" => InitFailureBoundary::AfterObjects,
		"after_version" => InitFailureBoundary::AfterVersion,
		"before_commit" => InitFailureBoundary::BeforeCommit,
		_ => eyre::bail!("unknown Radar ledger initialization failure boundary"),
	};

	configure_ledger_storage(connection)?;
	connection.execute_batch("BEGIN IMMEDIATE")?;
	let result = initialize_ledger_transaction(connection, boundary);
	let _ = connection.execute_batch("ROLLBACK");

	result
}

fn require_current_schema_or_empty(connection: &Connection) -> Result<bool> {
	let user_table_count: i64 = connection.query_row(
		"
		SELECT COUNT(*)
		FROM sqlite_master
		WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
		",
		[],
		|row| row.get(0),
	)?;
	if user_table_count == 0 {
		return Ok(true);
	}

	let version = connection
		.query_row("SELECT value FROM metadata WHERE key = 'schema_version'", [], |row| {
			row.get::<_, String>(0)
		})
		.optional()
		.map_err(|_| unsupported_schema_error())?;
	let expected = SCHEMA_VERSION.to_string();

	if version.as_deref() != Some(expected.as_str()) {
		return Err(unsupported_schema_error());
	}

	verify_current_schema(connection)?;

	Ok(false)
}

fn verify_current_schema(connection: &Connection) -> Result<()> {
	let expected_connection = Connection::open_in_memory()?;

	expected_connection.execute_batch(SCHEMA_OBJECTS_SQL)?;
	let actual = schema_inventory(connection)?;
	let expected = schema_inventory(&expected_connection)?;

	if actual != expected {
		eyre::bail!(
			"Radar ledger schema {SCHEMA_VERSION} structure is not canonical; remove the local \
			cache and bootstrap a clean ledger"
		);
	}
	let auto_vacuum: i64 = connection.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
	if auto_vacuum != 1 {
		eyre::bail!(
			"Radar ledger schema {SCHEMA_VERSION} must use full auto-vacuum; remove the local \
			 cache and bootstrap a clean ledger"
		);
	}

	Ok(())
}

fn schema_inventory(connection: &Connection) -> Result<Vec<(String, String, String, String)>> {
	let mut statement = connection.prepare(
		"
		SELECT type, name, tbl_name, sql
		FROM sqlite_master
		WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL
		ORDER BY type, name
		",
	)?;
	let rows = statement.query_map([], |row| {
		let sql = row.get::<_, String>(3)?;

		Ok((
			row.get::<_, String>(0)?,
			row.get::<_, String>(1)?,
			row.get::<_, String>(2)?,
			normalize_sql(&sql),
		))
	})?;

	rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
}

fn normalize_sql(sql: &str) -> String {
	#[derive(Clone, Copy, Eq, PartialEq)]
	enum Quote {
		Bracket,
		Backtick,
		Double,
		Single,
	}

	let mut normalized = String::with_capacity(sql.len());
	let mut characters = sql.chars().peekable();
	let mut quote = None;
	let mut pending_space = false;

	while let Some(character) = characters.next() {
		if let Some(active) = quote {
			normalized.push(character);
			let closing = match active {
				Quote::Bracket => ']',
				Quote::Backtick => '`',
				Quote::Double => '"',
				Quote::Single => '\'',
			};

			if character == closing {
				if characters.peek() == Some(&closing) {
					normalized.push(characters.next().expect("peeked quote must be present"));
				} else {
					quote = None;
				}
			}

			continue;
		}

		if character.is_whitespace() {
			pending_space = !normalized.is_empty();

			continue;
		}
		if pending_space {
			normalized.push(' ');
			pending_space = false;
		}

		quote = match character {
			'[' => Some(Quote::Bracket),
			'`' => Some(Quote::Backtick),
			'"' => Some(Quote::Double),
			'\'' => Some(Quote::Single),
			_ => None,
		};
		normalized.extend(character.to_lowercase());
	}

	normalized
}

fn unsupported_schema_error() -> eyre::Report {
	eyre::eyre!(
		"unsupported Radar ledger schema; remove the obsolete local cache and bootstrap schema \
		 version {SCHEMA_VERSION}"
	)
}
