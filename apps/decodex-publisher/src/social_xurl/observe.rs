use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
	ledger,
	model::{
		MAX_OBSERVATION_RECOVERY_CALLS, OBSERVATION_ATTEMPT_SCHEMA, READ_COST_MICROUSD,
		READ_RECOVERY_EXHAUSTED_STATUS, TARGET_ACCOUNT, XURL_APP, XurlCall, XurlObservationAttempt,
	},
	pricing, runtime,
};
use crate::{
	SOCIAL_MONTHLY_BUDGET_MICROUSD, SOCIAL_POST_SCHEMA, SocialObserveXurlReport,
	SocialObserveXurlRequest, SocialReconcileXurlReport, SocialReconcileXurlRequest,
	prelude::{Result, eyre},
};

struct ObserveContext {
	root: PathBuf,
	post_path: PathBuf,
	outcome_path: PathBuf,
	attempts_dir: PathBuf,
	attempt_path: PathBuf,
	billing_month: String,
	published_url: String,
	post_id: String,
	publication_lineage_sha256: String,
	text: String,
	verified_user_id: String,
	xurl_version: String,
	authorization_contract_sha256: String,
}

struct PreparedObservation {
	context: ObserveContext,
	post: Value,
	existing_outcome: Option<Value>,
	outcomes_dir: PathBuf,
	provenance: Option<super::auth_contract::VerifiedAuthorizationContract>,
}

struct OutcomeRecovery {
	attempt: XurlObservationAttempt,
	context: ObserveContext,
	synthetic_request: SocialObserveXurlRequest,
	post: Value,
}

struct RecoveryPost {
	post: Value,
	post_path: PathBuf,
	published_url: String,
	post_id: String,
	publication_lineage_sha256: String,
	text: String,
	verified_user_id: String,
	xurl_version: String,
}

enum OutcomeRecoveryEligibility {
	Retry,
	Exhausted,
}

pub(super) fn run(
	request: &SocialObserveXurlRequest,
	xurl_binary: &runtime::TrustedXurlBinary,
) -> Result<SocialObserveXurlReport> {
	run_with_pricing_check(request, xurl_binary, pricing::require_current_at)
}

#[cfg(test)]
pub(super) fn run_without_pricing_for_test(
	request: &SocialObserveXurlRequest,
	xurl_binary: &runtime::TrustedXurlBinary,
) -> Result<SocialObserveXurlReport> {
	run_with_pricing_check(request, xurl_binary, |_| Ok(()))
}

fn run_with_pricing_check(
	request: &SocialObserveXurlRequest,
	xurl_binary: &runtime::TrustedXurlBinary,
	require_current_pricing: impl FnOnce(OffsetDateTime) -> Result<()>,
) -> Result<SocialObserveXurlReport> {
	let requested_observed_at = validate_request(request)?;
	let _state_lock = crate::social_publish::scan::acquire_social_state_lock(&request.locks_dir)?;
	let mut prepared =
		prepare_observation(request, xurl_binary, requested_observed_at, require_current_pricing)?;
	if let Some(outcome) = prepared.existing_outcome {
		return finish_existing(request, &prepared.context, &outcome);
	}
	let mut provenance = prepared
		.provenance
		.take()
		.ok_or_else(|| eyre::eyre!("xurl authorization contract is unavailable"))?;
	execute_observation(
		request,
		xurl_binary,
		&mut provenance,
		&prepared.context,
		&prepared.post,
		&prepared.outcomes_dir,
	)
}

pub(super) fn reconcile_local(
	request: &SocialReconcileXurlRequest,
	outcome_path: &Path,
	reconciled_at: OffsetDateTime,
) -> Result<SocialReconcileXurlReport> {
	let root = crate::repo_root()?;
	let outcomes_dir = crate::resolve_against(&root, &request.outcomes_dir);
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let attempts_dir = crate::resolve_against(&root, &request.attempts_dir);
	crate::require_contained_regular_file(outcome_path, &outcomes_dir)
		.map_err(|error| eyre::eyre!("reconciliation outcome is invalid: {error}"))?;
	let (outcome, outcome_sha256) = crate::load_json_with_sha256(outcome_path)?;
	crate::validate_generated_social_artifact(&outcome)
		.map_err(|error| eyre::eyre!("reconciliation outcome failed validation: {error}"))?;
	let original_run_id = outcome_owner_run_id(&outcome)?;
	if request.operation_id == original_run_id {
		return Err(eyre::eyre!(
			"reconciliation operation_id must differ from the original observation run"
		));
	}
	if outcome_path.file_stem().and_then(|value| value.to_str()) != Some(original_run_id) {
		return Err(eyre::eyre!("outcome path does not match its original owner run"));
	}
	let post_ref = required_string(&outcome, "social_post_ref")?;
	let post_path = crate::resolve_against(&root, Path::new(post_ref));
	crate::require_contained_regular_file(&post_path, &posts_dir)
		.map_err(|error| eyre::eyre!("reconciliation social post is invalid: {error}"))?;
	let post = load_post(&post_path)?;
	let publication = post["publication"]
		.as_object()
		.ok_or_else(|| eyre::eyre!("published social post has no publication evidence"))?;
	let posted_at =
		OffsetDateTime::parse(required_object_string(publication, "posted_at")?, &Rfc3339)
			.map_err(|_| eyre::eyre!("publication.posted_at is invalid"))?;
	let observed_at = existing_outcome_observed_at(&outcome)?;
	let window = required_string(&outcome, "window")?;
	validate_outcome_window(posted_at, observed_at, window)?;
	let post_id = required_object_string(publication, "post_id")?.to_owned();
	let verified_user_id = required_object_string(publication, "verified_user_id")?.to_owned();
	let published_url = required_string(&outcome, "published_url")?.to_owned();
	if published_url != runtime::canonical_status_url(&post_id)
		|| outcome.get("social_post_ref").and_then(Value::as_str)
			!= Some(crate::path_arg(&root, &post_path).as_str())
		|| publication
			.get("published_urls")
			.and_then(Value::as_array)
			.and_then(|urls| urls.first())
			.and_then(Value::as_str)
			!= Some(&published_url)
	{
		return Err(eyre::eyre!("outcome does not match its durable social post"));
	}
	let text = post
		.get("text")
		.and_then(Value::as_array)
		.and_then(|items| items.first())
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("published social post text is missing"))?
		.to_owned();
	let billing_month = format!("{:04}-{:02}", observed_at.year(), u8::from(observed_at.month()));
	let attempt_key = runtime::sha256(format!("{post_ref}\0{window}").as_bytes());
	let attempt_path =
		attempts_dir.join(&billing_month).join(format!("observe-{attempt_key}.json"));
	crate::require_contained_regular_file(&attempt_path, &attempts_dir)
		.map_err(|error| eyre::eyre!("reconciliation observation attempt is invalid: {error}"))?;
	let durable_attempt = ledger::load_observation_attempt(&attempt_path)?;
	let authorization_contract_sha256 = required_authorization_contract_digest(&durable_attempt)?;
	let observation = required_observation(&outcome)?;
	let xurl_version = required_object_string(observation, "xurl_version")?.to_owned();
	let context = ObserveContext {
		root: root.clone(),
		post_path: post_path.clone(),
		outcome_path: outcome_path.to_path_buf(),
		attempts_dir,
		attempt_path: attempt_path.clone(),
		billing_month,
		published_url,
		post_id,
		publication_lineage_sha256: durable_attempt.publication_lineage_sha256,
		text,
		verified_user_id,
		xurl_version,
		authorization_contract_sha256,
	};
	let synthetic_request = SocialObserveXurlRequest {
		run_id: original_run_id.into(),
		post_path,
		authorization_contract_path: PathBuf::from(crate::DEFAULT_XURL_AUTH_CONTRACT_PATH),
		posts_dir,
		outcomes_dir,
		attempts_dir: context.attempts_dir.clone(),
		locks_dir: crate::resolve_against(&root, &request.locks_dir),
		observed_at: required_string(&outcome, "observed_at")?.into(),
		window: window.into(),
		monthly_budget_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
	};
	let response_sha256 = required_object_string(observation, "response_sha256")?;
	let changed = finalize_outcome_reconciliation(
		request,
		original_run_id,
		&context,
		&synthetic_request,
		response_sha256,
		&outcome_sha256,
		reconciled_at,
	)?;

	Ok(super::reconcile::report(super::reconcile::ReportInput {
		status: if changed { "reconciled" } else { "already_terminal" },
		kind: "outcome",
		request,
		original_run_id,
		root: &root,
		artifact_path: outcome_path,
		attempt_path: &attempt_path,
		paid_call_count: 0,
	}))
}

