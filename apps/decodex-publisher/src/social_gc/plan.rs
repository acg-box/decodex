use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use time::{Date, Duration, Month, OffsetDateTime, Time, format_description::well_known::Rfc3339};

use super::{
	GcPolicy, current_billing_month,
	inventory::{ArtifactKind, ArtifactRecord, AttemptRecord, AttemptValue, Inventory, StoredFile},
	one_string_ref, required_string,
};
use crate::social_xurl::model::{XurlAttempt, XurlObservationAttempt};

const MAX_CLOCK_SKEW: Duration = Duration::minutes(5);

pub(super) struct DeletionPlan {
	pub(super) files: Vec<StoredFile>,
	pub(super) deleted_lineages: usize,
	pub(super) deleted_strategies: usize,
	pub(super) retained_lineages: usize,
	pub(super) retained_strategies: usize,
	pub(super) retained_by_strategy: bool,
	pub(super) retained_by_current_month: bool,
	pub(super) retained_nonterminal: bool,
	pub(super) retained_by_window: bool,
}
impl DeletionPlan {
	pub(super) fn preflight(&self) -> crate::prelude::Result<()> {
		let mut directories = BTreeMap::new();
		for file in &self.files {
			directories.insert(file.directory.identity()?, file.directory.clone());
		}
		for directory in directories.values() {
			directory.verify_current_path()?;
		}
		for file in &self.files {
			file.preflight()?;
		}

		Ok(())
	}
}

struct StrategyPlan {
	delete_keys: BTreeSet<String>,
	retained_references: BTreeSet<String>,
	deleted_count: usize,
	retained_count: usize,
}

struct Component<'a> {
	candidate: &'a ArtifactRecord,
	reservations: Vec<&'a ArtifactRecord>,
	posts: Vec<&'a ArtifactRecord>,
	outcomes: Vec<&'a ArtifactRecord>,
	attempts: Vec<&'a AttemptRecord>,
}
impl Component<'_> {
	fn artifact_files(&self) -> Vec<StoredFile> {
		let mut files = vec![self.candidate.file.clone()];
		files.extend(self.reservations.iter().map(|record| record.file.clone()));
		files.extend(self.posts.iter().map(|record| record.file.clone()));
		files.extend(self.outcomes.iter().map(|record| record.file.clone()));
		files.sort_by(|left, right| left.key.cmp(&right.key));
		files.dedup_by(|left, right| left.key == right.key);
		files
	}

	fn terminal_files(&self) -> Vec<StoredFile> {
		let mut files = self.artifact_files();
		files.extend(self.attempts.iter().map(|record| record.file.clone()));
		files.sort_by(|left, right| left.key.cmp(&right.key));
		files.dedup_by(|left, right| left.key == right.key);
		files
	}
}

pub(super) fn build(
	inventory: &Inventory,
	policy: GcPolicy,
	now: OffsetDateTime,
) -> Result<DeletionPlan, ()> {
	let cutoff = now - policy.minimum_retention;
	let strategies = plan_strategies(inventory, policy, cutoff, now)?;
	validate_successful_attempt_references(inventory)?;

	let candidates = records_by_kind(inventory, ArtifactKind::Candidate);
	let reservations = records_by_kind(inventory, ArtifactKind::Reservation);
	let posts = records_by_kind(inventory, ArtifactKind::Post);
	let outcomes = records_by_kind(inventory, ArtifactKind::Outcome);
	let current_month = current_billing_month(now);
	let mut files = inventory
		.artifacts
		.iter()
		.filter(|record| strategies.delete_keys.contains(&record.file.key))
		.map(|record| record.file.clone())
		.collect::<Vec<_>>();
	let mut deleted_lineages = 0;
	let mut retained_by_strategy = false;
	let mut retained_by_current_month = false;
	let mut retained_nonterminal = false;
	let mut retained_by_window = false;

	for candidate in candidates.values() {
		let component =
			component_for(candidate, &reservations, &posts, &outcomes, &inventory.attempts);
		let strategy_protected = component
			.artifact_files()
			.iter()
			.any(|file| strategies.retained_references.contains(&file.key))
			|| component.posts.iter().any(|post| {
				post.value
					.get("publication")
					.and_then(Value::as_object)
					.and_then(|publication| publication.get("published_urls"))
					.and_then(Value::as_array)
					.into_iter()
					.flatten()
					.filter_map(Value::as_str)
					.any(|url| strategies.retained_references.contains(url))
			});
		let current_month_protected =
			component.attempts.iter().any(|attempt| attempt.uses_billing_month(&current_month));
		let terminal_at = terminal_component_time(&component, now)?;
		let complete = terminal_at.is_some();
		let old_enough = terminal_at.is_some_and(|value| value <= cutoff);

		if strategy_protected {
			retained_by_strategy = true;
		}
		if current_month_protected {
			retained_by_current_month = true;
		}
		if !complete {
			retained_nonterminal = true;
		} else if !old_enough {
			retained_by_window = true;
		}
		if complete && old_enough && !strategy_protected && !current_month_protected {
			files.extend(component.terminal_files());
			deleted_lineages += 1;
		}
	}

	files.sort_by(|left, right| left.key.cmp(&right.key));
	if files.windows(2).any(|files| files[0].key == files[1].key) {
		return Err(());
	}
	let candidate_count = candidates.len();

	Ok(DeletionPlan {
		files,
		deleted_lineages,
		deleted_strategies: strategies.deleted_count,
		retained_lineages: candidate_count.checked_sub(deleted_lineages).ok_or(())?,
		retained_strategies: strategies.retained_count,
		retained_by_strategy,
		retained_by_current_month,
		retained_nonterminal,
		retained_by_window,
	})
}

