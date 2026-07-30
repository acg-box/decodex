//! Cross-file social artifact uniqueness checks.

use std::{
	collections::{BTreeMap, BTreeSet},
	path::Path,
};

use serde_json::Value;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SOCIAL_OUTCOME_SCHEMA, SOCIAL_POST_SCHEMA,
	SOCIAL_PUBLISH_RESERVATION_SCHEMA, SOCIAL_STRATEGY_SCHEMA,
};

#[derive(Debug)]
struct CandidateLineage {
	claims: Value,
	evidence_digests: Value,
	evidence_notes: Value,
	slug: String,
	mode: String,
	idempotency_key: String,
	publication_lineage_sha256: String,
	text: Vec<String>,
}

#[derive(Debug)]
struct OutcomeReference {
	display_path: String,
	observed_at: String,
	post_ref: String,
	publication_lineage_sha256: String,
	published_url: String,
	window: String,
}

#[derive(Debug)]
struct PublishedPost {
	candidate_ref: String,
	claims: Value,
	evidence_digests: Value,
	evidence_notes: Value,
	idempotency_key: String,
	mode: String,
	owner_run_id: String,
	posted_at: String,
	publication_lineage_sha256: String,
	reservation_ref: String,
	slug: String,
	text: Vec<String>,
	urls: Vec<String>,
}

#[derive(Debug)]
struct ReservationLineage {
	candidate_ref: String,
	consumed_post_ref: Option<String>,
	idempotency_key: String,
	mode: String,
	publication_lineage_sha256: String,
	run_id: String,
	slug: String,
	status: String,
}

#[derive(Debug)]
struct StrategyReference {
	display_path: String,
	evidence_refs: Vec<String>,
	requires_three_24h_outcomes: bool,
}

#[derive(Debug)]
pub(crate) struct SocialValidationState {
	active_reservation_idempotency_keys: BTreeMap<String, String>,
	candidate_radar_lineages: BTreeMap<String, String>,
	candidates: BTreeMap<String, CandidateLineage>,
	outcome_cycles: BTreeMap<String, String>,
	outcome_references: Vec<OutcomeReference>,
	published_posts: BTreeMap<String, PublishedPost>,
	reservations: BTreeMap<String, ReservationLineage>,
	strategy_cycles: BTreeMap<String, String>,
	strategy_references: Vec<StrategyReference>,
	terminal_post_idempotency_keys: BTreeMap<String, String>,
}
impl SocialValidationState {
	pub(crate) fn new() -> Self {
		Self {
			active_reservation_idempotency_keys: BTreeMap::new(),
			candidate_radar_lineages: BTreeMap::new(),
			candidates: BTreeMap::new(),
			outcome_cycles: BTreeMap::new(),
			outcome_references: Vec::new(),
			published_posts: BTreeMap::new(),
			reservations: BTreeMap::new(),
			strategy_cycles: BTreeMap::new(),
			strategy_references: Vec::new(),
			terminal_post_idempotency_keys: BTreeMap::new(),
		}
	}

	pub(crate) fn finish(self, errors: &mut Vec<String>) {
		let mut valid_24h_outcomes = BTreeMap::new();
		validate_publication_lineage(&self, errors);

		for outcome in &self.outcome_references {
			let Some(post) = self.published_posts.get(&outcome.post_ref) else {
				errors.push(format!(
					"{}: social_outcome references missing published social_post {:?}",
					outcome.display_path, outcome.post_ref
				));

				continue;
			};

			let url_matches = post.urls.contains(&outcome.published_url);
			if !url_matches {
				errors.push(format!(
					"{}: social_outcome published_url does not match referenced social_post {:?}",
					outcome.display_path, outcome.post_ref
				));
			}
			if outcome.publication_lineage_sha256 != post.publication_lineage_sha256 {
				errors.push(format!(
					"{}: social_outcome publication lineage does not match referenced social_post {:?}",
					outcome.display_path, outcome.post_ref
				));
			}
			let window_matches = validate_outcome_window(outcome, post, errors);

			if outcome.window == "24h" && url_matches && window_matches {
				valid_24h_outcomes.insert(outcome.display_path.clone(), outcome.post_ref.clone());
			}
		}

		for strategy in self.strategy_references {
			if !strategy.requires_three_24h_outcomes {
				continue;
			}
			let referenced_posts = strategy
				.evidence_refs
				.iter()
				.filter_map(|reference| valid_24h_outcomes.get(reference))
				.collect::<BTreeSet<_>>();

			if referenced_posts.len() < 3 {
				errors.push(format!(
					"{}: numerical topic_weight or format_preference change requires evidence_refs for at least three distinct valid 24h outcomes",
					strategy.display_path
				));
			}
		}
	}
}

