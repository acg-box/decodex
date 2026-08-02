//! Small agent-facing workflows over the X safety primitives.

use std::{
	collections::BTreeSet,
	path::{Path, PathBuf},
};

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	DEFAULT_SOCIAL_ATTEMPTS_DIR, DEFAULT_SOCIAL_CANDIDATES_DIR, DEFAULT_SOCIAL_LOCKS_DIR,
	DEFAULT_SOCIAL_OUTCOMES_DIR, DEFAULT_SOCIAL_POSTS_DIR, DEFAULT_SOCIAL_RESERVATIONS_DIR,
	DEFAULT_XURL_AUTH_CONTRACT_PATH, SOCIAL_DAILY_LIMIT, SOCIAL_MONTHLY_BUDGET_MICROUSD,
	SOCIAL_POST_SCHEMA, SOCIAL_TIMEZONE, SocialObserveDueReport, SocialObserveDueRequest,
	SocialObserveXurlReport, SocialObserveXurlRequest, SocialPublishNextReport,
	SocialPublishNextRequest, SocialPublishXurlReport, SocialPublishXurlRequest,
	SocialReconcileXurlReport, SocialReconcileXurlRequest, SocialReservePublishRequest,
	SocialTerminalizeSkipRequest,
	prelude::{Result, eyre},
};