fn plan_strategies(
	inventory: &Inventory,
	policy: GcPolicy,
	cutoff: OffsetDateTime,
	now: OffsetDateTime,
) -> Result<StrategyPlan, ()> {
	let mut by_cadence: BTreeMap<&str, Vec<(OffsetDateTime, &ArtifactRecord)>> = BTreeMap::new();
	for record in inventory.artifacts.iter().filter(|record| record.kind == ArtifactKind::Strategy)
	{
		let cadence = required_string(&record.value, "cadence").ok_or(())?;
		let reviewed_at =
			parse_time(required_string(&record.value, "reviewed_at").ok_or(())?, now)?;
		by_cadence.entry(cadence).or_default().push((reviewed_at, record));
	}
	let mut delete_keys = BTreeSet::new();
	for (cadence, records) in &mut by_cadence {
		records.sort_by(|left, right| {
			right.0.cmp(&left.0).then_with(|| right.1.file.key.cmp(&left.1.file.key))
		});
		let keep = match *cadence {
			"daily" => policy.daily_strategy_keep,
			"weekly" => policy.weekly_strategy_keep,
			_ => return Err(()),
		};
		for (index, (reviewed_at, record)) in records.iter().enumerate() {
			if index >= keep && *reviewed_at <= cutoff {
				delete_keys.insert(record.file.key.clone());
			}
		}
	}
	let mut retained_references = BTreeSet::new();
	let mut retained_count = 0;
	for record in inventory.artifacts.iter().filter(|record| record.kind == ArtifactKind::Strategy)
	{
		if delete_keys.contains(&record.file.key) {
			continue;
		}
		retained_count += 1;
		retained_references.extend(
			record
				.value
				.get("evidence_refs")
				.and_then(Value::as_array)
				.into_iter()
				.flatten()
				.filter_map(Value::as_str)
				.map(Into::into),
		);
	}

	Ok(StrategyPlan {
		deleted_count: delete_keys.len(),
		delete_keys,
		retained_references,
		retained_count,
	})
}

fn records_by_kind(inventory: &Inventory, kind: ArtifactKind) -> BTreeMap<&str, &ArtifactRecord> {
	inventory
		.artifacts
		.iter()
		.filter(|record| record.kind == kind)
		.map(|record| (record.file.key.as_str(), record))
		.collect()
}

fn component_for<'a>(
	candidate: &'a ArtifactRecord,
	reservations: &BTreeMap<&str, &'a ArtifactRecord>,
	posts: &BTreeMap<&str, &'a ArtifactRecord>,
	outcomes: &BTreeMap<&str, &'a ArtifactRecord>,
	attempts: &'a [AttemptRecord],
) -> Component<'a> {
	let reservations = reservations
		.values()
		.copied()
		.filter(|record| {
			one_string_ref(&record.value, "candidate_refs", "social_candidates").as_deref()
				== Some(candidate.file.key.as_str())
		})
		.collect::<Vec<_>>();
	let posts = posts
		.values()
		.copied()
		.filter(|record| {
			one_string_ref(&record.value, "source_refs", "social_candidates").as_deref()
				== Some(candidate.file.key.as_str())
		})
		.collect::<Vec<_>>();
	let post_keys = posts.iter().map(|record| record.file.key.as_str()).collect::<BTreeSet<_>>();
	let reservation_keys =
		reservations.iter().map(|record| record.file.key.as_str()).collect::<BTreeSet<_>>();
	let outcomes = outcomes
		.values()
		.copied()
		.filter(|record| {
			record
				.value
				.get("social_post_ref")
				.and_then(Value::as_str)
				.is_some_and(|reference| post_keys.contains(reference))
		})
		.collect::<Vec<_>>();
	let attempts = attempts
		.iter()
		.filter(|record| match &record.value {
			AttemptValue::Publish(value) =>
				value.candidate_ref == candidate.file.key
					|| reservation_keys.contains(value.reservation_ref.as_str()),
			AttemptValue::Observe(value) => post_keys.contains(value.post_ref.as_str()),
		})
		.collect();

	Component { candidate, reservations, posts, outcomes, attempts }
}

