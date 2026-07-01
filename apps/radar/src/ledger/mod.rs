//! Radar ledger persistence and artifact ingestion.

mod commands;
mod files;
mod ingest;
mod records;
mod schema;
mod stats;
mod store;
mod subjects;

use std::{
	collections::BTreeMap,
	fs,
	path::{Path, PathBuf},
};

use rusqlite::{self, Connection};
use serde_json::{Map, Value};

use crate::prelude::eyre;

use self::{
	files::{
		existing_path, file_digest, file_stem, json_files_in_directory, linked_signal_paths,
		path_for_storage,
	},
	ingest::{ingest_artifact_set, record_signal_artifact},
	records::{
		ArtifactLinkInput, CommitInput, ReviewInput, record_artifact, record_commit, record_review,
	},
	schema::{initialize_ledger, open_ledger},
	stats::summary_counts,
	subjects::{RadarSubject, subject_refs_for_signal},
};

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

pub(crate) use commands::{
	default_ledger_path, ledger_artifact_link, ledger_bootstrap, ledger_ingest,
	ledger_ingest_existing, ledger_summary,
};
pub(crate) use store::RadarLedger;
