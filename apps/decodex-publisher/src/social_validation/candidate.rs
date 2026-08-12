//! `decodex/content-evidence/1` validation.

use std::collections::BTreeSet;

use crate::social_validation::{self, Map, SOCIAL_POST_MODES, SOCIAL_POST_PRIORITIES, Value};

const SOURCE_KINDS: &[&str] = &["landed_decodex", "official_codex", "radar_secondary"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceKind {
	LandedDecodex,
	OfficialCodex,
	RadarSecondary,
}

impl SourceKind {
	const fn label(self) -> &'static str {
		match self {
			Self::LandedDecodex => "landed_decodex",
			Self::OfficialCodex => "official_codex",
			Self::RadarSecondary => "radar_secondary",
		}
	}

	const fn is_primary(self) -> bool {
		matches!(self, Self::LandedDecodex | Self::OfficialCodex)
	}
}

struct ExactHttpsUrl<'a> {
	host: &'a str,
	path: &'a str,
}

pub(super) fn validate_social_candidate(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	social_validation::validate_exact_keys(
		entry,
		"content_evidence",
		&[
			"audience",
			"candidate_text",
			"caveats",
			"channel",
			"claims",
			"decision",
			"evidence_notes",
			"mode",
			"priority",
			"repo",
			"schema",
			"slug",
			"source_kinds",
			"source_refs",
			"target_account",
		],
		errors,
	);

	for field in ["slug", "repo", "audience"] {
		if !social_validation::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}
	if social_validation::string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if social_validation::string_field(entry, "channel") != Some("x") {
		errors.push("channel must be x".into());
	}
	if social_validation::string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if !social_validation::matches_one_of(entry.get("mode"), SOCIAL_POST_MODES) {
		errors
			.push(format!("mode must be one of {}", social_validation::choices(SOCIAL_POST_MODES)));
	}
	if !social_validation::matches_one_of(entry.get("priority"), SOCIAL_POST_PRIORITIES) {
		errors.push(format!(
			"priority must be one of {}",
			social_validation::choices(SOCIAL_POST_PRIORITIES)
		));
	}

	validate_candidate_text(entry.get("candidate_text"), errors);
	validate_sources(entry.get("source_refs"), entry.get("source_kinds"), errors);
	social_validation::validate_non_empty_string_list(
		entry.get("evidence_notes"),
		"evidence_notes",
		errors,
	);
	social_validation::validate_social_post_claims(
		entry.get("claims"),
		entry.get("source_refs"),
		None,
		false,
		errors,
	);
	validate_decision(entry.get("decision"), errors);
	social_validation::validate_optional_string_list(entry.get("caveats"), "caveats", errors);
}

fn validate_candidate_text(value: Option<&Value>, errors: &mut Vec<String>) {
	social_validation::validate_social_post_text(value, errors);
	let Some(items) = value.and_then(Value::as_array) else {
		return;
	};
	if items.len() != 1 {
		errors.push("candidate_text must contain exactly one item".into());
	}
	if items.first().and_then(Value::as_str).is_none_or(|text| text.chars().count() < 80) {
		errors.push("candidate_text item must contain at least 80 Unicode characters".into());
	}
}

fn validate_sources(refs: Option<&Value>, kinds: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());
		return;
	};
	social_validation::validate_exact_keys(refs, "source_refs", &["urls"], errors);
	let Some(urls) = refs.get("urls").and_then(Value::as_array).filter(|urls| !urls.is_empty())
	else {
		errors.push("source_refs.urls must be a non-empty list of https URLs".into());
		return;
	};
	if urls.len() > 8 || !urls.iter().all(|url| social_validation::is_https_string(Some(url))) {
		errors.push("source_refs.urls must contain at most 8 canonical HTTPS URLs".into());
	}
	let url_values = urls.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>();
	if url_values.len() != urls.len() {
		errors.push("source_refs.urls must be unique".into());
	}

	let Some(kinds) = kinds.and_then(Value::as_object) else {
		errors.push("source_kinds must map every source URL to its evidence class".into());
		return;
	};
	if kinds.keys().map(String::as_str).collect::<BTreeSet<_>>() != url_values {
		errors.push("source_kinds keys must exactly match source_refs.urls".into());
	}
	for (url, kind) in kinds {
		if !SOURCE_KINDS.contains(&kind.as_str().unwrap_or_default()) {
			errors.push(format!(
				"source_kinds[{url:?}] must be one of {}",
				social_validation::choices(SOURCE_KINDS)
			));
		}
	}

	let mut has_primary = false;
	for url in url_values {
		let Some(classified) = classify_source_url(url) else {
			errors.push(format!(
				"source_refs.urls entry {url:?} must be a canonical HTTPS URL without userinfo, a port, query, fragment, percent escape, or non-normal path"
			));
			continue;
		};
		let Some(declared) = kinds.get(url).and_then(Value::as_str) else {
			continue;
		};
		if !SOURCE_KINDS.contains(&declared) {
			continue;
		}
		if declared != classified.label() {
			errors.push(format!(
				"source_kinds[{url:?}] must be {:?} for that URL",
				classified.label()
			));
			continue;
		}
		if classified.is_primary() {
			has_primary = true;
		}
	}
	if !has_primary {
		errors.push("at least one official_codex or landed_decodex source is required".into());
	}
}

