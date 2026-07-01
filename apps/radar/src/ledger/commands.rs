//! Public Radar ledger command entrypoints.

#[allow(clippy::wildcard_imports)] use super::*;

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