fn terminal_component_time(
	component: &Component<'_>,
	now: OffsetDateTime,
) -> Result<Option<OffsetDateTime>, ()> {
	let worthiness = component
		.candidate
		.value
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("worthiness"))
		.and_then(Value::as_str);
	match worthiness {
		Some("publish") => published_component_time(component, now),
		Some("skip") => skipped_component_time(component, now),
		_ => Ok(None),
	}
}

fn published_component_time(
	component: &Component<'_>,
	now: OffsetDateTime,
) -> Result<Option<OffsetDateTime>, ()> {
	if component.reservations.len() != 1
		|| component.posts.len() != 1
		|| component.outcomes.len() != 2
		|| component.attempts.len() != 3
	{
		return Ok(None);
	}
	let reservation = component.reservations[0];
	let post = component.posts[0];
	if reservation.value.get("status").and_then(Value::as_str) != Some("consumed")
		|| post.value.get("status").and_then(Value::as_str) != Some("published")
		|| reservation.value.get("consumed_by_social_post").and_then(Value::as_str)
			!= Some(post.file.key.as_str())
	{
		return Ok(None);
	}
	let post_time = parse_time(
		post.value
			.get("publication")
			.and_then(Value::as_object)
			.and_then(|publication| publication.get("posted_at"))
			.and_then(Value::as_str)
			.ok_or(())?,
		now,
	)?;
	let mut terminal_at = post_time;
	for field in ["reserved_at", "expires_at"] {
		terminal_at = terminal_at
			.max(parse_time(required_string(&reservation.value, field).ok_or(())?, now)?);
	}
	let mut windows = BTreeSet::new();
	for outcome in &component.outcomes {
		let window = required_string(&outcome.value, "window").ok_or(())?;
		windows.insert(window);
		terminal_at = terminal_at
			.max(parse_time(required_string(&outcome.value, "observed_at").ok_or(())?, now)?);
	}
	if windows != BTreeSet::from(["24h", "7d"]) {
		return Ok(None);
	}

	let mut publish_attempt = None;
	let mut observation_attempts = BTreeMap::new();
	for attempt in &component.attempts {
		terminal_at = terminal_at.max(parse_time(attempt.updated_at(), now)?);
		match &attempt.value {
			AttemptValue::Publish(value) =>
				if publish_attempt.replace(value).is_some() {
					return Ok(None);
				},
			AttemptValue::Observe(value) => {
				if observation_attempts.insert(value.window.as_str(), value).is_some() {
					return Ok(None);
				}
			},
		}
	}
	let Some(publish_attempt) = publish_attempt else {
		return Ok(None);
	};
	if publish_attempt.status != "published"
		|| observation_attempts.keys().copied().collect::<BTreeSet<_>>()
			!= BTreeSet::from(["24h", "7d"])
		|| observation_attempts.values().any(|attempt| attempt.status != "observed")
		|| !published_attempt_matches(component.candidate, reservation, post, publish_attempt)
	{
		return Ok(None);
	}
	for outcome in &component.outcomes {
		let window = required_string(&outcome.value, "window").ok_or(())?;
		if !observation_attempt_matches(
			post,
			outcome,
			observation_attempts.get(window).copied().ok_or(())?,
		) {
			return Ok(None);
		}
	}

	Ok(Some(terminal_at))
}