#[cfg(test)]
std::thread_local! {
	static INTERRUPT_AFTER_RESERVATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct WorkflowPaths {
	candidates: PathBuf,
	reservations: PathBuf,
	posts: PathBuf,
	outcomes: PathBuf,
	attempts: PathBuf,
	locks: PathBuf,
	authorization_contract: PathBuf,
}

impl Default for WorkflowPaths {
	fn default() -> Self {
		Self {
			candidates: PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			reservations: PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			posts: PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			outcomes: PathBuf::from(DEFAULT_SOCIAL_OUTCOMES_DIR),
			attempts: PathBuf::from(DEFAULT_SOCIAL_ATTEMPTS_DIR),
			locks: PathBuf::from(DEFAULT_SOCIAL_LOCKS_DIR),
			authorization_contract: PathBuf::from(DEFAULT_XURL_AUTH_CONTRACT_PATH),
		}
	}
}

#[cfg(test)]
impl WorkflowPaths {
	fn under(root: &Path) -> Self {
		Self {
			candidates: root.join("candidates"),
			reservations: root.join("reservations"),
			posts: root.join("posts"),
			outcomes: root.join("outcomes"),
			attempts: root.join("attempts"),
			locks: root.join("locks"),
			authorization_contract: root.join("xurl-authorization-contract.json"),
		}
	}
}

pub(crate) fn publish_next(request: &SocialPublishNextRequest) -> Result<SocialPublishNextReport> {
	publish_next_with(
		request,
		&WorkflowPaths::default(),
		crate::publish_social_xurl,
		crate::reconcile_social_xurl,
	)
}

#[cfg(test)]
pub(crate) fn publish_next_with_test_binary(
	request: &SocialPublishNextRequest,
	state_root: &Path,
	xurl_binary: &Path,
) -> Result<SocialPublishNextReport> {
	publish_next_with(
		request,
		&WorkflowPaths::under(state_root),
		|effect| crate::social_xurl::publish_with_test_binary(effect, xurl_binary),
		|effect| crate::social_xurl::reconcile_with_test_binary(effect, xurl_binary),
	)
}

#[cfg(test)]
pub(crate) fn publish_next_with_identity_interruption_for_test(
	request: &SocialPublishNextRequest,
	state_root: &Path,
	xurl_binary: &Path,
) -> Result<SocialPublishNextReport> {
	publish_next_with(
		request,
		&WorkflowPaths::under(state_root),
		|effect| {
			crate::social_xurl::publish_with_identity_interruption_for_test(effect, xurl_binary)
		},
		|effect| crate::social_xurl::reconcile_with_test_binary(effect, xurl_binary),
	)
}

#[cfg(test)]
pub(crate) fn publish_next_with_reservation_interruption_for_test(
	request: &SocialPublishNextRequest,
	state_root: &Path,
	xurl_binary: &Path,
) -> Result<SocialPublishNextReport> {
	INTERRUPT_AFTER_RESERVATION.with(|interrupt| interrupt.set(true));
	let result = publish_next_with_test_binary(request, state_root, xurl_binary);
	INTERRUPT_AFTER_RESERVATION.with(|interrupt| interrupt.set(false));
	result
}

#[cfg(test)]
pub(crate) fn publish_next_with_reserved_attempt_interruption_for_test(
	request: &SocialPublishNextRequest,
	state_root: &Path,
	xurl_binary: &Path,
) -> Result<SocialPublishNextReport> {
	publish_next_with(
		request,
		&WorkflowPaths::under(state_root),
		|effect| {
			crate::social_xurl::publish_with_reserved_attempt_interruption_for_test(
				effect,
				xurl_binary,
			)
		},
		|effect| crate::social_xurl::reconcile_with_test_binary(effect, xurl_binary),
	)
}

fn publish_next_with(
	request: &SocialPublishNextRequest,
	paths: &WorkflowPaths,
	publish_effect: impl Fn(&SocialPublishXurlRequest) -> Result<SocialPublishXurlReport>,
	reconcile_effect: impl Fn(&SocialReconcileXurlRequest) -> Result<SocialReconcileXurlReport>,
) -> Result<SocialPublishNextReport> {
	validate_run_id(&request.run_id)?;
	if !matches!(request.decision.as_str(), "publish" | "skip") {
		eyre::bail!("decision must be publish or skip");
	}
	if let Some(recovered) = recover_one_interrupted_effect(
		&request.run_id,
		&request.clock.now,
		paths,
		&reconcile_effect,
	)? {
		return Ok(SocialPublishNextReport {
			status: "recovered_interrupted_effect".into(),
			candidate_path: None,
			effect_path: Some(recovered),
			published_url: None,
		});
	}

	let root = crate::repo_root()?;
	let candidates_dir = crate::resolve_against(&root, &paths.candidates);
	let posts_dir = crate::resolve_against(&root, &paths.posts);
	let attempts_dir = crate::resolve_against(&root, &paths.attempts);
	let Some((candidate_path, candidate)) =
		pending_candidate(&root, &candidates_dir, &posts_dir, &attempts_dir)?
	else {
		return Ok(SocialPublishNextReport {
			status: "no_candidate".into(),
			candidate_path: None,
			effect_path: None,
			published_url: None,
		});
	};
	let candidate_ref = crate::path_arg(&root, &candidate_path);
	if let Some(report) =
		terminalize_selected_candidate(request, paths, &candidate_path, &candidate, &candidate_ref)?
	{
		return Ok(report);
	}

	let active_reservation =
		active_reservation_for_candidate(&root, &paths.reservations, &candidate_ref)?;
	let active_reservation = match active_reservation {
		Some(path)
			if crate::social_publish::release_orphaned_active_reservation(
				&path,
				&paths.reservations,
				&paths.attempts,
				&paths.locks,
				&request.run_id,
			)? =>
			None,
		other => other,
	};
	let (reservation_path, created_reservation) = match active_reservation {
		Some(path) => (path, false),
		None => {
			let report = crate::reserve_social_publish(&SocialReservePublishRequest {
				candidate_path: candidate_path.clone(),
				candidates_dir: paths.candidates.clone(),
				reserved_at: request.clock.now.clone(),
				expires_at: request.clock.expires_at.clone(),
				day: request.clock.day.clone(),
				timezone: SOCIAL_TIMEZONE.into(),
				out_dir: paths.reservations.clone(),
				posts_dir: paths.posts.clone(),
				attempts_dir: paths.attempts.clone(),
				locks_dir: paths.locks.clone(),
				run_id: request.run_id.clone(),
				daily_limit: SOCIAL_DAILY_LIMIT,
				dry_run: false,
			})?;
			(PathBuf::from(report.path), true)
		},
	};
	#[cfg(test)]
	if created_reservation && INTERRUPT_AFTER_RESERVATION.with(|interrupt| interrupt.replace(false))
	{
		return Err(eyre::eyre!("simulated interruption after the durable reservation"));
	}
	#[cfg(not(test))]
	let _ = created_reservation;
	let report = publish_effect(&SocialPublishXurlRequest {
		reservation_path,
		authorization_contract_path: paths.authorization_contract.clone(),
		reservations_dir: paths.reservations.clone(),
		candidates_dir: paths.candidates.clone(),
		posts_dir: paths.posts.clone(),
		attempts_dir: paths.attempts.clone(),
		locks_dir: paths.locks.clone(),
		run_id: request.run_id.clone(),
		posted_at: request.clock.now.clone(),
		monthly_budget_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
	})?;
	Ok(SocialPublishNextReport {
		status: report.status,
		candidate_path: Some(candidate_ref),
		effect_path: Some(report.post_path),
		published_url: Some(report.published_url),
	})
}

fn terminalize_selected_candidate(
	request: &SocialPublishNextRequest,
	paths: &WorkflowPaths,
	candidate_path: &Path,
	candidate: &Value,
	candidate_ref: &str,
) -> Result<Option<SocialPublishNextReport>> {
	let candidate_decision = candidate
		.pointer("/decision/worthiness")
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("candidate decision is missing"))?;
	if candidate_decision != "no_op" && request.decision != "skip" {
		return Ok(None);
	}

	let reason =
		if request.decision == "skip" {
			request.reason.clone().filter(|reason| !reason.trim().is_empty()).ok_or_else(|| {
				eyre::eyre!("a non-empty reason is required when decision is skip")
			})?
		} else {
			candidate
				.pointer("/decision/reason")
				.and_then(Value::as_str)
				.ok_or_else(|| eyre::eyre!("no-op candidate reason is missing"))?
				.into()
		};
	let report = crate::terminalize_social_skip(&SocialTerminalizeSkipRequest {
		candidate_path: candidate_path.to_path_buf(),
		candidates_dir: paths.candidates.clone(),
		reservations_dir: paths.reservations.clone(),
		posts_dir: paths.posts.clone(),
		locks_dir: paths.locks.clone(),
		run_id: request.run_id.clone(),
		day: request.clock.day.clone(),
		timezone: SOCIAL_TIMEZONE.into(),
		daily_limit: SOCIAL_DAILY_LIMIT,
		dry_run: false,
		reason: Some(reason),
	})?;
	Ok(Some(SocialPublishNextReport {
		status: report.status,
		candidate_path: Some(candidate_ref.into()),
		effect_path: Some(report.path),
		published_url: None,
	}))
}

