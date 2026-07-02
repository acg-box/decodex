use crate::{RadarBackfillReleaseRangeRequest, release_delta::backfill::model::BackfillPaths};

pub(in crate::release_delta::backfill) fn signal_backfill_paths(
	repo: &str,
	pr_number: u64,
	request: &RadarBackfillReleaseRangeRequest,
) -> BackfillPaths {
	let stem = format!("{}-pr-{pr_number}", repo_path_stem(repo));

	BackfillPaths {
		bundle: request.bundles_dir.join(format!("{stem}.json")),
		analysis: request.analysis_dir.join(format!("{stem}.analysis.json")),
		signal: request.signals_dir.join(format!("{stem}.json")),
	}
}

fn repo_path_stem(repo: &str) -> String {
	repo.chars()
		.map(
			|character| {
				if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '-' }
			},
		)
		.collect::<String>()
		.trim_matches('-')
		.to_owned()
}