fn published_attempt_matches(
	candidate: &ArtifactRecord,
	reservation: &ArtifactRecord,
	post: &ArtifactRecord,
	attempt: &XurlAttempt,
) -> bool {
	let publication = post.value.get("publication").and_then(Value::as_object);
	let decision = post.value.get("decision").and_then(Value::as_object);
	let run_id = reservation
		.value
		.get("owner")
		.and_then(Value::as_object)
		.and_then(|owner| owner.get("run_id"))
		.and_then(Value::as_str);
	let post_owner_run_id = post
		.value
		.get("owner")
		.and_then(Value::as_object)
		.and_then(|owner| owner.get("run_id"))
		.and_then(Value::as_str);
	attempt.candidate_ref == candidate.file.key
		&& attempt.reservation_ref == reservation.file.key
		&& decision.and_then(|value| value.get("idempotency_key")).and_then(Value::as_str)
			== Some(attempt.idempotency_key.as_str())
		&& run_id == Some(attempt.run_id.as_str())
		&& post_owner_run_id == Some(attempt.run_id.as_str())
		&& post.file.filename().and_then(|name| name.strip_suffix(".json"))
			== Some(attempt.run_id.as_str())
		&& publication.and_then(|value| value.get("post_id")).and_then(Value::as_str)
			== attempt.post_id.as_deref()
		&& publication
			.and_then(|value| value.get("published_urls"))
			.and_then(Value::as_array)
			.and_then(|urls| urls.first())
			.and_then(Value::as_str)
			== attempt.published_url.as_deref()
		&& publication.and_then(|value| value.get("verified_user_id")).and_then(Value::as_str)
			== attempt.verified_user_id.as_deref()
		&& publication.and_then(|value| value.get("xurl_version")).and_then(Value::as_str)
			== Some(attempt.xurl_version.as_str())
		&& publication
			.and_then(|value| value.get("recorded_cost_ceiling_microusd"))
			.and_then(Value::as_u64)
			== Some(attempt.reserved_cost_ceiling_microusd)
		&& call_digest(attempt, "identity_read")
			== publication
				.and_then(|value| value.get("identity_response_sha256"))
				.and_then(Value::as_str)
		&& call_digest(attempt, "content_create")
			== publication
				.and_then(|value| value.get("create_response_sha256"))
				.and_then(Value::as_str)
		&& attempt
			.calls
			.iter()
			.rev()
			.find(|call| call.operation.starts_with("post_read") && call.status == "succeeded")
			.and_then(|call| call.response_sha256.as_deref())
			== publication
				.and_then(|value| value.get("read_response_sha256"))
				.and_then(Value::as_str)
}

fn observation_attempt_matches(
	post: &ArtifactRecord,
	outcome: &ArtifactRecord,
	attempt: &XurlObservationAttempt,
) -> bool {
	let publication = post.value.get("publication").and_then(Value::as_object);
	let observation = outcome.value.get("observation").and_then(Value::as_object);
	let owner_run_id = outcome
		.value
		.get("owner")
		.and_then(Value::as_object)
		.and_then(|owner| owner.get("run_id"))
		.and_then(Value::as_str);
	attempt.post_ref == post.file.key
		&& publication.and_then(|value| value.get("post_id")).and_then(Value::as_str)
			== Some(attempt.post_id.as_str())
		&& outcome.value.get("window").and_then(Value::as_str) == Some(attempt.window.as_str())
		&& outcome.value.get("observed_at").and_then(Value::as_str)
			== Some(attempt.updated_at.as_str())
		&& owner_run_id == Some(attempt.run_id.as_str())
		&& attempt.call.status == "succeeded"
		&& attempt.call.response_sha256.as_deref()
			== observation.and_then(|value| value.get("response_sha256")).and_then(Value::as_str)
}

fn skipped_component_time(
	component: &Component<'_>,
	now: OffsetDateTime,
) -> Result<Option<OffsetDateTime>, ()> {
	if !component.reservations.is_empty()
		|| component.posts.len() != 1
		|| !component.outcomes.is_empty()
		|| !component.attempts.is_empty()
	{
		return Ok(None);
	}
	let post = component.posts[0];
	let candidate_decision =
		component.candidate.value.get("decision").and_then(Value::as_object).ok_or(())?;
	let post_decision = post.value.get("decision").and_then(Value::as_object).ok_or(())?;
	let idempotency_key = post_decision.get("idempotency_key").and_then(Value::as_str).ok_or(())?;
	let expected_post_filename =
		format!("{}.json", crate::social_publish::idempotency_digest(idempotency_key));
	if post.value.get("status").and_then(Value::as_str) != Some("skipped")
		|| post_decision.get("worthiness").and_then(Value::as_str) != Some("skip")
		|| one_string_ref(&post.value, "source_refs", "social_candidates").as_deref()
			!= Some(component.candidate.file.key.as_str())
		|| post.file.filename() != Some(expected_post_filename.as_str())
		|| post.value.get("slug") != component.candidate.value.get("slug")
		|| post.value.get("channel") != component.candidate.value.get("channel")
		|| post.value.get("target_account") != component.candidate.value.get("target_account")
		|| post.value.get("mode") != component.candidate.value.get("mode")
		|| post.value.get("audience") != component.candidate.value.get("audience")
		|| post.value.get("text") != component.candidate.value.get("candidate_text")
		|| post.value.get("evidence_notes") != component.candidate.value.get("evidence_notes")
		|| post.value.get("claims") != component.candidate.value.get("claims")
		|| post_decision.get("priority") != component.candidate.value.get("priority")
		|| post_decision.get("idempotency_key") != candidate_decision.get("idempotency_key")
		|| post_decision.get("reason") != candidate_decision.get("reason")
		|| post.value.get("skip").and_then(Value::as_object).and_then(|skip| skip.get("reason"))
			!= candidate_decision.get("reason")
	{
		return Ok(None);
	}
	let day = post_decision.get("day").and_then(Value::as_str).ok_or(())?;
	let end_of_day = day_end(day)?;
	if end_of_day > now + MAX_CLOCK_SKEW {
		return Err(());
	}

	Ok(Some(end_of_day))
}

