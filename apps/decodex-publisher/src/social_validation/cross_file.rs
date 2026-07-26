//! Cross-file social artifact uniqueness checks.

use std::{
	collections::{BTreeMap, BTreeSet},
	path::Path,
};

use serde_json::Value;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	SOCIAL_OUTCOME_SCHEMA, SOCIAL_POST_SCHEMA, SOCIAL_PUBLISH_RESERVATION_SCHEMA,
	SOCIAL_STRATEGY_SCHEMA,
};

#[derive(Debug)]
struct OutcomeReference {
	display_path: String,
	observed_at: String,
	post_ref: String,
	published_url: String,
	window: String,
}

#[derive(Debug)]
struct PublishedPost {
	posted_at: String,
	urls: Vec<String>,
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
	outcome_cycles: BTreeMap<String, String>,
	outcome_references: Vec<OutcomeReference>,
	published_posts: BTreeMap<String, PublishedPost>,
	strategy_cycles: BTreeMap<String, String>,
	strategy_references: Vec<StrategyReference>,
	terminal_post_idempotency_keys: BTreeMap<String, String>,
}
impl SocialValidationState {
	pub(crate) fn new() -> Self {
		Self {
			active_reservation_idempotency_keys: BTreeMap::new(),
			outcome_cycles: BTreeMap::new(),
			outcome_references: Vec::new(),
			published_posts: BTreeMap::new(),
			strategy_cycles: BTreeMap::new(),
			strategy_references: Vec::new(),
			terminal_post_idempotency_keys: BTreeMap::new(),
		}
	}

	pub(crate) fn finish(self, errors: &mut Vec<String>) {
		let mut valid_24h_outcomes = BTreeMap::new();

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
		Some(SOCIAL_OUTCOME_SCHEMA) => record_outcome(payload, &display_path, state, errors),
		Some(SOCIAL_POST_SCHEMA) => record_post(payload, &display_path, state, errors),
		Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA) =>
			record_reservation(payload, &display_path, state, errors),
		Some(SOCIAL_STRATEGY_SCHEMA) => record_strategy(payload, &display_path, state, errors),
		_ => {},
	}
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

		state.published_posts.insert(display_path.to_owned(), PublishedPost { posted_at, urls });
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
	if payload.get("status").and_then(Value::as_str) != Some("active") {
		return;
	}

	let Some(key) = payload.get("idempotency_key").and_then(Value::as_str) else {
		return;
	};

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
