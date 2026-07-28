//! Radar ledger persistence and artifact ingestion.

mod bounds;
mod commands;
mod files;
mod ingest;
mod records;
mod schema;
mod stats;
mod store;
mod subjects;

#[cfg(test)] pub(crate) use self::schema::initialize_ledger_with_failure;
pub(crate) use self::{
	commands::{
		default_ledger_path, ledger_artifact_link, ledger_bootstrap, ledger_ingest,
		ledger_ingest_existing, ledger_summary,
	},
	schema::{RadarLedgerConnection, open_ledger, open_ledger_under_cache_lock},
	store::RadarLedger,
};

use std::{
	collections::BTreeMap,
	path::{Path, PathBuf},
};

use rusqlite::{self, Connection};
use serde_json::{Map, Value};

use self::{
	bounds::{
		MAX_ARTIFACT_PATH_BYTES, MAX_EVIDENCE_TEXT_BYTES, MAX_IDENTIFIER_BYTES, MAX_TITLE_BYTES,
		MAX_URL_BYTES, bounded_write, validate_ledger_bounds, validate_text,
	},
	files::{LedgerArtifactReader, file_stem, linked_signal_paths, path_for_storage},
	ingest::{ingest_artifact_set, record_signal_artifact},
	records::{
		ArtifactLinkInput, CommitInput, ReviewInput, record_artifact, record_commit, record_review,
	},
	stats::summary_counts,
	subjects::{RadarSubject, subject_refs_for_signal},
};
use crate::{
	ARTIFACT_KINDS, BUNDLE_SCHEMA, DEFAULT_LEDGER_PATH, REVIEW_STATUSES, RecentCommit,
	SIGNAL_CONFIDENCE, SIGNAL_SCHEMA, UPSTREAM_SUBJECT_KINDS, non_empty_array, object_value,
	optional_string,
	prelude::eyre,
	requests::{
		RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
	},
	require_member, required_string, utc_now_iso, validate_artifact,
};