pub(super) fn reconcile_safe_read(
	request: &SocialReconcileXurlRequest,
	attempt_path: &Path,
	reconciled_at: OffsetDateTime,
	binary_source: &super::reconcile::BinarySource,
	require_pricing: bool,
) -> Result<SocialReconcileXurlReport> {
	let mut recovery = prepare_outcome_recovery(request, attempt_path, reconciled_at)?;
	if recovery.context.outcome_path.exists() {
		return reconcile_local(request, &recovery.context.outcome_path, reconciled_at);
	}
	validate_attempt(&recovery.attempt, &recovery.synthetic_request, &recovery.context)?;
	require_monotonic_recovery_time(&recovery.attempt, reconciled_at)?;
	if recovery.attempt.status == READ_RECOVERY_EXHAUSTED_STATUS {
		return finalize_outcome_recovery_exhaustion(request, &mut recovery, 0);
	}
	if matches!(
		require_eligible_outcome_recovery(request, &recovery.attempt)?,
		OutcomeRecoveryEligibility::Exhausted
	) || ledger::remaining_lineage_budget(
		&recovery.context.attempts_dir,
		&recovery.context.publication_lineage_sha256,
	)? < READ_COST_MICROUSD
	{
		return finalize_outcome_recovery_exhaustion(request, &mut recovery, 0);
	}
	require_outcome_recovery_budget(request, &recovery)?;
	if require_pricing {
		pricing::require_current_at(reconciled_at)?;
	}
	let binary = binary_source.load()?;
	let mut provenance = super::auth_contract::load_current_at(
		&request.authorization_contract_path,
		reconciled_at,
		&binary,
	)?;
	if provenance.contract_sha256() != recovery.context.authorization_contract_sha256 {
		return Err(eyre::eyre!(
			"xurl outcome recovery authorization contract does not match its durable attempt"
		));
	}
	runtime::verify_ready(&binary, &provenance)?;
	reserve_outcome_recovery(request, &mut recovery)?;
	let report =
		execute_outcome_recovery(request, attempt_path, &binary, &mut provenance, &mut recovery)?;
	binary.require_command_time_remaining()?;
	Ok(report)
}

pub(super) fn terminal_recovery(
	attempt_path: &Path,
	attempts_dir: &Path,
	posts_dir: &Path,
) -> Result<bool> {
	let root = crate::repo_root()?;
	let attempt_path = crate::resolve_against(&root, attempt_path);
	let attempts_dir = crate::resolve_against(&root, attempts_dir);
	crate::require_contained_regular_file(&attempt_path, &attempts_dir)
		.map_err(|error| eyre::eyre!("terminal observation attempt is invalid: {error}"))?;
	let attempt = ledger::load_observation_attempt(&attempt_path)?;
	ledger::validate_observation_cost_record(&attempt)?;
	if attempt.status != READ_RECOVERY_EXHAUSTED_STATUS {
		return Ok(false);
	}
	let expected_key =
		runtime::sha256(format!("{}\0{}", attempt.post_ref, attempt.window).as_bytes());
	if attempt_path
		!= attempts_dir.join(&attempt.billing_month).join(format!("observe-{expected_key}.json"))
	{
		return Err(eyre::eyre!("terminal observation attempt path is not canonical"));
	}
	let posts_dir = crate::resolve_against(&root, posts_dir);
	let post_path = crate::resolve_against(&root, Path::new(&attempt.post_ref));
	crate::require_contained_regular_file(&post_path, &posts_dir)
		.map_err(|error| eyre::eyre!("terminal observation post is invalid: {error}"))?;
	let (post, post_sha256) = crate::load_json_with_sha256(&post_path)?;
	load_post(&post_path)?;
	let publication = post
		.get("publication")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("terminal observation post has no publication"))?;
	if required_object_string(publication, "post_id")? != attempt.post_id
		|| required_object_string(publication, "publication_lineage_sha256")?
			!= attempt.publication_lineage_sha256
	{
		return Err(eyre::eyre!("terminal observation attempt does not match its post"));
	}
	let reconciliation = attempt
		.reconciliation
		.as_ref()
		.ok_or_else(|| eyre::eyre!("terminal observation recovery stamp is missing"))?;
	if reconciliation.reconciled_at != attempt.updated_at {
		return Err(eyre::eyre!("terminal observation recovery timestamp does not match"));
	}
	super::reconcile::validate_stamp(
		reconciliation,
		&attempt.run_id,
		&attempt.post_ref,
		&post_sha256,
	)?;
	Ok(true)
}