pub(crate) fn observe_due(request: &SocialObserveDueRequest) -> Result<SocialObserveDueReport> {
	observe_due_with(
		request,
		&WorkflowPaths::default(),
		crate::observe_social_xurl,
		crate::reconcile_social_xurl,
	)
}

#[cfg(test)]
pub(crate) fn observe_due_with_test_binary(
	request: &SocialObserveDueRequest,
	state_root: &Path,
	xurl_binary: &Path,
) -> Result<SocialObserveDueReport> {
	observe_due_with(
		request,
		&WorkflowPaths::under(state_root),
		|effect| crate::social_xurl::observe_with_test_binary(effect, xurl_binary),
		|effect| crate::social_xurl::reconcile_with_test_binary(effect, xurl_binary),
	)
}

fn observe_due_with(
	request: &SocialObserveDueRequest,
	paths: &WorkflowPaths,
	observe_effect: impl Fn(&SocialObserveXurlRequest) -> Result<SocialObserveXurlReport>,
	reconcile_effect: impl Fn(&SocialReconcileXurlRequest) -> Result<SocialReconcileXurlReport>,
) -> Result<SocialObserveDueReport> {
	validate_run_id(&request.run_id)?;
	let now = OffsetDateTime::parse(&request.observed_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("observed_at must be an RFC3339 timestamp"))?;
	let root = crate::repo_root()?;
	let posts_dir = crate::resolve_against(&root, &paths.posts);
	let outcomes_dir = crate::resolve_against(&root, &paths.outcomes);
	let mut observed =
		crate::social_outcome_store::validated_observed_windows(&root, &outcomes_dir, &posts_dir)?;
	if let Some(recovered) = recover_one_interrupted_effect(
		&request.run_id,
		&request.observed_at,
		paths,
		&reconcile_effect,
	)? {
		return Ok(SocialObserveDueReport {
			status: "recovered_interrupted_effect".into(),
			post_path: None,
			outcome_path: Some(recovered),
			window: None,
		});
	}

	observed.extend(terminal_observation_windows(&root, &paths.attempts, &paths.posts)?);
	let mut due = Vec::new();
	for path in existing_json_files(&posts_dir)? {
		let post = crate::load_json(&path)?;
		crate::validate_generated_social_artifact(&post)?;
		if post.get("schema").and_then(Value::as_str) != Some(SOCIAL_POST_SCHEMA)
			|| post.get("status").and_then(Value::as_str) != Some("published")
		{
			continue;
		}
		let post_ref = crate::path_arg(&root, &path);
		let posted_at = post
			.pointer("/publication/posted_at")
			.and_then(Value::as_str)
			.ok_or_else(|| eyre::eyre!("published post has no posted_at"))
			.and_then(|value| OffsetDateTime::parse(value, &Rfc3339).map_err(Into::into))?;
		let elapsed = (now - posted_at).whole_hours();
		for (window, minimum_hours, order) in [("24h", 23, 0_u8), ("7d", 167, 1_u8)] {
			let eligible = elapsed >= minimum_hours;
			if eligible && !observed.contains(&(post_ref.clone(), window.into())) {
				due.push((posted_at, order, path.clone(), post_ref.clone(), window.to_owned()));
			}
		}
	}
	due.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
	let Some((_, _, post_path, post_ref, window)) = due.into_iter().next() else {
		return Ok(SocialObserveDueReport {
			status: "no_due_outcome".into(),
			post_path: None,
			outcome_path: None,
			window: None,
		});
	};
	let report = observe_effect(&SocialObserveXurlRequest {
		run_id: request.run_id.clone(),
		post_path,
		authorization_contract_path: paths.authorization_contract.clone(),
		posts_dir: paths.posts.clone(),
		outcomes_dir: paths.outcomes.clone(),
		attempts_dir: paths.attempts.clone(),
		locks_dir: paths.locks.clone(),
		observed_at: request.observed_at.clone(),
		window: window.clone(),
		monthly_budget_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
	})?;
	Ok(SocialObserveDueReport {
		status: report.status,
		post_path: Some(post_ref),
		outcome_path: Some(report.outcome_path),
		window: Some(window),
	})
}

