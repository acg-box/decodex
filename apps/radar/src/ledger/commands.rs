//! Public Radar ledger command entrypoints.

use crate::{
	ledger::{
		self, ArtifactLinkInput, BTreeMap, DEFAULT_LEDGER_PATH, PathBuf,
		RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
	},
	prelude::Result,
};

/// Return the default local Radar ledger path.
pub(crate) fn default_ledger_path() -> PathBuf {
	PathBuf::from(DEFAULT_LEDGER_PATH)
}

/// Initialize the local Radar ledger schema.
pub(crate) fn ledger_bootstrap(request: &RadarLedgerBootstrapRequest) -> Result<PathBuf> {
	let connection = ledger::open_ledger(&request.db_path)?;

	connection.close().map_err(|(_, error)| error)?;

	Ok(request.db_path.clone())
}

/// Ingest one bundle and optional derived artifacts into the local Radar ledger.
pub(crate) fn ledger_ingest(request: &RadarLedgerIngestRequest) -> Result<BTreeMap<String, i64>> {
	let connection = ledger::open_ledger(&request.db_path)?;

	ledger::ingest_artifact_set(
		&connection,
		&request.bundle_path,
		request.analysis_path.as_deref(),
		request.signal_path.as_deref(),
	)?;

	ledger::summary_counts(&connection)
}

/// Ingest existing checked-in Radar artifacts into the local Radar ledger.
pub(crate) fn ledger_ingest_existing(
	request: &RadarLedgerIngestExistingRequest,
) -> Result<BTreeMap<String, i64>> {
	let connection = ledger::open_ledger(&request.db_path)?;
	let mut ingested = 0_i64;

	for bundle_path in ledger::json_files_in_directory(&request.bundles_dir)? {
		let stem = ledger::file_stem(&bundle_path)?;
		let candidate_analysis = request.analysis_dir.join(format!("{stem}.analysis.json"));
		let candidate_signal = request.signals_dir.join(format!("{stem}.json"));

		ledger::ingest_artifact_set(
			&connection,
			&bundle_path,
			ledger::existing_path(&candidate_analysis),
			ledger::existing_path(&candidate_signal),
		)?;

		ingested += 1;
	}

	let linked_signal_paths =
		ledger::linked_signal_paths(&request.bundles_dir, &request.signals_dir)?;

	for signal_path in ledger::json_files_in_directory(&request.signals_dir)? {
		if linked_signal_paths.contains(&signal_path) {
			continue;
		}

		ledger::record_signal_artifact(&connection, &signal_path)?;
	}

	let mut summary = ledger::summary_counts(&connection)?;

	summary.insert("bundles_ingested".into(), ingested);

	Ok(summary)
}

/// Link one artifact path to a Radar subject in the local ledger.
pub(crate) fn ledger_artifact_link(
	request: &RadarLedgerArtifactLinkRequest,
) -> Result<BTreeMap<String, i64>> {
	let connection = ledger::open_ledger(&request.db_path)?;

	ledger::record_artifact(
		&connection,
		ArtifactLinkInput {
			repo: &request.repo,
			subject_kind: &request.subject_kind,
			subject_id: &request.subject_id,
			artifact_kind: &request.artifact_kind,
			path: &request.path,
		},
	)?;

	ledger::summary_counts(&connection)
}

/// Read local Radar ledger summary counts.
pub(crate) fn ledger_summary(request: &RadarLedgerSummaryRequest) -> Result<BTreeMap<String, i64>> {
	let connection = ledger::open_ledger(&request.db_path)?;

	ledger::summary_counts(&connection)
}