fn classify_source_url(value: &str) -> Option<SourceKind> {
	let url = parse_exact_https_url(value)?;
	if url.host == "github.com" && path_at_or_below(url.path, "openai/codex") {
		return Some(SourceKind::OfficialCodex);
	}
	if url.host == "developers.openai.com" && path_at_or_below(url.path, "codex") {
		return Some(SourceKind::OfficialCodex);
	}
	if url.host == "platform.openai.com" && path_at_or_below(url.path, "docs/codex") {
		return Some(SourceKind::OfficialCodex);
	}
	if url.host == "openai.com"
		&& (path_at_or_below(url.path, "codex") || is_openai_codex_release_path(url.path))
	{
		return Some(SourceKind::OfficialCodex);
	}
	if url.host == "github.com" && is_decodex_commit_path(url.path) {
		return Some(SourceKind::LandedDecodex);
	}

	Some(SourceKind::RadarSecondary)
}

fn parse_exact_https_url(value: &str) -> Option<ExactHttpsUrl<'_>> {
	if !value.is_ascii() {
		return None;
	}
	let remainder = value.strip_prefix("https://")?;
	if remainder.is_empty()
		|| remainder
			.bytes()
			.any(|byte| matches!(byte, b'%' | b'\\' | b'?' | b'#') || byte.is_ascii_whitespace())
	{
		return None;
	}
	let (host, path) = remainder.split_once('/').unwrap_or((remainder, ""));
	if !valid_dns_host(host) || !valid_url_path(path) {
		return None;
	}

	Some(ExactHttpsUrl { host, path })
}

fn valid_dns_host(host: &str) -> bool {
	host.len() <= 253
		&& host.contains('.')
		&& host.bytes().any(|byte| byte.is_ascii_lowercase())
		&& host.split('.').all(|label| {
			!label.is_empty()
				&& label.len() <= 63
				&& label
					.bytes()
					.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
				&& label
					.bytes()
					.next()
					.is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
				&& label
					.bytes()
					.next_back()
					.is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
		})
}

fn valid_url_path(path: &str) -> bool {
	if path.is_empty() {
		return true;
	}
	let path = path.strip_suffix('/').unwrap_or(path);
	!path.is_empty()
		&& path.split('/').all(|segment| {
			!segment.is_empty()
				&& !matches!(segment, "." | "..")
				&& segment.bytes().all(|byte| {
					byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
				})
		})
}