fn validate_successful_attempt_references(inventory: &Inventory) -> Result<(), ()> {
	let candidates = records_by_kind(inventory, ArtifactKind::Candidate);
	let reservations = records_by_kind(inventory, ArtifactKind::Reservation);
	let posts = records_by_kind(inventory, ArtifactKind::Post);
	let outcomes = records_by_kind(inventory, ArtifactKind::Outcome);
	for attempt in &inventory.attempts {
		match &attempt.value {
			AttemptValue::Publish(value) if value.status == "published" => {
				let candidate = candidates.get(value.candidate_ref.as_str());
				let reservation = reservations.get(value.reservation_ref.as_str());
				if candidate.is_none() && reservation.is_none() {
					continue;
				}
				let candidate = candidate.ok_or(())?;
				let reservation = reservation.ok_or(())?;
				let matching = posts.values().filter(|post| {
					one_string_ref(&post.value, "source_refs", "social_candidates").as_deref()
						== Some(candidate.file.key.as_str())
						&& one_string_ref(&post.value, "source_refs", "reservations").as_deref()
							== Some(reservation.file.key.as_str())
				});
				if matching.count() != 1 {
					return Err(());
				}
			},
			AttemptValue::Observe(value) if value.status == "observed" => {
				let outcome_count = outcomes
					.values()
					.filter(|outcome| {
						outcome.value.get("social_post_ref").and_then(Value::as_str)
							== Some(value.post_ref.as_str())
							&& outcome.value.get("window").and_then(Value::as_str)
								== Some(value.window.as_str())
					})
					.count();
				if posts.contains_key(value.post_ref.as_str()) {
					if outcome_count != 1 {
						return Err(());
					}
				} else if outcome_count != 0 {
					return Err(());
				}
			},
			_ => {},
		}
	}
	for post in posts
		.values()
		.filter(|post| post.value.get("status").and_then(Value::as_str) == Some("skipped"))
	{
		let candidate_ref =
			one_string_ref(&post.value, "source_refs", "social_candidates").ok_or(())?;
		if !candidates.contains_key(candidate_ref.as_str()) {
			return Err(());
		}
	}

	Ok(())
}

fn call_digest<'a>(attempt: &'a XurlAttempt, operation: &str) -> Option<&'a str> {
	attempt
		.calls
		.iter()
		.find(|call| call.operation == operation && call.status == "succeeded")
		.and_then(|call| call.response_sha256.as_deref())
}

fn parse_time(value: &str, now: OffsetDateTime) -> Result<OffsetDateTime, ()> {
	let parsed = OffsetDateTime::parse(value, &Rfc3339).map_err(|_| ())?;
	if parsed > now + MAX_CLOCK_SKEW {
		return Err(());
	}

	Ok(parsed)
}

fn day_end(value: &str) -> Result<OffsetDateTime, ()> {
	let mut parts = value.split('-');
	let year = parts.next().and_then(|value| value.parse().ok()).ok_or(())?;
	let month: u8 = parts.next().and_then(|value| value.parse().ok()).ok_or(())?;
	let day = parts.next().and_then(|value| value.parse().ok()).ok_or(())?;
	if parts.next().is_some() {
		return Err(());
	}
	let date = Date::from_calendar_date(year, Month::try_from(month).map_err(|_| ())?, day)
		.map_err(|_| ())?;

	Ok(date.with_time(Time::MIDNIGHT).assume_utc() + Duration::days(1))
}
