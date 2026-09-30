//! Radar subject extraction from artifact references.

use crate::ledger::{Map, Value};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct RadarSubject {
	pub(super) repo: String,
	pub(super) subject_kind: String,
	pub(super) subject_id: String,
}
pub(super) fn subject_refs_for_signal(signal: &Map<String, Value>) -> Vec<RadarSubject> {
	let Some(refs) = signal.get("source_refs").and_then(Value::as_object) else {
		return Vec::new();
	};
	let Some(repo) = refs.get("repo").and_then(Value::as_str) else {
		return Vec::new();
	};
	let mut subjects = Vec::new();

	if let Some(pr_url) = refs.get("pr_url").and_then(Value::as_str)
		&& let Some(subject_id) = parse_pr_url_subject(pr_url, repo)
	{
		subjects.push(RadarSubject { repo: repo.into(), subject_kind: "pr".into(), subject_id });
	}
	if let Some(commit_urls) = refs.get("commit_urls").and_then(Value::as_array) {
		for url in commit_urls.iter().filter_map(Value::as_str) {
			if let Some(subject_id) = parse_commit_url_subject(url, repo) {
				subjects.push(RadarSubject {
					repo: repo.into(),
					subject_kind: "commit".into(),
					subject_id,
				});
			}
		}
	}

	subjects
}

fn parse_pr_url_subject(url: &str, repo: &str) -> Option<String> {
	let prefix = format!("https://github.com/{repo}/pull/");
	let number = url.trim_end_matches('/').strip_prefix(&prefix)?;

	if !number.is_empty() && number.chars().all(|character| character.is_ascii_digit()) {
		Some(number.into())
	} else {
		None
	}
}

fn parse_commit_url_subject(url: &str, repo: &str) -> Option<String> {
	let prefix = format!("https://github.com/{repo}/commit/");
	let sha = url.trim_end_matches('/').strip_prefix(&prefix)?;

	if (7..=40).contains(&sha.len()) && sha.chars().all(|character| character.is_ascii_hexdigit()) {
		Some(sha.into())
	} else {
		None
	}
}

#[cfg(test)]
mod tests {
	use crate::ledger::subjects::{self, RadarSubject};

	#[test]
	fn signal_subjects_belong_to_the_declared_github_repository() {
		let signal = serde_json::json!({
			"source_refs": {
				"repo": "openai/codex",
				"pr_url": "https://github.com/other/project/pull/22414",
				"commit_urls": [
					"https://github.com/other/project/commit/abcdef1",
					"https://example.com/openai/codex/commit/abcdef2",
					"https://github.com/openai/codex/commit/abcdef3/"
				]
			}
		});

		assert_eq!(
			subjects::subject_refs_for_signal(signal.as_object().unwrap()),
			vec![RadarSubject {
				repo: "openai/codex".into(),
				subject_kind: "commit".into(),
				subject_id: "abcdef3".into(),
			}]
		);

		for (url, count) in [
			("https://github.com/openai/codex/pull/22414/", 1),
			("https://github.com/openai/codex/pull/", 0),
			("https://example.com/openai/codex/pull/22414", 0),
			("https://github.com/openai/codex/pull/22414?view=1", 0),
		] {
			let signal =
				serde_json::json!({"source_refs": {"repo": "openai/codex", "pr_url": url}});

			assert_eq!(
				subjects::subject_refs_for_signal(signal.as_object().unwrap()).len(),
				count,
				"{url}"
			);
		}
	}
}
