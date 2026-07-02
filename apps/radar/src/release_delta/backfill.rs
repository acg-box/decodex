//! Release-window signal backfill orchestration.

mod execution;
mod model;
mod paths;
mod selection;

use crate::{
	RadarBackfillReleaseRangeReport, RadarBackfillReleaseRangeRequest, RadarRenderSignalRequest,
	RadarValidateRequest, prelude::Result,
};

/// Select and optionally execute release-window signal backfills.
pub(crate) fn backfill_release_range(
	request: &RadarBackfillReleaseRangeRequest,
) -> Result<RadarBackfillReleaseRangeReport> {
	let root = crate::repo_root()?;
	let prepared_release_delta = selection::prepare_release_delta_path(request, &root)?;
	let release_delta = crate::load_json(&prepared_release_delta.path)?;
	let selection = selection::selected_release_comparison(
		&release_delta,
		request.stable_tag.as_deref(),
		request.preview_tag.as_deref(),
	)?;
	let signals_dir = crate::resolve_against(&root, &request.signals_dir);
	let published = selection::published_pr_numbers(&signals_dir)?;
	let mut target_prs = selection
		.pr_numbers
		.into_iter()
		.filter(|number| !published.contains(number))
		.collect::<Vec<_>>();

	if let Some(limit) = request.max_prs {
		target_prs.truncate(limit);
	}

	let mut report = RadarBackfillReleaseRangeReport {
		stable_tag: selection.stable_tag,
		preview_tag: selection.preview_tag,
		target_prs,
		created: 0,
		dry_run: request.dry_run,
	};

	if request.dry_run {
		return Ok(report);
	}

	for pr_number in &report.target_prs {
		let paths = paths::signal_backfill_paths(&request.repo, *pr_number, request);
		let note = format!(
			"Backfilled from release compare range {}...{}",
			report.stable_tag, report.preview_tag
		);
		let bundle_path = crate::resolve_against(&root, &paths.bundle);
		let analysis_path = crate::resolve_against(&root, &paths.analysis);
		let signal_path = crate::resolve_against(&root, &paths.signal);

		execution::run_build_bundle(request, *pr_number, &bundle_path, &note)?;
		execution::run_codex_analysis(&root, request, &bundle_path, &analysis_path)?;
		crate::render_signal(&RadarRenderSignalRequest {
			bundle: bundle_path,
			analysis: analysis_path,
			out: signal_path,
			published_at: None,
		})?;

		report.created += 1;
	}

	crate::validate(&RadarValidateRequest {
		paths: vec![crate::resolve_against(&root, &request.signals_dir)],
	})?;
	execution::run_refresh_release_delta(request, &request.release_delta, false)?;

	Ok(report)
}