fn prepare_outcome_recovery(
	request: &SocialReconcileXurlRequest,
	attempt_path: &Path,
	reconciled_at: OffsetDateTime,
) -> Result<OutcomeRecovery> {
	let root = crate::repo_root()?;
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let outcomes_dir = crate::resolve_against(&root, &request.outcomes_dir);
	let attempts_dir = crate::resolve_against(&root, &request.attempts_dir);
	let attempt = ledger::load_observation_attempt(attempt_path)?;
	if attempt.schema != OBSERVATION_ATTEMPT_SCHEMA
		|| !crate::social_publish::valid_run_id(&attempt.run_id)
		|| request.operation_id == attempt.run_id
	{
		return Err(eyre::eyre!("xurl outcome recovery owner is invalid"));
	}
	let post_path = crate::resolve_against(&root, Path::new(&attempt.post_ref));
	crate::require_contained_regular_file(&post_path, &posts_dir)
		.map_err(|error| eyre::eyre!("outcome recovery post is invalid: {error}"))?;
	let expected_key = runtime::sha256(
		format!("{}\0{}", crate::path_arg(&root, &post_path), attempt.window).as_bytes(),
	);
	if attempt_path
		!= attempts_dir.join(&attempt.billing_month).join(format!("observe-{expected_key}.json"))
	{
		return Err(eyre::eyre!("xurl outcome recovery attempt path is not canonical"));
	}
	let recovered_post = load_recovery_post(&post_path, &attempt, reconciled_at)?;
	let outcome_path = outcomes_dir.join(format!("{}.json", attempt.run_id));
	let synthetic_request = SocialObserveXurlRequest {
		run_id: attempt.run_id.clone(),
		post_path: recovered_post.post_path.clone(),
		authorization_contract_path: request.authorization_contract_path.clone(),
		posts_dir,
		outcomes_dir: outcomes_dir.clone(),
		attempts_dir: attempts_dir.clone(),
		locks_dir: crate::resolve_against(&root, &request.locks_dir),
		observed_at: request.reconciled_at.clone(),
		window: attempt.window.clone(),
		monthly_budget_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
	};
	let context = ObserveContext {
		root: root.clone(),
		post_path: recovered_post.post_path,
		outcome_path,
		attempts_dir,
		attempt_path: attempt_path.to_path_buf(),
		billing_month: attempt.billing_month.clone(),
		published_url: recovered_post.published_url,
		post_id: recovered_post.post_id,
		publication_lineage_sha256: recovered_post.publication_lineage_sha256,
		text: recovered_post.text,
		verified_user_id: recovered_post.verified_user_id,
		xurl_version: recovered_post.xurl_version,
		authorization_contract_sha256: required_authorization_contract_digest(&attempt)?,
	};

	Ok(OutcomeRecovery { attempt, context, synthetic_request, post: recovered_post.post })
}

fn load_recovery_post(
	post_path: &Path,
	attempt: &XurlObservationAttempt,
	reconciled_at: OffsetDateTime,
) -> Result<RecoveryPost> {
	let post = load_post(post_path)?;
	let publication = post["publication"]
		.as_object()
		.ok_or_else(|| eyre::eyre!("outcome recovery post has no publication evidence"))?;
	let posted_at =
		OffsetDateTime::parse(required_object_string(publication, "posted_at")?, &Rfc3339)
			.map_err(|_| eyre::eyre!("outcome recovery publication time is invalid"))?;
	validate_outcome_window(posted_at, reconciled_at, &attempt.window)?;
	let post_id = required_object_string(publication, "post_id")?.to_owned();
	if post_id != attempt.post_id {
		return Err(eyre::eyre!("outcome recovery post ID does not match its attempt"));
	}
	if required_object_string(publication, "publication_lineage_sha256")?
		!= attempt.publication_lineage_sha256
	{
		return Err(eyre::eyre!("outcome recovery post lineage does not match its attempt"));
	}
	let published_url = publication
		.get("published_urls")
		.and_then(Value::as_array)
		.and_then(|urls| urls.first())
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("outcome recovery publication URL is missing"))?
		.to_owned();
	if published_url != runtime::canonical_status_url(&post_id) {
		return Err(eyre::eyre!("outcome recovery publication URL is invalid"));
	}
	let text = post
		.get("text")
		.and_then(Value::as_array)
		.and_then(|items| items.first())
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("outcome recovery post text is missing"))?
		.to_owned();
	let verified_user_id = required_object_string(publication, "verified_user_id")?.to_owned();
	let publication_lineage_sha256 =
		required_object_string(publication, "publication_lineage_sha256")?.to_owned();
	let xurl_version = required_object_string(publication, "xurl_version")?.to_owned();

	Ok(RecoveryPost {
		post,
		post_path: post_path.to_path_buf(),
		published_url,
		post_id,
		publication_lineage_sha256,
		text,
		verified_user_id,
		xurl_version,
	})
}