pub(crate) fn validate_social_cross_file_constraints(
	path: &Path,
	payload: &Value,
	state: &mut SocialValidationState,
	errors: &mut Vec<String>,
) {
	let root = crate::repo_root().ok();
	let display_path = root.as_deref().map_or_else(
		|| path.to_string_lossy().replace('\\', "/"),
		|root| crate::path_arg(root, path),
	);

	match payload.get("schema").and_then(Value::as_str) {
		Some(SOCIAL_CANDIDATE_SCHEMA) => record_candidate(payload, &display_path, state, errors),
		Some(SOCIAL_OUTCOME_SCHEMA) => record_outcome(payload, &display_path, state, errors),
		Some(SOCIAL_POST_SCHEMA) => record_post(payload, &display_path, state, errors),
		Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA) =>
			record_reservation(payload, &display_path, state, errors),
		Some(SOCIAL_STRATEGY_SCHEMA) => record_strategy(payload, &display_path, state, errors),
		_ => {},
	}
}

fn record_candidate(
	payload: &Value,
	display_path: &str,
	state: &mut SocialValidationState,
	errors: &mut Vec<String>,
) {
	let Some(slug) = payload.get("slug").and_then(Value::as_str) else {
		return;
	};
	let Some(mode) = payload.get("mode").and_then(Value::as_str) else {
		return;
	};
	let Some(idempotency_key) = payload
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("idempotency_key"))
		.and_then(Value::as_str)
	else {
		return;
	};
	let text = string_array(payload.get("candidate_text"));
	if payload
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("worthiness"))
		.and_then(Value::as_str)
		== Some("publish")
		&& let Some(lineage) = payload
			.get("radar_eligibility")
			.and_then(Value::as_object)
			.and_then(|eligibility| eligibility.get("lineage_sha256"))
			.and_then(Value::as_str)
		&& let Some(existing) =
			state.candidate_radar_lineages.insert(lineage.into(), display_path.into())
	{
		errors.push(format!(
			"{display_path}: duplicate publish candidate Radar lineage also used by {existing}"
		));
	}
	state.candidates.insert(
		display_path.into(),
		CandidateLineage {
			claims: payload.get("claims").cloned().unwrap_or(Value::Null),
			evidence_digests: payload
				.get("evidence_digests")
				.cloned()
				.unwrap_or_else(|| Value::Object(serde_json::Map::new())),
			evidence_notes: payload.get("evidence_notes").cloned().unwrap_or(Value::Null),
			slug: slug.into(),
			mode: mode.into(),
			idempotency_key: idempotency_key.into(),
			publication_lineage_sha256: crate::social_record::publication_lineage_sha256(payload)
				.unwrap_or_default(),
			text,
		},
	);
}