fn pending_candidate(
	root: &Path,
	candidates_dir: &Path,
	posts_dir: &Path,
	attempts_dir: &Path,
) -> Result<Option<(PathBuf, Value)>> {
	let consumed = existing_json_files(posts_dir)?
		.into_iter()
		.map(|path| crate::load_json(&path))
		.collect::<Result<Vec<_>>>()?
		.into_iter()
		.filter_map(|post| post.pointer("/source_refs/social_candidates").cloned())
		.filter_map(|refs| refs.as_array().cloned())
		.flatten()
		.filter_map(|value| value.as_str().map(str::to_owned))
		.collect::<BTreeSet<_>>();
	let mut pending = Vec::new();
	for path in existing_json_files(candidates_dir)? {
		let candidate = crate::load_json(&path)?;
		crate::validate_generated_social_artifact(&candidate)?;
		crate::social_evidence::validate_source_evidence(&candidate)?;
		let publication_lineage_sha256 =
			crate::social_record::publication_lineage_sha256(&candidate)?;
		let effect_started = crate::social_xurl::publication_effect_conflict(
			attempts_dir,
			&publication_lineage_sha256,
			None,
		)?
		.is_some();
		if !consumed.contains(&crate::path_arg(root, &path)) && !effect_started {
			pending.push((path, candidate));
		}
	}
	pending.sort_by(|left, right| left.0.cmp(&right.0));
	Ok(pending.into_iter().next())
}

fn active_reservation_for_candidate(
	root: &Path,
	reservations_dir: &Path,
	candidate_ref: &str,
) -> Result<Option<PathBuf>> {
	let directory = crate::resolve_against(root, reservations_dir);
	let mut matches = Vec::new();
	for path in existing_json_files(&directory)? {
		let reservation = crate::load_json(&path)?;
		if reservation.get("status").and_then(Value::as_str) == Some("active")
			&& reservation.pointer("/candidate_refs/social_candidates/0").and_then(Value::as_str)
				== Some(candidate_ref)
		{
			matches.push(path);
		}
	}
	if matches.len() > 1 {
		eyre::bail!("candidate has multiple active reservations");
	}
	Ok(matches.pop())
}