fn require_eligible_outcome_recovery(
	request: &SocialReconcileXurlRequest,
	attempt: &XurlObservationAttempt,
) -> Result<OutcomeRecoveryEligibility> {
	let recovery_count =
		attempt.calls.iter().filter(|call| call.operation == "outcome_read_reconcile").count();
	let last = attempt
		.calls
		.last()
		.ok_or_else(|| eyre::eyre!("outcome recovery attempt has no paid read"))?;
	if !matches!(
		attempt.status.as_str(),
		"read_inflight" | "read_reconcile_inflight" | "read_reconcile_halted" | "halted"
	) || !matches!(last.operation.as_str(), "outcome_read" | "outcome_read_reconcile")
		|| !matches!(last.status.as_str(), "inflight" | "failed" | "invalid" | "uncertain")
	{
		return Err(eyre::eyre!("interrupted outcome read recovery is ineligible"));
	}
	if recovery_count >= MAX_OBSERVATION_RECOVERY_CALLS {
		return Ok(OutcomeRecoveryEligibility::Exhausted);
	}
	if attempt.calls.iter().any(|call| call.operation_id.as_deref() == Some(&request.operation_id))
	{
		return Err(eyre::eyre!("interrupted outcome read recovery reuses an owner"));
	}

	Ok(OutcomeRecoveryEligibility::Retry)
}

fn require_monotonic_recovery_time(
	attempt: &XurlObservationAttempt,
	reconciled_at: OffsetDateTime,
) -> Result<()> {
	let created_at = OffsetDateTime::parse(&attempt.created_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("xurl observation attempt created_at is invalid"))?;
	let updated_at = OffsetDateTime::parse(&attempt.updated_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("xurl observation attempt updated_at is invalid"))?;
	if created_at > updated_at || updated_at > reconciled_at {
		return Err(eyre::eyre!("xurl observation recovery timestamps are not monotonic"));
	}
	Ok(())
}

fn require_outcome_recovery_budget(
	request: &SocialReconcileXurlRequest,
	recovery: &OutcomeRecovery,
) -> Result<()> {
	let reconciled_at = OffsetDateTime::parse(&request.reconciled_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("reconciled_at must be an RFC3339 timestamp"))?;
	let billing_month =
		format!("{:04}-{:02}", reconciled_at.year(), u8::from(reconciled_at.month()));
	ledger::ensure_budget(&recovery.context.attempts_dir, &billing_month, READ_COST_MICROUSD)?;
	ledger::ensure_lineage_budget(
		&recovery.context.attempts_dir,
		&recovery.context.publication_lineage_sha256,
		READ_COST_MICROUSD,
	)?;
	Ok(())
}

fn reserve_outcome_recovery(
	request: &SocialReconcileXurlRequest,
	recovery: &mut OutcomeRecovery,
) -> Result<()> {
	let reconciled_at = OffsetDateTime::parse(&request.reconciled_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("reconciled_at must be an RFC3339 timestamp"))?;
	let billing_month =
		format!("{:04}-{:02}", reconciled_at.year(), u8::from(reconciled_at.month()));
	ledger::reserve_observation_reconcile_call(
		&recovery.context.attempt_path,
		&mut recovery.attempt,
		&recovery.context.attempts_dir,
		XurlCall {
			operation: "outcome_read_reconcile".into(),
			operation_id: Some(request.operation_id.clone()),
			billing_month: Some(billing_month),
			status: "inflight".into(),
			recorded_cost_ceiling_microusd: READ_COST_MICROUSD,
			response_sha256: None,
		},
		&request.reconciled_at,
	)
}

fn execute_outcome_recovery(
	request: &SocialReconcileXurlRequest,
	attempt_path: &Path,
	binary: &runtime::TrustedXurlBinary,
	provenance: &mut super::auth_contract::VerifiedAuthorizationContract,
	recovery: &mut OutcomeRecovery,
) -> Result<SocialReconcileXurlReport> {
	let mut output =
		match runtime::read(binary, provenance, &recovery.context.post_id, "outcome_read") {
			Ok(output) => output,
			Err(error) => {
				ledger::finish_observation_call(
					attempt_path,
					&mut recovery.attempt,
					"failed",
					"read_reconcile_halted",
					&request.reconciled_at,
					None,
				)?;
				if outcome_recovery_exhausted(&recovery.attempt) {
					return finalize_outcome_recovery_exhaustion(request, recovery, 1);
				}
				return Err(error);
			},
		};
	let (response, response_sha256) = match runtime::parse_read(
		&mut output,
		provenance,
		&recovery.context.post_id,
		&recovery.context.text,
		&recovery.context.verified_user_id,
	) {
		Ok(result) => result,
		Err(error) => {
			let call_status = if output.status.success() { "invalid" } else { "failed" };
			ledger::finish_observation_call(
				attempt_path,
				&mut recovery.attempt,
				call_status,
				"read_reconcile_halted",
				&request.reconciled_at,
				Some(runtime::sha256(&output.stdout)),
			)?;
			if outcome_recovery_exhausted(&recovery.attempt) {
				return finalize_outcome_recovery_exhaustion(request, recovery, 1);
			}
			return Err(error);
		},
	};
	let outcome = outcome_payload(
		&recovery.synthetic_request,
		&recovery.context,
		&recovery.post,
		&response,
		&response_sha256,
	)?;
	crate::validate_generated_social_artifact(&outcome)
		.map_err(|error| eyre::eyre!("recovered social outcome failed validation: {error}"))?;
	crate::write_new_json(&recovery.context.outcome_path, &outcome)?;
	let (_, outcome_sha256) = crate::load_json_with_sha256(&recovery.context.outcome_path)?;
	let stamp = super::reconcile::stamp(
		&request.operation_id,
		&request.reconciled_at,
		crate::path_arg(&recovery.context.root, &recovery.context.outcome_path),
		outcome_sha256,
	);
	ledger::reconcile_observation(
		attempt_path,
		&mut recovery.attempt,
		&request.reconciled_at,
		&response_sha256,
		stamp,
	)?;

	Ok(super::reconcile::report(super::reconcile::ReportInput {
		status: "reconciled",
		kind: "outcome_read",
		request,
		original_run_id: &recovery.attempt.run_id,
		root: &recovery.context.root,
		artifact_path: &recovery.context.outcome_path,
		attempt_path,
		paid_call_count: 1,
	}))
}