fn record_outcome(
	payload: &Value,
	display_path: &str,
	state: &mut SocialValidationState,
	errors: &mut Vec<String>,
) {
	let Some(post_ref) = payload.get("social_post_ref").and_then(Value::as_str) else {
		return;
	};
	let Some(window) = payload.get("window").and_then(Value::as_str) else {
		return;
	};
	let key = format!("{post_ref}:{window}");

	if let Some(existing) = state.outcome_cycles.insert(key.clone(), display_path.to_owned()) {
		errors.push(format!(
			"{display_path}: duplicate social_outcome cycle {key:?} also used by {existing}"
		));
	}
	if let (Some(published_url), Some(observed_at)) = (
		payload.get("published_url").and_then(Value::as_str),
		payload.get("observed_at").and_then(Value::as_str),
	) {
		state.outcome_references.push(OutcomeReference {
			display_path: display_path.to_owned(),
			observed_at: observed_at.to_owned(),
			post_ref: post_ref.to_owned(),
			publication_lineage_sha256: payload
				.get("observation")
				.and_then(Value::as_object)
				.and_then(|observation| observation.get("publication_lineage_sha256"))
				.and_then(Value::as_str)
				.unwrap_or_default()
				.into(),
			published_url: published_url.to_owned(),
			window: window.to_owned(),
		});
	}
}

fn record_post(
	payload: &Value,
	display_path: &str,
	state: &mut SocialValidationState,
	errors: &mut Vec<String>,
) {
	let status = payload.get("status").and_then(Value::as_str);

	if !matches!(status, Some("published" | "blocked" | "skipped")) {
		return;
	}
	if status == Some("published") {
		let publication = payload.get("publication").and_then(Value::as_object);
		let urls = publication
			.and_then(|publication| publication.get("published_urls"))
			.and_then(Value::as_array)
			.map(|urls| {
				urls.iter().filter_map(Value::as_str).map(ToOwned::to_owned).collect::<Vec<_>>()
			})
			.unwrap_or_default();
		let posted_at = publication
			.and_then(|publication| publication.get("posted_at"))
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_owned();
		let source_refs = payload.get("source_refs").and_then(Value::as_object);
		let candidate_ref = single_string_ref(source_refs, "social_candidates");
		let reservation_ref = single_string_ref(source_refs, "reservations");
		let slug = payload.get("slug").and_then(Value::as_str).unwrap_or_default();
		let mode = payload.get("mode").and_then(Value::as_str).unwrap_or_default();
		let idempotency_key = payload
			.get("decision")
			.and_then(Value::as_object)
			.and_then(|decision| decision.get("idempotency_key"))
			.and_then(Value::as_str)
			.unwrap_or_default();

		state.published_posts.insert(
			display_path.to_owned(),
			PublishedPost {
				candidate_ref,
				claims: payload.get("claims").cloned().unwrap_or(Value::Null),
				evidence_digests: payload
					.get("evidence_digests")
					.cloned()
					.unwrap_or_else(|| Value::Object(serde_json::Map::new())),
				evidence_notes: payload.get("evidence_notes").cloned().unwrap_or(Value::Null),
				idempotency_key: idempotency_key.into(),
				mode: mode.into(),
				owner_run_id: payload
					.get("owner")
					.and_then(Value::as_object)
					.and_then(|owner| owner.get("run_id"))
					.and_then(Value::as_str)
					.unwrap_or_default()
					.into(),
				posted_at,
				publication_lineage_sha256: publication
					.and_then(|publication| publication.get("publication_lineage_sha256"))
					.and_then(Value::as_str)
					.unwrap_or_default()
					.into(),
				reservation_ref,
				slug: slug.into(),
				text: string_array(payload.get("text")),
				urls,
			},
		);
	}

	let Some(key) = payload
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("idempotency_key"))
		.and_then(Value::as_str)
	else {
		return;
	};

	if let Some(existing) =
		state.terminal_post_idempotency_keys.insert(key.to_owned(), display_path.to_owned())
	{
		errors.push(format!(
			"{display_path}: duplicate terminal social_post idempotency_key {key:?} also used by {existing}"
		));
	}
	if let Some(existing) = state.active_reservation_idempotency_keys.get(key) {
		errors.push(format!(
			"{display_path}: terminal social_post idempotency_key {key:?} conflicts with active reservation {existing}"
		));
	}
}

