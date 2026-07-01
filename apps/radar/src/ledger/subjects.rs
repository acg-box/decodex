//! Radar subject extraction from artifact references.

#[allow(clippy::wildcard_imports)] use super::*;

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
		&& let Some(subject_id) = parse_pr_url_subject(pr_url)
	{
		subjects.push(RadarSubject { repo: repo.into(), subject_kind: "pr".into(), subject_id });
	}
	if let Some(commit_urls) = refs.get("commit_urls").and_then(Value::as_array) {
		for url in commit_urls.iter().filter_map(Value::as_str) {
			if let Some(subject_id) = parse_commit_url_subject(url) {
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

fn parse_pr_url_subject(url: &str) -> Option<String> {
	let (_, number) = url.trim_end_matches('/').rsplit_once("/pull/")?;

	if number.chars().all(|character| character.is_ascii_digit()) {
		Some(number.into())
	} else {
		None
	}
}

fn parse_commit_url_subject(url: &str) -> Option<String> {
	let (_, sha) = url.trim_end_matches('/').rsplit_once("/commit/")?;

	if (7..=40).contains(&sha.len()) && sha.chars().all(|character| character.is_ascii_hexdigit()) {
		Some(sha.into())
	} else {
		None
	}
}