fn outcome_recovery_exhausted(attempt: &XurlObservationAttempt) -> bool {
	attempt.calls.iter().filter(|call| call.operation == "outcome_read_reconcile").count()
		>= MAX_OBSERVATION_RECOVERY_CALLS
}

fn finalize_outcome_recovery_exhaustion(
	request: &SocialReconcileXurlRequest,
	recovery: &mut OutcomeRecovery,
	paid_call_count: u64,
) -> Result<SocialReconcileXurlReport> {
	let (post, post_sha256) = crate::load_json_with_sha256(&recovery.context.post_path)?;
	if post != recovery.post {
		return Err(eyre::eyre!("outcome recovery post changed before terminalization"));
	}
	let post_ref = crate::path_arg(&recovery.context.root, &recovery.context.post_path);
	let changed = if let Some(stamp) = &recovery.attempt.reconciliation {
		super::reconcile::validate_stamp(stamp, &recovery.attempt.run_id, &post_ref, &post_sha256)?;
		false
	} else {
		let stamp = super::reconcile::stamp(
			&request.operation_id,
			&request.reconciled_at,
			post_ref,
			post_sha256,
		);
		ledger::terminalize_observation(
			&recovery.context.attempt_path,
			&mut recovery.attempt,
			&request.reconciled_at,
			stamp,
		)?;
		true
	};

	Ok(super::reconcile::report(super::reconcile::ReportInput {
		status: if changed { "outcome_read_recovery_exhausted" } else { "already_terminal" },
		kind: "outcome_read",
		request,
		original_run_id: &recovery.attempt.run_id,
		root: &recovery.context.root,
		artifact_path: &recovery.context.post_path,
		attempt_path: &recovery.context.attempt_path,
		paid_call_count,
	}))
}

fn required_observation(outcome: &Value) -> Result<&serde_json::Map<String, Value>> {
	outcome
		.get("observation")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("outcome observation is required"))
}

fn finalize_outcome_reconciliation(
	request: &SocialReconcileXurlRequest,
	original_run_id: &str,
	context: &ObserveContext,
	synthetic_request: &SocialObserveXurlRequest,
	response_sha256: &str,
	outcome_sha256: &str,
	reconciled_at: OffsetDateTime,
) -> Result<bool> {
	let mut attempt = ledger::load_observation_attempt(&context.attempt_path)?;
	validate_attempt(&attempt, synthetic_request, context)?;
	if attempt.pricing_policy_id.as_deref() != Some(super::model::PRICING_POLICY_ID) {
		return Err(eyre::eyre!(
			"xurl observation attempt lacks the current pricing policy binding"
		));
	}
	require_monotonic_recovery_time(&attempt, reconciled_at)?;
	let outcome_ref = crate::path_arg(&context.root, &context.outcome_path);
	let last = attempt
		.calls
		.last()
		.ok_or_else(|| eyre::eyre!("xurl observation attempt has no paid call"))?;
	match (attempt.status.as_str(), last.status.as_str()) {
		("observed", "succeeded") if last.response_sha256.as_deref() == Some(response_sha256) => {
			if let Some(stamp) = &attempt.reconciliation {
				super::reconcile::validate_stamp(
					stamp,
					original_run_id,
					&outcome_ref,
					outcome_sha256,
				)?;
			}
			Ok(false)
		},
		("read_inflight" | "read_reconcile_inflight", "inflight")
			if last.response_sha256.is_none() =>
		{
			let stamp = super::reconcile::stamp(
				&request.operation_id,
				&request.reconciled_at,
				outcome_ref,
				outcome_sha256.into(),
			);
			ledger::reconcile_observation(
				&context.attempt_path,
				&mut attempt,
				&request.reconciled_at,
				response_sha256,
				stamp,
			)?;
			Ok(true)
		},
		_ => Err(eyre::eyre!("outcome has no locally recoverable successful paid-read attempt")),
	}
}

fn outcome_owner_run_id(outcome: &Value) -> Result<&str> {
	let owner = outcome
		.get("owner")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("outcome owner is required"))?;
	if owner.get("automation_id").and_then(Value::as_str) != Some("decodex-xurl-publisher") {
		return Err(eyre::eyre!("outcome owner automation is invalid"));
	}
	let run_id = required_object_string(owner, "run_id")?;
	if !crate::social_publish::valid_run_id(run_id) {
		return Err(eyre::eyre!("outcome owner run_id is invalid"));
	}

	Ok(run_id)
}