fn record_reservation(
	payload: &Value,
	display_path: &str,
	state: &mut SocialValidationState,
	errors: &mut Vec<String>,
) {
	let Some(key) = payload.get("idempotency_key").and_then(Value::as_str) else {
		return;
	};
	let status = payload.get("status").and_then(Value::as_str).unwrap_or_default();
	let candidate_ref = single_string_ref(
		payload.get("candidate_refs").and_then(Value::as_object),
		"social_candidates",
	);
	let run_id = payload
		.get("owner")
		.and_then(Value::as_object)
		.and_then(|owner| owner.get("run_id"))
		.and_then(Value::as_str)
		.unwrap_or_default();
	state.reservations.insert(
		display_path.into(),
		ReservationLineage {
			candidate_ref,
			consumed_post_ref: payload
				.get("consumed_by_social_post")
				.and_then(Value::as_str)
				.map(Into::into),
			idempotency_key: key.into(),
			mode: payload.get("mode").and_then(Value::as_str).unwrap_or_default().into(),
			publication_lineage_sha256: payload
				.get("publication_lineage_sha256")
				.and_then(Value::as_str)
				.unwrap_or_default()
				.into(),
			run_id: run_id.into(),
			slug: payload.get("slug").and_then(Value::as_str).unwrap_or_default().into(),
			status: status.into(),
		},
	);
	if status != "active" {
		return;
	}

	if let Some(existing) = state.terminal_post_idempotency_keys.get(key) {
		errors.push(format!(
			"{display_path}: active social_publish_reservation idempotency_key {key:?} conflicts with terminal social_post {existing}"
		));
	}
	if let Some(existing) =
		state.active_reservation_idempotency_keys.insert(key.to_owned(), display_path.to_owned())
	{
		errors.push(format!(
			"{display_path}: duplicate active social_publish_reservation idempotency_key {key:?} also used by {existing}"
		));
	}
}

fn validate_publication_lineage(state: &SocialValidationState, errors: &mut Vec<String>) {
	for (path, reservation) in &state.reservations {
		let Some(candidate) = state.candidates.get(&reservation.candidate_ref) else {
			errors.push(format!(
				"{path}: social_publish_reservation references missing social_candidate {:?}",
				reservation.candidate_ref
			));
			continue;
		};
		if reservation.slug != candidate.slug
			|| reservation.mode != candidate.mode
			|| reservation.idempotency_key != candidate.idempotency_key
			|| reservation.publication_lineage_sha256 != candidate.publication_lineage_sha256
		{
			errors.push(format!(
				"{path}: social_publish_reservation does not match its social_candidate"
			));
		}
		if reservation.status == "consumed" {
			let Some(post_ref) = reservation.consumed_post_ref.as_ref() else {
				continue;
			};
			let Some(post) = state.published_posts.get(post_ref) else {
				errors.push(format!(
					"{path}: consumed social_publish_reservation references missing published social_post {post_ref:?}"
				));
				continue;
			};
			if post.reservation_ref != *path {
				errors.push(format!(
					"{path}: consumed social_publish_reservation and published social_post do not reference each other"
				));
			}
		}
	}

	for (path, post) in &state.published_posts {
		let Some(reservation) = state.reservations.get(&post.reservation_ref) else {
			errors.push(format!(
				"{path}: published social_post references missing social_publish_reservation {:?}",
				post.reservation_ref
			));
			continue;
		};
		let Some(candidate) = state.candidates.get(&post.candidate_ref) else {
			errors.push(format!(
				"{path}: published social_post references missing social_candidate {:?}",
				post.candidate_ref
			));
			continue;
		};
		if reservation.status != "consumed"
			|| reservation.consumed_post_ref.as_deref() != Some(path)
			|| reservation.candidate_ref != post.candidate_ref
		{
			errors.push(format!(
				"{path}: published social_post requires one matching consumed reservation"
			));
		}
		if post.slug != candidate.slug
			|| post.mode != candidate.mode
			|| post.idempotency_key != candidate.idempotency_key
			|| post.publication_lineage_sha256 != candidate.publication_lineage_sha256
			|| post.publication_lineage_sha256 != reservation.publication_lineage_sha256
			|| post.text != candidate.text
			|| post.claims != candidate.claims
			|| post.evidence_digests != candidate.evidence_digests
			|| post.evidence_notes != candidate.evidence_notes
			|| reservation.run_id != post.owner_run_id
			|| post.owner_run_id != path_run_id(path)
			|| reservation.run_id != path_run_id(path)
		{
			errors.push(format!(
				"{path}: published social_post lineage does not match its candidate and reservation"
			));
		}
	}
}