fn path_at_or_below(path: &str, root: &str) -> bool {
	path == root || path.strip_prefix(root).is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_openai_codex_release_path(path: &str) -> bool {
	let path = path.strip_suffix('/').unwrap_or(path);
	let Some(slug) = path.strip_prefix("index/") else {
		return false;
	};
	!slug.is_empty()
		&& !slug.contains('/')
		&& slug.contains("codex")
		&& slug
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_decodex_commit_path(path: &str) -> bool {
	let Some(oid) = path.strip_prefix("acg-box/decodex/commit/") else {
		return false;
	};
	oid.len() == 40 && oid.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn validate_decision(value: Option<&Value>, errors: &mut Vec<String>) {
	let Some(decision) = value.and_then(Value::as_object) else {
		errors.push("decision must be an object".into());
		return;
	};
	social_validation::validate_exact_keys(
		decision,
		"decision",
		&["idempotency_key", "reason", "worthiness"],
		errors,
	);
	if !social_validation::matches_one_of(decision.get("worthiness"), &["no_op", "publish"]) {
		errors.push("decision.worthiness must be one of ['no_op', 'publish']".into());
	}
	if !social_validation::is_non_empty_string(decision.get("reason")) {
		errors.push("decision.reason must be a non-empty string".into());
	}
	if !decision.get("idempotency_key").and_then(Value::as_str).is_some_and(|value| {
		value.len() == 84
			&& value.starts_with("content-publication:")
			&& value[20..].bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
	}) {
		errors.push("decision.idempotency_key must be content-publication:<sha256>".into());
	}
}

#[cfg(test)]
mod tests {
	use serde_json::{Value, json};

	use super::{SourceKind, classify_source_url, validate_sources};

	const DECODEX_COMMIT_URL: &str =
		"https://github.com/acg-box/decodex/commit/0123456789abcdef0123456789abcdef01234567";
	const OFFICIAL_CODEX_URL: &str = "https://github.com/openai/codex/pull/22414";

	#[test]
	fn accepts_each_supported_primary_source_form() {
		for (url, kind) in [
			(OFFICIAL_CODEX_URL, "official_codex"),
			("https://developers.openai.com/codex/config-reference", "official_codex"),
			("https://platform.openai.com/docs/codex/overview", "official_codex"),
			("https://openai.com/codex/", "official_codex"),
			("https://openai.com/index/introducing-codex/", "official_codex"),
			(DECODEX_COMMIT_URL, "landed_decodex"),
		] {
			let errors = source_errors(url, kind);
			assert!(errors.is_empty(), "{url}: {errors:?}");
		}
	}

	#[test]
	fn rejects_malformed_https_urls() {
		for url in [
			"http://github.com/openai/codex",
			"https://github.com@evil.example/openai/codex",
			"https://evil.example@github.com/openai/codex",
			"https://github.com:443/openai/codex",
			"https://github%2ecom/openai/codex",
			"https://github..com/openai/codex",
			"https://github.com./openai/codex",
			"https://github.com/openai/codex?tab=readme",
			"https://github.com/openai/codex#readme",
			"https://github.com/openai%2fcodex",
			"https://github.com/openai/%ZZcodex",
			"https://github.com/openai/codex/../other",
			"https://github.com/openai//codex",
			"https://github.com\\openai/codex",
		] {
			assert_eq!(classify_source_url(url), None, "{url}");
		}
	}

	#[test]
	fn caller_labels_cannot_promote_unrelated_or_deceptive_sources() {
		for (url, kind) in [
			("https://example.com/codex", "official_codex"),
			("https://github.com.evil.example/openai/codex", "official_codex"),
			("https://github.com/openai/codex.evil", "official_codex"),
			("https://github.com/openai/not-codex", "official_codex"),
			("https://github.com/other/codex", "official_codex"),
			(
				"https://github.com/acg-box/other/commit/0123456789abcdef0123456789abcdef01234567",
				"landed_decodex",
			),
			(
				"https://github.com/hack-ink/decodex/commit/0123456789abcdef0123456789abcdef01234567",
				"landed_decodex",
			),
			("https://github.com/acg-box/decodex/pull/1", "landed_decodex"),
			("https://github.com/acg-box/decodex/commit/0123456", "landed_decodex"),
			(
				"https://github.com/acg-box/decodex/commit/0123456789ABCDEF0123456789ABCDEF01234567",
				"landed_decodex",
			),
			(
				"https://github.com/acg-box/decodex/commit/0123456789abcdef0123456789abcdef01234567/extra",
				"landed_decodex",
			),
			(
				"https://github.com/acg-box/decodex/commit/0123456789abcdef0123456789abcdef01234567/",
				"landed_decodex",
			),
		] {
			let errors = source_errors(url, kind);
			assert!(
				errors.iter().any(|error| error.contains("must be \"radar_secondary\"")),
				"{url}: {errors:?}"
			);
			assert!(errors.iter().any(|error| error.contains("at least one official_codex")));
		}
	}

	#[test]
	fn labels_must_match_recognized_primary_urls() {
		for (url, kind, expected) in [
			(OFFICIAL_CODEX_URL, "radar_secondary", SourceKind::OfficialCodex),
			(DECODEX_COMMIT_URL, "official_codex", SourceKind::LandedDecodex),
		] {
			assert_eq!(classify_source_url(url), Some(expected));
			let errors = source_errors(url, kind);
			assert!(errors.iter().any(|error| error.contains("for that URL")));
			assert!(errors.iter().any(|error| error.contains("at least one official_codex")));
		}
	}

	#[test]
	fn radar_secondary_cannot_satisfy_the_primary_requirement() {
		let errors = source_errors("https://codexradar.example/reports/22414", "radar_secondary");
		assert_eq!(errors, ["at least one official_codex or landed_decodex source is required"]);
	}

	#[test]
	fn candidate_claims_stay_bound_to_declared_sources() {
		let mut candidate = candidate_with_source(OFFICIAL_CODEX_URL, "official_codex");
		let validation = crate::social_validation::validate_social_artifact(&candidate);
		assert!(validation.errors.is_empty(), "{:?}", validation.errors);

		candidate["claims"][0]["evidence"] = json!("https://example.com/not-declared");
		let validation = crate::social_validation::validate_social_artifact(&candidate);
		assert!(validation.errors.iter().any(|error| {
			error.contains("claims[0].evidence must exactly match one declared source reference")
		}));
	}

	fn source_errors(url: &str, kind: &str) -> Vec<String> {
		let refs = json!({"urls": [url]});
		let kinds = json!({(url): kind});
		let mut errors = Vec::new();
		validate_sources(Some(&refs), Some(&kinds), &mut errors);
		errors
	}

	fn candidate_with_source(url: &str, kind: &str) -> Value {
		json!({
			"schema": crate::SOCIAL_CANDIDATE_SCHEMA,
			"slug": "source-classification",
			"repo": "openai/codex",
			"channel": "x",
			"target_account": "decodexspace",
			"mode": "operator_impact",
			"priority": "normal",
			"audience": "Codex operators",
			"candidate_text": ["Codex operators can now verify this concrete source-backed change before they rely on the documented workflow."],
			"source_refs": {"urls": [url]},
			"source_kinds": {(url): kind},
			"evidence_notes": ["Direct primary source."],
			"claims": [{
				"text": "Codex documents this change.",
				"evidence": url,
				"confidence": "confirmed"
			}],
			"decision": {
				"worthiness": "publish",
				"reason": "Concrete operator consequence.",
				"idempotency_key": format!("content-publication:{}", "a".repeat(64))
			}
		})
	}
}
