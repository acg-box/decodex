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
	crate::percent_encode(&repo.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
	use crate::release_delta::backfill::paths;

	#[test]
	fn backfill_repository_stems_do_not_collapse_distinct_names() {
		for (first, second) in
			[("a-b/c", "a/b-c"), ("a/b.c", "a/b-c"), ("a/b_c", "a/b-c"), ("a/b", "a%2Fb")]
		{
			assert_ne!(
				paths::repo_path_stem(first),
				paths::repo_path_stem(second),
				"{first} and {second}"
			);
		}

		assert_eq!(paths::repo_path_stem("OpenAI/Codex"), paths::repo_path_stem("openai/codex"));

		for repo in ["a-b/c", "a/b.c", "a/b_c", "a%2Fb", "../example"] {
			let name = format!("{}-pr-42.json", paths::repo_path_stem(repo));

			assert_eq!(std::path::Path::new(&name).components().count(), 1);
		}
	}
}