fn prepare_observation(
	request: &SocialObserveXurlRequest,
	xurl_binary: &runtime::TrustedXurlBinary,
	requested_observed_at: OffsetDateTime,
	require_current_pricing: impl FnOnce(OffsetDateTime) -> Result<()>,
) -> Result<PreparedObservation> {
	let root = crate::repo_root()?;
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let post_path = crate::resolve_against(&root, &request.post_path);
	let outcomes_dir = crate::resolve_against(&root, &request.outcomes_dir);
	let attempts_dir = crate::resolve_against(&root, &request.attempts_dir);
	crate::require_contained_regular_file(&post_path, &posts_dir)
		.map_err(|error| eyre::eyre!("social post is invalid: {error}"))?;
	let post = load_post(&post_path)?;
	let publication = post["publication"]
		.as_object()
		.ok_or_else(|| eyre::eyre!("published social post has no publication evidence"))?;
	let posted_at =
		OffsetDateTime::parse(required_object_string(publication, "posted_at")?, &Rfc3339)
			.map_err(|_| eyre::eyre!("publication.posted_at is invalid"))?;
	let post_id = required_object_string(publication, "post_id")?.to_owned();
	let verified_user_id = required_object_string(publication, "verified_user_id")?.to_owned();
	let publication_lineage_sha256 =
		required_object_string(publication, "publication_lineage_sha256")?.to_owned();
	let published_url = publication
		.get("published_urls")
		.and_then(Value::as_array)
		.and_then(|urls| urls.first())
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("publication.published_urls is missing"))?
		.to_owned();
	if published_url != runtime::canonical_status_url(&post_id) {
		return Err(eyre::eyre!("published URL does not match the post id"));
	}
	let text = post
		.get("text")
		.and_then(Value::as_array)
		.and_then(|items| items.first())
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("published social post text is missing"))?
		.to_owned();
	let outcome_path = outcomes_dir.join(format!("{}.json", request.run_id));
	let existing_outcome = if outcome_path.exists() {
		let outcome = crate::load_json(&outcome_path)?;
		crate::validate_generated_social_artifact(&outcome)
			.map_err(|error| eyre::eyre!("existing social outcome failed validation: {error}"))?;
		Some(outcome)
	} else {
		None
	};
	let effective_observed_at = existing_outcome
		.as_ref()
		.map(existing_outcome_observed_at)
		.transpose()?
		.unwrap_or(requested_observed_at);
	validate_outcome_window(posted_at, effective_observed_at, &request.window)?;
	let billing_month = format!(
		"{:04}-{:02}",
		effective_observed_at.year(),
		u8::from(effective_observed_at.month())
	);
	let post_ref = crate::path_arg(&root, &post_path);
	let attempt_key = runtime::sha256(format!("{post_ref}\0{}", request.window).as_bytes());
	let attempt_path =
		attempts_dir.join(&billing_month).join(format!("observe-{attempt_key}.json"));
	let (xurl_version, authorization_contract_sha256, provenance) =
		if let Some(outcome) = &existing_outcome {
			let version = required_object_string(
				outcome
					.get("observation")
					.and_then(Value::as_object)
					.ok_or_else(|| eyre::eyre!("existing outcome observation is required"))?,
				"xurl_version",
			)?
			.to_owned();
			let attempt = ledger::load_observation_attempt(&attempt_path)?;
			(version, required_authorization_contract_digest(&attempt)?, None)
		} else {
			require_current_pricing(requested_observed_at)?;
			let provenance = super::auth_contract::load_current_at(
				&request.authorization_contract_path,
				requested_observed_at,
				xurl_binary,
			)?;
			let version = runtime::verify_ready(xurl_binary, &provenance)?;
			let digest = provenance.contract_sha256().into();
			(version, digest, Some(provenance))
		};
	let context = ObserveContext {
		root,
		post_path,
		outcome_path,
		attempts_dir,
		attempt_path,
		billing_month,
		published_url,
		post_id,
		publication_lineage_sha256,
		text,
		verified_user_id,
		xurl_version,
		authorization_contract_sha256,
	};

	Ok(PreparedObservation { context, post, existing_outcome, outcomes_dir, provenance })
}

fn execute_observation(
	request: &SocialObserveXurlRequest,
	xurl_binary: &runtime::TrustedXurlBinary,
	provenance: &mut super::auth_contract::VerifiedAuthorizationContract,
	context: &ObserveContext,
	post: &Value,
	outcomes_dir: &Path,
) -> Result<SocialObserveXurlReport> {
	ensure_no_duplicate_outcome(request, context, outcomes_dir)?;
	if ledger::observation_attempt_exists(
		&context.attempts_dir,
		&crate::path_arg(&context.root, &context.post_path),
		&request.window,
	)? {
		return Err(eyre::eyre!(
			"xurl outcome read was already attempted for this post and window; another paid retry is forbidden"
		));
	}
	let (mut attempt, created) = load_or_create_attempt(request, context)?;
	validate_attempt(&attempt, request, context)?;
	if !created {
		return Err(eyre::eyre!(
			"xurl outcome read was already attempted; another paid retry is forbidden"
		));
	}
	let mut output = match runtime::read(xurl_binary, provenance, &context.post_id, "outcome_read")
	{
		Ok(output) => output,
		Err(error) => {
			ledger::finish_observation_call(
				&context.attempt_path,
				&mut attempt,
				"failed",
				"halted",
				&request.observed_at,
				None,
			)?;
			return Err(error);
		},
	};
	let (response, response_sha256) = match runtime::parse_read(
		&mut output,
		provenance,
		&context.post_id,
		&context.text,
		&context.verified_user_id,
	) {
		Ok(result) => result,
		Err(error) => {
			let call_status = if output.status.success() { "invalid" } else { "failed" };
			ledger::finish_observation_call(
				&context.attempt_path,
				&mut attempt,
				call_status,
				"halted",
				&request.observed_at,
				Some(runtime::sha256(&output.stdout)),
			)?;
			return Err(error);
		},
	};
	let outcome = outcome_payload(request, context, post, &response, &response_sha256)?;
	crate::validate_generated_social_artifact(&outcome)
		.map_err(|error| eyre::eyre!("generated social outcome failed validation: {error}"))?;
	crate::write_new_json(&context.outcome_path, &outcome)?;
	ledger::finish_observation_call(
		&context.attempt_path,
		&mut attempt,
		"succeeded",
		"observed",
		&request.observed_at,
		Some(response_sha256),
	)?;
	report("observed", request, context)
}

fn validate_request(request: &SocialObserveXurlRequest) -> Result<OffsetDateTime> {
	if !crate::social_publish::valid_run_id(&request.run_id) {
		return Err(eyre::eyre!("run_id must be a lowercase UUID"));
	}
	if request.monthly_budget_microusd != SOCIAL_MONTHLY_BUDGET_MICROUSD {
		return Err(eyre::eyre!(
			"monthly_budget_microusd must be {SOCIAL_MONTHLY_BUDGET_MICROUSD}"
		));
	}
	if !matches!(request.window.as_str(), "24h" | "7d") {
		return Err(eyre::eyre!("window must be 24h or 7d"));
	}
	OffsetDateTime::parse(&request.observed_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("observed_at must be an RFC3339 timestamp"))
}

fn load_post(path: &Path) -> Result<Value> {
	let post = crate::load_json(path)?;
	crate::validate_generated_social_artifact(&post)
		.map_err(|error| eyre::eyre!("social post failed validation: {error}"))?;
	if post.get("schema").and_then(Value::as_str) != Some(SOCIAL_POST_SCHEMA)
		|| post.get("status").and_then(Value::as_str) != Some("published")
	{
		return Err(eyre::eyre!("outcome observation requires a published social post"));
	}
	crate::social_evidence::validate_source_evidence(&post)
		.map_err(|error| eyre::eyre!("social post evidence failed validation: {error}"))?;

	Ok(post)
}