fn single_string_ref(object: Option<&serde_json::Map<String, Value>>, field: &str) -> String {
	object
		.and_then(|object| object.get(field))
		.and_then(Value::as_array)
		.and_then(|values| values.first())
		.and_then(Value::as_str)
		.unwrap_or_default()
		.into()
}

fn string_array(value: Option<&Value>) -> Vec<String> {
	value
		.and_then(Value::as_array)
		.map(|values| values.iter().filter_map(Value::as_str).map(Into::into).collect())
		.unwrap_or_default()
}

fn path_run_id(path: &str) -> &str {
	Path::new(path).file_stem().and_then(|value| value.to_str()).unwrap_or_default()
}

fn record_strategy(
	payload: &Value,
	display_path: &str,
	state: &mut SocialValidationState,
	errors: &mut Vec<String>,
) {
	let Some(cycle_key) = payload.get("cycle_key").and_then(Value::as_str) else {
		return;
	};

	if let Some(existing) =
		state.strategy_cycles.insert(cycle_key.to_owned(), display_path.to_owned())
	{
		errors.push(format!(
			"{display_path}: duplicate social_strategy cycle_key {cycle_key:?} also used by {existing}"
		));
	}
	let evidence_refs = payload
		.get("evidence_refs")
		.and_then(Value::as_array)
		.map(|references| {
			references.iter().filter_map(Value::as_str).map(ToOwned::to_owned).collect::<Vec<_>>()
		})
		.unwrap_or_default();

	state.strategy_references.push(StrategyReference {
		display_path: display_path.to_owned(),
		evidence_refs,
		requires_three_24h_outcomes: strategy_changes_numerical_topic_or_format(payload),
	});
}

fn validate_outcome_window(
	outcome: &OutcomeReference,
	post: &PublishedPost,
	errors: &mut Vec<String>,
) -> bool {
	let (Ok(observed_at), Ok(posted_at)) = (
		OffsetDateTime::parse(&outcome.observed_at, &Rfc3339),
		OffsetDateTime::parse(&post.posted_at, &Rfc3339),
	) else {
		return false;
	};
	let elapsed = observed_at - posted_at;
	let range = match outcome.window.as_str() {
		"24h" => Duration::hours(23)..=Duration::hours(48),
		"7d" => Duration::hours(167)..=Duration::hours(192),
		_ => return false,
	};

	if !range.contains(&elapsed) {
		errors.push(format!(
			"{}: social_outcome {:?} window is outside its allowed observation interval",
			outcome.display_path, outcome.window
		));

		return false;
	}

	true
}

fn strategy_changes_numerical_topic_or_format(payload: &Value) -> bool {
	payload.get("decisions").and_then(Value::as_array).is_some_and(|decisions| {
		decisions.iter().any(|decision| {
			let Some(decision) = decision.as_object() else {
				return false;
			};
			let dimension = decision.get("dimension").and_then(Value::as_str);
			let previous = decision.get("previous_value").and_then(Value::as_f64);
			let next = decision.get("next_value").and_then(Value::as_f64);

			matches!(dimension, Some("topic_weight" | "format_preference"))
				&& previous.zip(next).is_some_and(|(previous, next)| previous != next)
		})
	})
}
