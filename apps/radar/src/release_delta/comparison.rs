//! Release comparison payloads and tracked-signal matching.

use crate::{
	prelude::Result,
	release_delta::{
		self, BTreeSet, GitHubApi, HashSet, Path, RadarRefreshReleaseDeltaRequest, ReleasePair,
		Value, extract_pr_number_from_url, eyre, required_value_i64, required_value_string,
		serde_json,
	},
};

pub(super) fn build_release_comparison(
	api: &GitHubApi,
	request: &RadarRefreshReleaseDeltaRequest,
	pair: &ReleasePair,
	signals: &[Value],
) -> Result<Value> {
	let stable_tag = release_delta::required_release_tag(&pair.stable)?;
	let preview_tag = release_delta::required_release_tag(&pair.preview)?;
	let compare = api
		.get(&format!(
			"https://api.github.com/repos/{}/compare/{stable_tag}...{preview_tag}",
			request.repo
		))?
		.payload;
	let commits = compare
		.get("commits")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Expected compare.commits from GitHub API"))?;
	let commit_shas = commits
		.iter()
		.filter_map(|commit| commit.get("sha").and_then(Value::as_str).map(str::to_owned))
		.collect::<Vec<_>>();
	let pr_numbers = compare_pr_numbers(commits);
	let tracked_signal_slugs = tracked_signal_slugs(signals, &commit_shas, &pr_numbers);

	Ok(serde_json::json!({
		"stable_tag_name": stable_tag,
		"prerelease_tag_name": preview_tag,
		"compare": {
			"status": required_value_string(&compare, "status")?,
			"ahead_by": required_value_i64(&compare, "ahead_by")?,
			"total_commits": required_value_i64(&compare, "total_commits")?,
			"url": required_value_string(&compare, "html_url")?,
			"commit_shas": commit_shas,
			"pr_numbers": pr_numbers,
		},
		"tracked_signal_slugs": tracked_signal_slugs,
	}))
}

pub(super) fn load_signal_entries(signals_dir: &Path, repo: &str) -> Result<Vec<Value>> {
	let mut entries = Vec::new();

	for path in release_delta::sorted_json_files(signals_dir)? {
		let payload = release_delta::load_json(&path)?;

		release_delta::validate_signal_file(&path, &payload)?;

		if payload.pointer("/source_refs/repo").and_then(Value::as_str) == Some(repo) {
			entries.push(payload);
		}
	}

	Ok(entries)
}

fn tracked_signal_slugs(
	signals: &[Value],
	commit_shas: &[String],
	pr_numbers: &[u64],
) -> Vec<String> {
	let commit_set = commit_shas.iter().map(String::as_str).collect::<HashSet<_>>();
	let pr_set = pr_numbers.iter().copied().collect::<HashSet<_>>();
	let mut sorted_signals = signals.iter().collect::<Vec<_>>();

	sorted_signals.sort_by(|left, right| {
		right
			.get("published_at")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.cmp(left.get("published_at").and_then(Value::as_str).unwrap_or_default())
	});

	sorted_signals
		.into_iter()
		.filter(|signal| {
			let signal_shas = signal_commit_shas(signal);
			let signal_pr = signal_pr_number(signal);

			signal_shas.iter().any(|sha| commit_set.contains(sha.as_str()))
				|| signal_pr.is_some_and(|number| pr_set.contains(&number))
		})
		.filter_map(|signal| signal.get("slug").and_then(Value::as_str).map(str::to_owned))
		.collect()
}

fn signal_commit_shas(signal: &Value) -> Vec<String> {
	release_delta::string_array(signal.pointer("/source_refs/commit_urls"))
		.into_iter()
		.filter_map(|url| release_delta::extract_commit_sha_from_url(&url))
		.collect()
}

fn signal_pr_number(signal: &Value) -> Option<u64> {
	signal
		.pointer("/source_refs/pr_url")
		.and_then(Value::as_str)
		.and_then(extract_pr_number_from_url)
}

fn compare_pr_numbers(commits: &[Value]) -> Vec<u64> {
	let mut numbers = commits
		.iter()
		.flat_map(|commit| {
			commit
				.pointer("/commit/message")
				.and_then(Value::as_str)
				.map(pr_numbers_from_message)
				.unwrap_or_default()
		})
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();

	numbers.sort();

	numbers
}

fn pr_numbers_from_message(message: &str) -> Vec<u64> {
	let mut numbers = Vec::new();
	let mut rest = message;

	while let Some(start) = rest.find("(#") {
		let candidate = &rest[start + 2..];
		let Some(end) = candidate.find(')') else {
			break;
		};
		let digits = &candidate[..end];

		if !digits.is_empty()
			&& digits.chars().all(|ch| ch.is_ascii_digit())
			&& let Ok(number) = digits.parse::<u64>()
		{
			numbers.push(number);
		}

		rest = &candidate[end + 1..];
	}

	numbers
}