fn terminal_observation_windows(
	root: &Path,
	attempts_dir: &Path,
	posts_dir: &Path,
) -> Result<BTreeSet<(String, String)>> {
	let attempts_dir = crate::resolve_against(root, attempts_dir);
	let mut terminal = BTreeSet::new();
	for path in existing_json_files(&attempts_dir)? {
		let payload = crate::load_json(&path)?;
		if payload.get("schema").and_then(Value::as_str)
			!= Some(crate::social_xurl::model::OBSERVATION_ATTEMPT_SCHEMA)
			|| payload.get("status").and_then(Value::as_str)
				!= Some(crate::social_xurl::model::READ_RECOVERY_EXHAUSTED_STATUS)
		{
			continue;
		}
		if !crate::social_xurl::terminal_observation_recovery(&path, &attempts_dir, posts_dir)? {
			return Err(eyre::eyre!("terminal observation attempt is not terminal"));
		}
		let attempt: crate::social_xurl::model::XurlObservationAttempt =
			serde_json::from_value(payload)
				.map_err(|_| eyre::eyre!("terminal observation attempt is invalid"))?;
		terminal.insert((attempt.post_ref, attempt.window));
	}
	Ok(terminal)
}

fn recover_one_interrupted_effect(
	run_id: &str,
	now: &str,
	paths: &WorkflowPaths,
	reconcile_effect: &impl Fn(&SocialReconcileXurlRequest) -> Result<SocialReconcileXurlReport>,
) -> Result<Option<String>> {
	let root = crate::repo_root()?;
	let attempts_dir = crate::resolve_against(&root, &paths.attempts);
	for path in existing_json_files(&attempts_dir)? {
		let attempt = crate::load_json(&path)?;
		let schema = attempt.get("schema").and_then(Value::as_str);
		let status = attempt.get("status").and_then(Value::as_str);
		let skip_recovery = match schema {
			Some(crate::social_xurl::model::ATTEMPT_SCHEMA) => {
				let typed: crate::social_xurl::model::XurlAttempt =
					serde_json::from_value(attempt.clone())
						.map_err(|_| eyre::eyre!("xurl publication attempt is invalid"))?;
				crate::social_xurl::ledger::validate_publication_cost_record(&typed)?;
				if (status == Some("reserved")
					&& !configured_reservation_exists(
						&root,
						&paths.reservations,
						&typed.reservation_ref,
					)?) || matches!(
					status,
					Some("create_inflight" | "create_uncertain" | "published")
				) {
					true
				} else {
					crate::social_xurl::terminal_publication_recovery(
						&path,
						&paths.attempts,
						&paths.reservations,
					)?
				}
			},
			Some(crate::social_xurl::model::OBSERVATION_ATTEMPT_SCHEMA) => {
				let typed: crate::social_xurl::model::XurlObservationAttempt =
					serde_json::from_value(attempt.clone())
						.map_err(|_| eyre::eyre!("xurl observation attempt is invalid"))?;
				crate::social_xurl::ledger::validate_observation_cost_record(&typed)?;
				status == Some("observed")
					|| crate::social_xurl::terminal_observation_recovery(
						&path,
						&paths.attempts,
						&paths.posts,
					)?
			},
			_ => continue,
		};
		if skip_recovery {
			continue;
		}
		let report = reconcile_effect(&SocialReconcileXurlRequest {
			evidence_path: PathBuf::new(),
			attempt_path: Some(path),
			authorization_contract_path: paths.authorization_contract.clone(),
			reservations_dir: paths.reservations.clone(),
			candidates_dir: paths.candidates.clone(),
			posts_dir: paths.posts.clone(),
			outcomes_dir: paths.outcomes.clone(),
			attempts_dir: paths.attempts.clone(),
			locks_dir: paths.locks.clone(),
			operation_id: run_id.into(),
			reconciled_at: now.into(),
		})?;
		return Ok(Some(report.artifact_path));
	}
	Ok(None)
}

fn configured_reservation_exists(
	root: &Path,
	reservations_dir: &Path,
	reservation_ref: &str,
) -> Result<bool> {
	let reservations_dir = crate::resolve_against(root, reservations_dir);
	let reservation_path = crate::resolve_against(root, Path::new(reservation_ref));
	if !reservation_path.starts_with(&reservations_dir) {
		return Ok(false);
	}
	match std::fs::symlink_metadata(&reservation_path) {
		Ok(_) => Ok(true),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
		Err(error) => Err(error.into()),
	}
}

fn existing_json_files(path: &Path) -> Result<Vec<PathBuf>> {
	if !path.exists() {
		return Ok(Vec::new());
	}
	crate::collect_json_files(&[path.to_path_buf()])
}

fn validate_run_id(run_id: &str) -> Result<()> {
	if !crate::social_publish::valid_run_id(run_id) {
		eyre::bail!("run_id must be a lowercase UUID");
	}
	Ok(())
}
