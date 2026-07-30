//! social_candidate/v1 schema validation.

use std::{
	collections::BTreeSet,
	path::{Component, Path},
};

use crate::social_validation::{self, Map, SOCIAL_POST_MODES, SOCIAL_POST_PRIORITIES, Value};

const CONNECTIVE_TEXT: &[&str] = &[" "];
const RADAR_PAIR_PREFIX: &str = ".agent/automations/radar/cache/github/content-review-pairs";
const RADAR_ELIGIBILITY_FIELDS: &[&str] = &[
	"commit_shas",
	"impact_sha256",
	"lineage_sha256",
	"queue_sha256",
	"repo",
	"review_sha256",
	"schema",
	"slug",
	"subject_id",
	"subject_kind",
	"upstream_head",
];

pub(super) fn validate_social_candidate(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	social_validation::validate_exact_keys(
		entry,
		"social_candidate",
		&[
			"audience",
			"candidate_text",
			"caveats",
			"channel",
			"claims",
			"decision",
			"evidence_digests",
			"evidence_notes",
			"mode",
			"next_steps",
			"priority",
			"radar_eligibility",
			"radar_source_refs",
			"repo",
			"schema",
			"slug",
			"source_refs",
			"target_account",
			"text_segments",
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

	social_validation::validate_social_post_text(entry.get("candidate_text"), errors);

	validate_social_candidate_source_refs(entry.get("source_refs"), errors);

	social_validation::validate_non_empty_string_list(
		entry.get("evidence_notes"),
		"evidence_notes",
		errors,
	);
	social_validation::validate_social_post_claims(
		entry.get("claims"),
		entry.get("source_refs"),
		entry.get("evidence_digests"),
		false,
		errors,
	);

	validate_social_candidate_decision(entry.get("decision"), errors);
	let publish = entry
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| social_validation::string_field(decision, "worthiness"))
		== Some("publish");
	if publish {
		if entry.get("candidate_text").and_then(Value::as_array).map(Vec::len) != Some(1) {
			errors.push("publish candidate_text must contain exactly one item".into());
		}
		if entry
			.get("candidate_text")
			.and_then(Value::as_array)
			.and_then(|items| items.first())
			.and_then(Value::as_str)
			.is_none_or(|text| text.chars().count() < 80)
		{
			errors.push(
				"publish candidate_text item must contain at least 80 Unicode characters".into(),
			);
		}
	}
	validate_radar_eligibility_contract(entry, publish, errors);
	validate_text_segments(entry, publish, errors);

	for field in ["caveats", "next_steps"] {
		social_validation::validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_social_candidate_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(
		refs,
		"source_refs",
		&["release_deltas", "signals", "upstream_impacts", "upstream_reviews", "urls"],
		errors,
	);
	let has_refs = ["upstream_reviews", "upstream_impacts", "signals", "release_deltas", "urls"]
		.iter()
		.any(|field| {
			refs.get(*field)
				.is_some_and(|value| !social_validation::is_empty_or_missing_array(Some(value)))
		});

	if !has_refs {
		errors.push(
			"source_refs must include upstream_reviews, upstream_impacts, signals, release_deltas, or urls"
				.into(),
		);
	}

	let uses_radar_inputs = ["upstream_reviews", "release_deltas"]
		.iter()
		.any(|field| social_validation::non_empty_array(refs.get(*field)).is_some());

	if uses_radar_inputs
		&& social_validation::non_empty_array(refs.get("upstream_impacts")).is_none()
	{
		errors.push(
			"source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff for Radar-derived social candidates"
				.into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !social_validation::is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in ["upstream_reviews", "upstream_impacts", "signals", "release_deltas"] {
		social_validation::validate_optional_string_list(
			refs.get(field),
			&format!("source_refs.{field}"),
			errors,
		);
	}
}

fn validate_radar_eligibility_contract(
	entry: &Map<String, Value>,
	publish: bool,
	errors: &mut Vec<String>,
) {
	let eligibility = entry.get("radar_eligibility");
	let radar_source_refs = entry.get("radar_source_refs");
	if publish && eligibility.is_none() {
		errors.push("publish candidate requires radar_eligibility".into());
	}
	if publish && radar_source_refs.is_none() {
		errors.push("publish candidate requires radar_source_refs".into());
	}
	let Some(eligibility) = eligibility else {
		if radar_source_refs.is_some() {
			errors.push("radar_source_refs requires radar_eligibility".into());
		}

		return;
	};
	let Some(eligibility) = eligibility.as_object() else {
		errors.push("radar_eligibility must be an object".into());

		return;
	};
	validate_radar_eligibility_fields(entry, eligibility, errors);

	let Some(radar_refs) = radar_source_refs.and_then(Value::as_object) else {
		if radar_source_refs.is_some() {
			errors.push("radar_source_refs must be an object".into());
		}

		return;
	};
	validate_radar_source_contract(entry, eligibility, radar_refs, publish, errors);
}

fn validate_radar_eligibility_fields(
	entry: &Map<String, Value>,
	eligibility: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	social_validation::validate_exact_keys(
		eligibility,
		"radar_eligibility",
		RADAR_ELIGIBILITY_FIELDS,
		errors,
	);
	if social_validation::string_field(eligibility, "schema")
		!= Some("radar_content_eligibility/v1")
	{
		errors.push("radar_eligibility.schema must be radar_content_eligibility/v1".into());
	}
	for field in ["repo", "slug", "subject_id"] {
		if !social_validation::is_non_empty_string(eligibility.get(field)) {
			errors.push(format!("radar_eligibility.{field} must be a non-empty string"));
		}
	}
	if !social_validation::matches_one_of(eligibility.get("subject_kind"), &["commit", "pr"]) {
		errors.push("radar_eligibility.subject_kind must be one of ['commit', 'pr']".into());
	}
	if !eligibility.get("upstream_head").and_then(Value::as_str).is_some_and(valid_git_oid) {
		errors.push("radar_eligibility.upstream_head must be a lowercase Git object ID".into());
	}
	for field in ["queue_sha256", "review_sha256", "impact_sha256", "lineage_sha256"] {
		if !eligibility.get(field).and_then(Value::as_str).is_some_and(valid_sha256) {
			errors.push(format!("radar_eligibility.{field} must be a lowercase SHA-256 digest"));
		}
	}
	let commits = eligibility.get("commit_shas").and_then(Value::as_array);
	if commits.is_none_or(Vec::is_empty) {
		errors.push("radar_eligibility.commit_shas must be a non-empty list".into());
	} else if let Some(commits) = commits {
		let values = commits.iter().filter_map(Value::as_str).collect::<Vec<_>>();
		if values.len() != commits.len() || values.iter().any(|value| !valid_git_oid(value)) {
			errors
				.push("radar_eligibility.commit_shas must contain lowercase Git object IDs".into());
		}
		let mut normalized = values.clone();
		normalized.sort_unstable();
		normalized.dedup();
		if normalized != values {
			errors.push(
				"radar_eligibility.commit_shas must be unique and lexicographically sorted".into(),
			);
		}
	}
	for (candidate_field, eligibility_field) in [("repo", "repo"), ("slug", "slug")] {
		if entry.get(candidate_field).and_then(Value::as_str)
			!= eligibility.get(eligibility_field).and_then(Value::as_str)
		{
			errors.push(format!(
				"{candidate_field} must exactly match radar_eligibility.{eligibility_field}"
			));
		}
	}
}

fn validate_radar_source_contract(
	entry: &Map<String, Value>,
	eligibility: &Map<String, Value>,
	radar_refs: &Map<String, Value>,
	publish: bool,
	errors: &mut Vec<String>,
) {
	social_validation::validate_exact_keys(
		radar_refs,
		"radar_source_refs",
		&["impact", "queue", "review"],
		errors,
	);
	if !radar_refs.get("queue").and_then(Value::as_str).is_some_and(|value| {
		normalized_radar_path(value, ".agent/automations/radar/cache/github/review-queue")
	}) {
		errors.push("radar_source_refs.queue must be a canonical private Radar JSON path".into());
	}
	let review_pair = radar_refs
		.get("review")
		.and_then(Value::as_str)
		.and_then(|value| normalized_radar_pair_path(value, "review.json"));
	let impact_pair = radar_refs
		.get("impact")
		.and_then(Value::as_str)
		.and_then(|value| normalized_radar_pair_path(value, "impact.json"));
	if review_pair.is_none() {
		errors.push(
			"radar_source_refs.review must be a canonical private Radar pair review path".into(),
		);
	}
	if impact_pair.is_none() {
		errors.push(
			"radar_source_refs.impact must be a canonical private Radar pair impact path".into(),
		);
	}
	if review_pair.is_some() && impact_pair.is_some() && review_pair != impact_pair {
		errors.push(
			"radar_source_refs.review and impact must share one canonical Radar pair directory"
				.into(),
		);
	}
	let Some(source_refs) = entry.get("source_refs").and_then(Value::as_object) else {
		return;
	};
	for (field, radar_field) in [("upstream_reviews", "review"), ("upstream_impacts", "impact")] {
		let expected = radar_refs.get(radar_field).and_then(Value::as_str);
		let actual = source_refs.get(field).and_then(Value::as_array);
		if actual.is_none_or(|values| {
			values.len() != 1 || values.first().and_then(Value::as_str) != expected
		}) {
			errors.push(format!(
				"source_refs.{field} must contain exactly radar_source_refs.{radar_field}"
			));
		}
	}
	let Some(digests) = entry.get("evidence_digests").and_then(Value::as_object) else {
		return;
	};
	for (radar_field, digest_field) in [("review", "review_sha256"), ("impact", "impact_sha256")] {
		let reference = radar_refs.get(radar_field).and_then(Value::as_str);
		let expected = eligibility.get(digest_field).and_then(Value::as_str);
		if reference.and_then(|reference| digests.get(reference)).and_then(Value::as_str)
			!= expected
		{
			errors.push(format!(
				"evidence_digests must bind radar_source_refs.{radar_field} to \
				 radar_eligibility.{digest_field}"
			));
		}
	}

	let verified_claim_sources = ["review", "impact"]
		.into_iter()
		.filter_map(|field| radar_refs.get(field).and_then(Value::as_str))
		.collect::<BTreeSet<_>>();
	if publish && let Some(claims) = entry.get("claims").and_then(Value::as_array) {
		for (index, claim) in claims.iter().enumerate() {
			if claim
				.get("evidence")
				.and_then(Value::as_str)
				.is_none_or(|evidence| !verified_claim_sources.contains(evidence))
			{
				errors.push(format!(
					"claims[{index}].evidence must bind one verified Radar review or impact"
				));
			}
		}
	}
}

fn validate_text_segments(entry: &Map<String, Value>, publish: bool, errors: &mut Vec<String>) {
	let Some(segments) = entry.get("text_segments") else {
		if publish {
			errors.push("publish candidate requires text_segments".into());
		}

		return;
	};
	let Some(segments) = segments.as_array().filter(|segments| !segments.is_empty()) else {
		errors.push("text_segments must be a non-empty list".into());

		return;
	};
	let claims = entry.get("claims").and_then(Value::as_array).cloned().unwrap_or_default();
	let mut rendered = String::new();
	let mut expected_claim = 0_u64;
	let mut expect_claim = true;

	for (index, segment) in segments.iter().enumerate() {
		let Some(segment) = segment.as_object() else {
			errors.push(format!("text_segments[{index}] must be an object"));
			continue;
		};
		let kind = social_validation::string_field(segment, "kind");
		match kind {
			Some("claim") => {
				social_validation::validate_exact_keys(
					segment,
					&format!("text_segments[{index}]"),
					&["claim_index", "kind"],
					errors,
				);
				if !expect_claim {
					errors
						.push("text_segments must alternate claim and connective segments".into());
				}
				let claim_index = segment.get("claim_index").and_then(Value::as_u64);
				if claim_index != Some(expected_claim) {
					errors.push(
						"text_segments claim_index values must cover claims once in order".into(),
					);
				}
				if let Some(text) = claim_index
					.and_then(|claim_index| usize::try_from(claim_index).ok())
					.and_then(|claim_index| claims.get(claim_index))
					.and_then(|claim| claim.get("text"))
					.and_then(Value::as_str)
				{
					rendered.push_str(text);
				}
				expected_claim = expected_claim.saturating_add(1);
				expect_claim = false;
			},
			Some("connective") => {
				social_validation::validate_exact_keys(
					segment,
					&format!("text_segments[{index}]"),
					&["kind", "text"],
					errors,
				);
				if expect_claim {
					errors
						.push("text_segments must alternate claim and connective segments".into());
				}
				let connective = segment.get("text").and_then(Value::as_str);
				if !connective.is_some_and(|value| CONNECTIVE_TEXT.contains(&value)) {
					errors.push(format!(
						"text_segments[{index}].text must be an approved non-factual connective"
					));
				}
				if let Some(connective) = connective {
					rendered.push_str(connective);
				}
				expect_claim = true;
			},
			Some(_) | None => errors.push(format!(
				"text_segments[{index}].kind must be one of ['claim', 'connective']"
			)),
		}
	}
	if expect_claim {
		errors.push("text_segments must end with a claim segment".into());
	}
	if usize::try_from(expected_claim).ok() != Some(claims.len()) {
		errors.push("text_segments must include every claim exactly once".into());
	}
	if entry
		.get("candidate_text")
		.and_then(Value::as_array)
		.and_then(|items| items.first())
		.and_then(Value::as_str)
		!= Some(rendered.as_str())
	{
		errors.push(
			"candidate_text must exactly equal the canonical ordered claim composition".into(),
		);
	}
}

fn normalized_radar_path(value: &str, prefix: &str) -> bool {
	let path = Path::new(value);
	let normalized = !path.is_absolute()
		&& path.extension().and_then(|extension| extension.to_str()) == Some("json")
		&& path.components().all(|component| matches!(component, Component::Normal(_)));
	if !normalized {
		return false;
	}
	if path.starts_with(prefix) {
		return true;
	}
	#[cfg(test)]
	{
		let expected_collection = Path::new(prefix).file_name().and_then(|value| value.to_str());
		let parts = path.iter().filter_map(|part| part.to_str()).collect::<Vec<_>>();

		path.starts_with("target")
			&& parts.windows(2).any(|parts| {
				parts.first() == Some(&"github") && parts.get(1).copied() == expected_collection
			})
	}
	#[cfg(not(test))]
	false
}

fn normalized_radar_pair_path(value: &str, expected_file: &str) -> Option<String> {
	let path = Path::new(value);
	if path.is_absolute()
		|| path.components().any(|component| !matches!(component, Component::Normal(_)))
		|| path.file_name().and_then(|value| value.to_str()) != Some(expected_file)
	{
		return None;
	}
	let parts = path.iter().filter_map(|part| part.to_str()).collect::<Vec<_>>();
	let pair_index = if path.starts_with(RADAR_PAIR_PREFIX) {
		Path::new(RADAR_PAIR_PREFIX).components().count()
	} else {
		#[cfg(test)]
		{
			parts
				.windows(2)
				.position(|parts| parts == ["github", "content-review-pairs"])
				.map(|index| index + 2)?
		}
		#[cfg(not(test))]
		{
			return None;
		}
	};
	if parts.len() != pair_index + 2 {
		return None;
	}
	let pair = parts[pair_index];
	let (run_id, digest) = pair.rsplit_once("--")?;
	if run_id.is_empty()
		|| run_id.chars().count() > 64
		|| !run_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
		|| digest.len() != 64
		|| !digest.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
	{
		return None;
	}

	Some(parts[..=pair_index].join("/"))
}

fn valid_git_oid(value: &str) -> bool {
	matches!(value.len(), 40 | 64)
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn validate_social_candidate_decision(decision: Option<&Value>, errors: &mut Vec<String>) {
	let Some(decision) = decision.and_then(Value::as_object) else {
		errors.push("decision must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(
		decision,
		"decision",
		&["idempotency_key", "reason", "worthiness"],
		errors,
	);

	if !social_validation::matches_one_of(decision.get("worthiness"), &["publish", "skip"]) {
		errors.push("decision.worthiness must be one of ['publish', 'skip']".into());
	}

	for field in ["reason", "idempotency_key"] {
		if !social_validation::is_non_empty_string(decision.get(field)) {
			errors.push(format!("decision.{field} must be a non-empty string"));
		}
	}
}