fn existing_outcome_observed_at(outcome: &Value) -> Result<OffsetDateTime> {
	if outcome.get("schema").and_then(Value::as_str) != Some("social_outcome/v1") {
		return Err(eyre::eyre!("existing social outcome uses an unsupported schema"));
	}
	let observed_at = outcome
		.get("observed_at")
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("existing social outcome observed_at is required"))?;
	OffsetDateTime::parse(observed_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("existing social outcome observed_at is invalid"))
}

fn validate_outcome_window(
	posted_at: OffsetDateTime,
	observed_at: OffsetDateTime,
	window: &str,
) -> Result<()> {
	let elapsed_hours = (observed_at - posted_at).whole_hours();
	let minimum_hours = match window {
		"24h" => 23,
		"7d" => 167,
		_ => return Err(eyre::eyre!("window must be 24h or 7d")),
	};
	if elapsed_hours < minimum_hours {
		return Err(eyre::eyre!(
			"{window} observation is before its earliest window: elapsed_hours={elapsed_hours}"
		));
	}

	Ok(())
}

fn load_or_create_attempt(
	request: &SocialObserveXurlRequest,
	context: &ObserveContext,
) -> Result<(XurlObservationAttempt, bool)> {
	if context.attempt_path.exists() {
		return Ok((ledger::load_observation_attempt(&context.attempt_path)?, false));
	}
	ledger::ensure_budget(&context.attempts_dir, &context.billing_month, READ_COST_MICROUSD)?;
	ledger::ensure_lineage_budget(
		&context.attempts_dir,
		&context.publication_lineage_sha256,
		READ_COST_MICROUSD,
	)?;
	let initial_call = XurlCall {
		operation: "outcome_read".into(),
		operation_id: None,
		billing_month: None,
		status: "inflight".into(),
		recorded_cost_ceiling_microusd: READ_COST_MICROUSD,
		response_sha256: None,
	};
	let attempt = XurlObservationAttempt {
		schema: OBSERVATION_ATTEMPT_SCHEMA.into(),
		run_id: request.run_id.clone(),
		billing_month: context.billing_month.clone(),
		reserved_cost_ceiling_microusd: READ_COST_MICROUSD,
		status: "read_inflight".into(),
		post_ref: crate::path_arg(&context.root, &context.post_path),
		post_id: context.post_id.clone(),
		publication_lineage_sha256: context.publication_lineage_sha256.clone(),
		window: request.window.clone(),
		created_at: request.observed_at.clone(),
		updated_at: request.observed_at.clone(),
		pricing_policy_id: Some(super::model::PRICING_POLICY_ID.into()),
		authorization_contract_sha256: Some(context.authorization_contract_sha256.clone()),
		call: initial_call.clone(),
		calls: vec![initial_call],
		reconciliation: None,
	};
	crate::write_new_json(&context.attempt_path, &serde_json::to_value(&attempt)?)?;

	Ok((attempt, true))
}

fn validate_attempt(
	attempt: &XurlObservationAttempt,
	request: &SocialObserveXurlRequest,
	context: &ObserveContext,
) -> Result<()> {
	let calls_valid = (1..=3).contains(&attempt.calls.len())
		&& attempt.calls.first().is_some_and(|call| call.operation == "outcome_read")
		&& attempt.calls.iter().skip(1).all(|call| call.operation == "outcome_read_reconcile")
		&& attempt.calls.iter().all(|call| {
			call.recorded_cost_ceiling_microusd == READ_COST_MICROUSD
				&& call.billing_month.as_deref().is_none_or(ledger::valid_billing_month)
				&& (call.operation == "outcome_read") == call.billing_month.is_none()
				&& (call.operation == "outcome_read") == call.operation_id.is_none()
				&& call.operation_id.as_deref().is_none_or(|operation_id| {
					crate::social_publish::valid_run_id(operation_id)
						&& operation_id != attempt.run_id
				}) && matches!(
				call.status.as_str(),
				"inflight" | "succeeded" | "failed" | "invalid" | "uncertain"
			)
		}) && attempt.calls.iter().try_fold(READ_COST_MICROUSD, |total, call| {
		if call.billing_month.is_some() {
			total.checked_add(call.recorded_cost_ceiling_microusd)
		} else {
			Some(total)
		}
	}) == Some(attempt.reserved_cost_ceiling_microusd);
	let mut recovery_owners =
		attempt.calls.iter().filter_map(|call| call.operation_id.as_deref()).collect::<Vec<_>>();
	recovery_owners.sort_unstable();
	if attempt.schema != OBSERVATION_ATTEMPT_SCHEMA
		|| attempt.run_id != request.run_id
		|| attempt.billing_month != context.billing_month
		|| !matches!(
			attempt.status.as_str(),
			"read_inflight"
				| "read_reconcile_inflight"
				| "read_reconcile_halted"
				| READ_RECOVERY_EXHAUSTED_STATUS
				| "halted" | "observed"
		) || attempt.post_ref != crate::path_arg(&context.root, &context.post_path)
		|| attempt.post_id != context.post_id
		|| attempt.publication_lineage_sha256 != context.publication_lineage_sha256
		|| attempt.window != request.window
		|| attempt.pricing_policy_id.as_deref() != Some(super::model::PRICING_POLICY_ID)
		|| attempt.authorization_contract_sha256.as_deref()
			!= Some(&context.authorization_contract_sha256)
		|| attempt.calls.last() != Some(&attempt.call)
		|| !calls_valid
		|| recovery_owners.windows(2).any(|window| window[0] == window[1])
		|| OffsetDateTime::parse(&attempt.created_at, &Rfc3339).is_err()
		|| OffsetDateTime::parse(&attempt.updated_at, &Rfc3339).is_err()
	{
		return Err(eyre::eyre!("existing xurl observation attempt does not match this outcome"));
	}

	Ok(())
}

