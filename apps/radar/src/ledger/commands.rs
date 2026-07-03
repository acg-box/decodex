//! Public Radar ledger command entrypoints.

use std::{collections::BTreeMap, path::PathBuf};

use crate::ledger::{
	files::{self},
	ingest::{self},
	records::{self, ArtifactLinkInput},
	schema, stats,
};
use crate::{
	DEFAULT_LEDGER_PATH,
	prelude::Result,
	requests::{
		RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
	},
};

/// Return the default local Radar ledger path.
pub(crate) fn default_ledger_path() -> PathBuf {
	PathBuf::from(DEFAULT_LEDGER_PATH)
}

/// Initialize the local Radar ledger schema.
pub(crate) fn ledger_bootstrap(request: &RadarLedgerBootstrapRequest) -> Result<PathBuf> {
	let connection = schema::open_ledger(&request.db_path)?;

	connection.close().map_err(|(_, error)| error)?;

	Ok(request.db_path.clone())
}

/// Ingest one bundle and optional derived artifacts into the local Radar ledger.
pub(crate) fn ledger_ingest(request: &RadarLedgerIngestRequest) -> Result<BTreeMap<String, i64>> {
	let connection = schema::open_ledger(&request.db_path)?;

	ingest::ingest_artifact_set(
		&connection,
		&request.bundle_path,
		request.analysis_path.as_deref(),
		request.signal_path.as_deref(),
	)?;

	stats::summary_counts(&connection)
}

/// Ingest existing checked-in Radar artifacts into the local Radar ledger.
pub(crate) fn ledger_ingest_existing(
	request: &RadarLedgerIngestExistingRequest,
) -> Result<BTreeMap<String, i64>> {
	let connection = schema::open_ledger(&request.db_path)?;
	let mut ingested = 0_i64;

	for bundle_path in files::json_files_in_directory(&request.bundles_dir)? {
		let stem = files::file_stem(&bundle_path)?;
		let candidate_analysis = request.analysis_dir.join(format!("{stem}.analysis.json"));
		let candidate_signal = request.signals_dir.join(format!("{stem}.json"));

		ingest::ingest_artifact_set(
			&connection,
			&bundle_path,
			files::existing_path(&candidate_analysis),
			files::existing_path(&candidate_signal),
		)?;

		ingested += 1;
	}

	let linked_signal_paths =
		files::linked_signal_paths(&request.bundles_dir, &request.signals_dir)?;

	for signal_path in files::json_files_in_directory(&request.signals_dir)? {
		if linked_signal_paths.contains(&signal_path) {
			continue;
		}

		ingest::record_signal_artifact(&connection, &signal_path)?;
	}

	let mut summary = stats::summary_counts(&connection)?;

	summary.insert("bundles_ingested".into(), ingested);

	Ok(summary)
}

/// Link one artifact path to a Radar subject in the local ledger.
pub(crate) fn ledger_artifact_link(
	request: &RadarLedgerArtifactLinkRequest,
) -> Result<BTreeMap<String, i64>> {
	let connection = schema::open_ledger(&request.db_path)?;

	records::record_artifact(
		&connection,
		ArtifactLinkInput {
			repo: &request.repo,
			subject_kind: &request.subject_kind,
			subject_id: &request.subject_id,
			artifact_kind: &request.artifact_kind,
			path: &request.path,
		},
	)?;

	stats::summary_counts(&connection)
}

/// Read local Radar ledger summary counts.
pub(crate) fn ledger_summary(request: &RadarLedgerSummaryRequest) -> Result<BTreeMap<String, i64>> {
	let connection = schema::open_ledger(&request.db_path)?;

	stats::summary_counts(&connection)
}