fn outcome_payload(
	request: &SocialObserveXurlRequest,
	context: &ObserveContext,
	post: &Value,
	response: &Value,
	response_sha256: &str,
) -> Result<Value> {
	Ok(json!({
		"schema": "social_outcome/v1",
		"slug": format!("{}-{}", required_string(post, "slug")?, request.window),
		"target_account": TARGET_ACCOUNT,
		"owner": {
			"automation_id": "decodex-xurl-publisher",
			"run_id": request.run_id,
		},
		"social_post_ref": crate::path_arg(&context.root, &context.post_path),
		"published_url": context.published_url,
		"observed_at": request.observed_at,
		"window": request.window,
			"metrics": outcome_metrics(response)?,
			"observation": {
			"reader": "xurl",
			"xurl_version": context.xurl_version,
			"xurl_app": XURL_APP,
				"verified_account": TARGET_ACCOUNT,
				"publication_lineage_sha256": context.publication_lineage_sha256,
			"response_sha256": response_sha256,
			"recorded_cost_ceiling_microusd": READ_COST_MICROUSD,
		},
		"notes": ["Metrics were read by post ID through the official xurl CLI."],
	}))
}

fn ensure_no_duplicate_outcome(
	request: &SocialObserveXurlRequest,
	context: &ObserveContext,
	outcomes_dir: &Path,
) -> Result<()> {
	for path in crate::collect_json_files(&[outcomes_dir.to_path_buf()])? {
		if path == context.outcome_path {
			continue;
		}
		let outcome = crate::load_json(&path)?;
		if outcome.get("schema").and_then(Value::as_str) == Some("social_outcome/v1")
			&& outcome.get("social_post_ref").and_then(Value::as_str)
				== Some(crate::path_arg(&context.root, &context.post_path).as_str())
			&& outcome.get("window").and_then(Value::as_str) == Some(&request.window)
		{
			return Err(eyre::eyre!(
				"social outcome already exists for this exact post and window"
			));
		}
	}

	Ok(())
}

fn outcome_metrics(response: &Value) -> Result<Value> {
	let public_metrics = response
		.get("data")
		.and_then(Value::as_object)
		.and_then(|data| data.get("public_metrics"))
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("xurl outcome response is missing public_metrics"))?;
	let mut metrics = serde_json::Map::new();
	for (source, target) in [
		("impression_count", "views"),
		("like_count", "likes"),
		("reply_count", "replies"),
		("retweet_count", "reposts"),
		("bookmark_count", "bookmarks"),
	] {
		if let Some(value) = public_metrics.get(source).and_then(Value::as_u64) {
			metrics.insert(target.into(), Value::from(value));
		}
	}
	if metrics.is_empty() {
		return Err(eyre::eyre!("xurl outcome response has no supported public metrics"));
	}

	Ok(Value::Object(metrics))
}

fn finish_existing(
	request: &SocialObserveXurlRequest,
	context: &ObserveContext,
	outcome: &Value,
) -> Result<SocialObserveXurlReport> {
	if outcome.get("social_post_ref").and_then(Value::as_str)
		!= Some(crate::path_arg(&context.root, &context.post_path).as_str())
		|| outcome.get("published_url").and_then(Value::as_str) != Some(&context.published_url)
		|| outcome.get("window").and_then(Value::as_str) != Some(&request.window)
		|| outcome
			.get("owner")
			.and_then(Value::as_object)
			.and_then(|owner| owner.get("run_id"))
			.and_then(Value::as_str)
			!= Some(&request.run_id)
	{
		return Err(eyre::eyre!("existing social outcome does not match this observation"));
	}
	let response_sha256 = required_object_string(
		outcome
			.get("observation")
			.and_then(Value::as_object)
			.ok_or_else(|| eyre::eyre!("existing outcome observation is required"))?,
		"response_sha256",
	)?;
	let mut attempt = ledger::load_observation_attempt(&context.attempt_path)?;
	validate_attempt(&attempt, request, context)?;
	let last = attempt
		.calls
		.last()
		.ok_or_else(|| eyre::eyre!("xurl observation attempt has no paid call"))?;
	match (attempt.status.as_str(), last.status.as_str()) {
		("observed", "succeeded") if last.response_sha256.as_deref() == Some(response_sha256) => {},
		("read_inflight" | "read_reconcile_inflight", "inflight")
			if last.response_sha256.is_none() =>
		{
			ledger::finish_observation_call(
				&context.attempt_path,
				&mut attempt,
				"succeeded",
				"observed",
				&request.observed_at,
				Some(response_sha256.into()),
			)?;
		},
		_ => return Err(eyre::eyre!("existing outcome has no recoverable usage attempt")),
	}
	report("already_observed", request, context)
}

fn report(
	status: &str,
	request: &SocialObserveXurlRequest,
	context: &ObserveContext,
) -> Result<SocialObserveXurlReport> {
	Ok(SocialObserveXurlReport {
		status: status.into(),
		outcome_path: crate::path_arg(&context.root, &context.outcome_path),
		post_path: crate::path_arg(&context.root, &context.post_path),
		published_url: context.published_url.clone(),
		window: request.window.clone(),
		verified_account: TARGET_ACCOUNT.into(),
		xurl_version: context.xurl_version.clone(),
		observation_recorded_cost_ceiling_microusd: READ_COST_MICROUSD,
		monthly_reserved_cost_ceiling_microusd: ledger::monthly_reserved_cost(
			&context.attempts_dir,
			&context.billing_month,
		)?,
		monthly_budget_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
	})
}

fn required_string<'a>(entry: &'a Value, field: &str) -> Result<&'a str> {
	entry
		.get(field)
		.and_then(Value::as_str)
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("{field} is required"))
}

fn required_object_string<'a>(
	entry: &'a serde_json::Map<String, Value>,
	field: &str,
) -> Result<&'a str> {
	entry
		.get(field)
		.and_then(Value::as_str)
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("{field} is required"))
}

fn required_authorization_contract_digest(attempt: &XurlObservationAttempt) -> Result<String> {
	attempt
		.authorization_contract_sha256
		.as_deref()
		.filter(|digest| lowercase_digest(digest))
		.map(str::to_owned)
		.ok_or_else(|| {
			eyre::eyre!("xurl observation attempt lacks its authorization contract digest")
		})
}

fn lowercase_digest(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
