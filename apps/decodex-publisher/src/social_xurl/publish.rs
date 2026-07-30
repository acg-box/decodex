use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
	auth_contract::APPROVED_XURL_VERSION,
	ledger,
	model::{
		ATTEMPT_SCHEMA, AUTOMATION_ID, CREATE_COST_MICROUSD, IDENTITY_READ_COST_MICROUSD,
		NORMAL_PUBLICATION_COST_MICROUSD, PUBLICATION_LINEAGE_BUDGET_MICROUSD, READ_COST_MICROUSD,
		TARGET_ACCOUNT, VerifiedXurlPost, XURL_APP, XurlAttempt, XurlCall,
	},
	pricing, runtime,
};
use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SOCIAL_MONTHLY_BUDGET_MICROUSD, SOCIAL_POST_SCHEMA,
	SOCIAL_PUBLISH_RESERVATION_SCHEMA, SocialPublishXurlReport, SocialPublishXurlRequest,
	SocialReconcileXurlReport, SocialReconcileXurlRequest,
	prelude::{Result, eyre},
};

struct PublishContext {
	root: PathBuf,
	reservations_dir: PathBuf,
	reservation_path: PathBuf,
	candidate_path: PathBuf,
	candidate_sha256: String,
	posts_dir: PathBuf,
	post_path: PathBuf,
	attempts_dir: PathBuf,
	attempt_path: PathBuf,
	idempotency_key: String,
	publication_lineage_sha256: String,
	billing_month: String,
	xurl_version: String,
	authorization_contract_sha256: String,
}

struct ReadbackExecution<'a> {
	binary: &'a runtime::TrustedXurlBinary,
	text: &'a str,
	posted_at: &'a str,
	context: &'a PublishContext,
}

struct PublicationRecovery<'a> {
	request: &'a SocialReconcileXurlRequest,
	context: &'a PublishContext,
	reservation: &'a Value,
	candidate: &'a Value,
	synthetic_request: &'a SocialPublishXurlRequest,
}

struct PreparedPostRead<'a> {
	post_id: String,
	user_id: String,
	text: &'a str,
	operation: &'static str,
	billing_month: Option<String>,
	reserve_additional: bool,
}

pub(super) fn run(
	request: &SocialPublishXurlRequest,
	xurl_binary: &runtime::TrustedXurlBinary,
) -> Result<SocialPublishXurlReport> {
	run_with_pricing_check(request, xurl_binary, pricing::require_current_at)
}

#[cfg(test)]
pub(super) fn run_without_pricing_for_test(
	request: &SocialPublishXurlRequest,
	xurl_binary: &runtime::TrustedXurlBinary,
) -> Result<SocialPublishXurlReport> {
	run_with_pricing_check(request, xurl_binary, |_| Ok(()))
}

#[cfg(test)]
pub(super) fn run_with_stale_pricing_for_test(
	request: &SocialPublishXurlRequest,
	xurl_binary: &runtime::TrustedXurlBinary,
) -> Result<SocialPublishXurlReport> {
	run_with_pricing_check(request, xurl_binary, |_| {
		Err(eyre::eyre!("X pricing policy is not current: stale"))
	})
}

fn run_with_pricing_check(
	request: &SocialPublishXurlRequest,
	xurl_binary: &runtime::TrustedXurlBinary,
	require_current_pricing: impl FnOnce(OffsetDateTime) -> Result<()>,
) -> Result<SocialPublishXurlReport> {
	let posted_at = validate_request(request)?;
	let root = crate::repo_root()?;
	let reservations_dir = crate::resolve_against(&root, &request.reservations_dir);
	let reservation_path = crate::resolve_against(&root, &request.reservation_path);
	let candidates_dir = crate::resolve_against(&root, &request.candidates_dir);
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let attempts_dir = crate::resolve_against(&root, &request.attempts_dir);
	crate::require_contained_regular_file(&reservation_path, &reservations_dir)
		.map_err(|error| eyre::eyre!("reservation is invalid: {error}"))?;
	let _state_lock = crate::social_publish::scan::acquire_social_state_lock(&request.locks_dir)?;
	let reservation = load_reservation(&reservation_path)?;
	let billing_month = reservation_billing_month(&reservation)?.to_owned();
	let attempt_path = attempts_dir.join(&billing_month).join(format!("{}.json", request.run_id));
	let existing_attempt =
		load_existing_attempt(&attempt_path, &attempts_dir, &root, &reservation_path, request)?;
	let post_path = posts_dir.join(format!("{}.json", request.run_id));
	let existing_post = if post_path.exists() {
		let post = crate::load_json(&post_path)?;
		crate::validate_generated_social_artifact(&post)
			.map_err(|error| eyre::eyre!("existing social post failed validation: {error}"))?;
		Some(post)
	} else {
		None
	};

	let candidate_path =
		reservation_candidate_path(&root, &reservation, &candidates_dir, &request.run_id)?;
	let (candidate, candidate_sha256) = crate::load_json_with_sha256(&candidate_path)?;
	crate::validate_generated_social_artifact(&candidate)
		.map_err(|error| eyre::eyre!("candidate failed validation: {error}"))?;
	crate::social_evidence::validate_internal_evidence_files(&candidate)
		.map_err(|error| eyre::eyre!("candidate evidence failed validation: {error}"))?;
	let publication_time =
		existing_post.as_ref().map(existing_posted_at).transpose()?.unwrap_or(posted_at);
	validate_lineage(&candidate, &reservation, &reservation_path, request, publication_time)?;
	let text = candidate_text(&candidate)?;
	reject_link_like_text(text)?;

	let idempotency_key = required_string(&reservation, "idempotency_key")?.to_owned();
	let publication_lineage_sha256 =
		required_string(&reservation, "publication_lineage_sha256")?.to_owned();
	if let Some(conflict) = super::publication_effect_conflict(
		&attempts_dir,
		&publication_lineage_sha256,
		Some(&attempt_path),
	)? {
		return Err(eyre::eyre!(
			"candidate has a prior uncertain or verified public-write attempt: {}",
			crate::path_arg(&root, &conflict)
		));
	}
	let (authorization_contract_sha256, authorization_contract) = if existing_post.is_some() {
		let attempt = existing_attempt
			.as_ref()
			.ok_or_else(|| eyre::eyre!("existing social post has no publication attempt"))?;
		let digest = required_authorization_contract_digest(attempt)?;
		(digest, None)
	} else {
		require_current_pricing(posted_at)?;
		let contract = super::auth_contract::load_current_at(
			&request.authorization_contract_path,
			posted_at,
			xurl_binary,
		)?;
		let digest = contract.contract_sha256().into();
		(digest, Some(contract))
	};
	let context = PublishContext {
		root,
		reservations_dir,
		reservation_path,
		candidate_path,
		candidate_sha256,
		posts_dir,
		post_path,
		attempts_dir,
		attempt_path,
		idempotency_key,
		publication_lineage_sha256,
		billing_month,
		xurl_version: APPROVED_XURL_VERSION.into(),
		authorization_contract_sha256,
	};
	if let Some(attempt) = &existing_attempt {
		validate_attempt(attempt, request, &context)?;
	}

	if let Some(post) = existing_post {
		return finish_existing(request, &context, &reservation, &candidate, &post);
	}
	crate::social_publish::scan::expire_active_reservations(&context.reservations_dir, posted_at)?;
	let reservation = load_reservation(&context.reservation_path)?;
	if reservation.get("status").and_then(Value::as_str) != Some("active") {
		return Err(eyre::eyre!("reservation is not active"));
	}
	validate_lineage(&candidate, &reservation, &context.reservation_path, request, posted_at)?;
	let mut authorization_contract = authorization_contract
		.ok_or_else(|| eyre::eyre!("xurl authorization contract is unavailable"))?;
	let xurl_version = runtime::verify_ready(xurl_binary, &authorization_contract)?;
	if xurl_version != context.xurl_version {
		return Err(eyre::eyre!("xurl runtime changed after fixed-version validation"));
	}
	let mut attempt = match existing_attempt {
		Some(attempt) => attempt,
		None => create_attempt(request, &context)?,
	};
	validate_attempt(&attempt, request, &context)?;
	let verified = continue_publication(
		xurl_binary,
		&mut authorization_contract,
		text,
		request,
		&context,
		&mut attempt,
	)?;
	finish_new(request, &context, &reservation, &candidate, &mut attempt, &verified)
}

pub(super) fn reconcile_local(
	request: &SocialReconcileXurlRequest,
	reservation_path: &Path,
	reconciled_at: OffsetDateTime,
) -> Result<SocialReconcileXurlReport> {
	let root = crate::repo_root()?;
	let reservations_dir = crate::resolve_against(&root, &request.reservations_dir);
	let candidates_dir = crate::resolve_against(&root, &request.candidates_dir);
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let attempts_dir = crate::resolve_against(&root, &request.attempts_dir);
	crate::require_contained_regular_file(reservation_path, &reservations_dir)
		.map_err(|error| eyre::eyre!("reconciliation reservation is invalid: {error}"))?;
	let reservation = load_reservation(reservation_path)?;
	let original_run_id = reservation_owner_run_id(&reservation)?;
	if request.operation_id == original_run_id {
		return Err(eyre::eyre!(
			"reconciliation operation_id must differ from the original publisher run"
		));
	}
	let candidate_path =
		reservation_candidate_path(&root, &reservation, &candidates_dir, original_run_id)?;
	let (candidate, candidate_sha256) = crate::load_json_with_sha256(&candidate_path)?;
	crate::validate_generated_social_artifact(&candidate)
		.map_err(|error| eyre::eyre!("candidate failed validation: {error}"))?;
	crate::social_evidence::validate_internal_evidence_files(&candidate)
		.map_err(|error| eyre::eyre!("candidate evidence failed validation: {error}"))?;
	let billing_month = reservation_billing_month(&reservation)?.to_owned();
	let attempt_path = attempts_dir.join(&billing_month).join(format!("{original_run_id}.json"));
	crate::require_contained_regular_file(&attempt_path, &attempts_dir)
		.map_err(|error| eyre::eyre!("reconciliation publication attempt is invalid: {error}"))?;
	let mut attempt = ledger::load_attempt(&attempt_path)?;
	ledger::validate_publication_cost_record(&attempt)?;
	if attempt.status == "published"
		&& reservation.get("status").and_then(Value::as_str) != Some("consumed")
	{
		return Err(eyre::eyre!(
			"published xurl attempt has an unconsumed reservation and cannot be reconciled"
		));
	}
	if attempt.candidate_sha256.as_deref() != Some(&candidate_sha256)
		|| attempt.pricing_policy_id.as_deref() != Some(super::model::PRICING_POLICY_ID)
	{
		return Err(eyre::eyre!(
			"xurl publication attempt lacks the current candidate and pricing policy bindings"
		));
	}
	let authorization_contract_sha256 = required_authorization_contract_digest(&attempt)?;
	let attempt_created_at = require_monotonic_recovery_time(&attempt, reconciled_at)?;
	let post_path = posts_dir.join(format!("{original_run_id}.json"));
	let existing_post = load_optional_private_json(&post_path, &posts_dir)?;
	if existing_post.is_none()
		&& (attempt.status == "published"
			|| reservation.get("status").and_then(Value::as_str) == Some("consumed"))
	{
		return Err(eyre::eyre!("terminal publication lineage is missing its durable social post"));
	}
	let publication_time = existing_post
		.as_ref()
		.map(|(post, _)| existing_posted_at(post))
		.transpose()?
		.unwrap_or(attempt_created_at);
	let synthetic_request = SocialPublishXurlRequest {
		reservation_path: reservation_path.to_path_buf(),
		authorization_contract_path: PathBuf::from(crate::DEFAULT_XURL_AUTH_CONTRACT_PATH),
		reservations_dir: reservations_dir.clone(),
		candidates_dir,
		posts_dir: posts_dir.clone(),
		attempts_dir: attempts_dir.clone(),
		locks_dir: crate::resolve_against(&root, &request.locks_dir),
		run_id: original_run_id.into(),
		posted_at: attempt.created_at.clone(),
		monthly_budget_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
	};
	validate_lineage(
		&candidate,
		&reservation,
		reservation_path,
		&synthetic_request,
		publication_time,
	)?;
	let text = candidate_text(&candidate)?;
	reject_link_like_text(text)?;
	let context = PublishContext {
		root: root.clone(),
		reservations_dir,
		reservation_path: reservation_path.to_path_buf(),
		candidate_path,
		candidate_sha256,
		posts_dir,
		post_path: post_path.clone(),
		attempts_dir,
		attempt_path: attempt_path.clone(),
		idempotency_key: required_string(&reservation, "idempotency_key")?.into(),
		publication_lineage_sha256: required_string(&reservation, "publication_lineage_sha256")?
			.into(),
		billing_month,
		xurl_version: attempt.xurl_version.clone(),
		authorization_contract_sha256,
	};
	validate_attempt(&attempt, &synthetic_request, &context)?;
	let changed = finalize_publication_reconciliation(
		request,
		&context,
		&reservation,
		&candidate,
		&synthetic_request,
		existing_post,
		&mut attempt,
	)?;

	Ok(super::reconcile::report(super::reconcile::ReportInput {
		status: if changed { "reconciled" } else { "already_terminal" },
		kind: "publication",
		request,
		original_run_id,
		root: &root,
		artifact_path: &post_path,
		attempt_path: &attempt_path,
		paid_call_count: 0,
	}))
}

pub(super) fn reconcile_safe_read(
	request: &SocialReconcileXurlRequest,
	attempt_path: &Path,
	reconciled_at: OffsetDateTime,
	binary_source: &super::reconcile::BinarySource,
) -> Result<SocialReconcileXurlReport> {
	let root = crate::repo_root()?;
	let reservations_dir = crate::resolve_against(&root, &request.reservations_dir);
	let candidates_dir = crate::resolve_against(&root, &request.candidates_dir);
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let attempts_dir = crate::resolve_against(&root, &request.attempts_dir);
	let mut attempt = load_recovery_attempt(attempt_path, &attempts_dir, &request.operation_id)?;
	let reservation_path = crate::resolve_against(&root, Path::new(&attempt.reservation_ref));
	crate::require_contained_regular_file(&reservation_path, &reservations_dir)
		.map_err(|error| eyre::eyre!("recovery reservation is invalid: {error}"))?;
	let reservation = load_reservation(&reservation_path)?;
	let original_run_id = reservation_owner_run_id(&reservation)?;
	if original_run_id != attempt.run_id {
		return Err(eyre::eyre!("recovery reservation owner does not match its xurl attempt"));
	}
	let candidate_path =
		reservation_candidate_path(&root, &reservation, &candidates_dir, original_run_id)?;
	let (candidate, candidate_sha256) = crate::load_json_with_sha256(&candidate_path)?;
	crate::validate_generated_social_artifact(&candidate)
		.map_err(|error| eyre::eyre!("recovery candidate failed validation: {error}"))?;
	crate::social_evidence::validate_internal_evidence_files(&candidate)
		.map_err(|error| eyre::eyre!("recovery candidate evidence failed validation: {error}"))?;
	let post_path = posts_dir.join(format!("{original_run_id}.json"));
	let synthetic_request = SocialPublishXurlRequest {
		reservation_path: reservation_path.clone(),
		authorization_contract_path: request.authorization_contract_path.clone(),
		reservations_dir: reservations_dir.clone(),
		candidates_dir,
		posts_dir: posts_dir.clone(),
		attempts_dir: attempts_dir.clone(),
		locks_dir: crate::resolve_against(&root, &request.locks_dir),
		run_id: original_run_id.into(),
		posted_at: attempt.created_at.clone(),
		monthly_budget_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
	};
	let context = PublishContext {
		root: root.clone(),
		reservations_dir,
		reservation_path: reservation_path.clone(),
		candidate_path,
		candidate_sha256,
		posts_dir,
		post_path: post_path.clone(),
		attempts_dir,
		attempt_path: attempt_path.to_path_buf(),
		idempotency_key: required_string(&reservation, "idempotency_key")?.into(),
		publication_lineage_sha256: required_string(&reservation, "publication_lineage_sha256")?
			.into(),
		billing_month: attempt.billing_month.clone(),
		xurl_version: APPROVED_XURL_VERSION.into(),
		authorization_contract_sha256: required_authorization_contract_digest(&attempt)?,
	};
	let attempt_created_at = require_monotonic_recovery_time(&attempt, reconciled_at)?;
	validate_lineage(
		&candidate,
		&reservation,
		&reservation_path,
		&synthetic_request,
		attempt_created_at,
	)?;
	validate_attempt(&attempt, &synthetic_request, &context)?;
	if post_path.exists() || matches!(attempt.status.as_str(), "verified" | "published") {
		return reconcile_local(request, &reservation_path, reconciled_at);
	}
	if attempt.status == "identity_reconciled" {
		return finalize_identity_recovery(request, &context, &reservation, &mut attempt, 0);
	}
	let identity_recovery = requires_identity_recovery(&attempt)?;
	require_recovery_reservation_status(&reservation)?;
	if identity_recovery {
		let billing_month = prepare_identity_recovery(request, &context, &attempt)?;
		let binary = binary_source.load()?;
		let mut provenance = verified_recovery_provenance(
			request,
			reconciled_at,
			&binary,
			&context.authorization_contract_sha256,
		)?;
		let report = reconcile_identity_read(
			request,
			&context,
			&reservation,
			&mut attempt,
			&binary,
			&mut provenance,
			&billing_month,
		)?;
		binary.require_command_time_remaining()?;
		Ok(report)
	} else {
		let recovery = PublicationRecovery {
			request,
			context: &context,
			reservation: &reservation,
			candidate: &candidate,
			synthetic_request: &synthetic_request,
		};
		let prepared = prepare_known_post_read(&recovery, &attempt)?;
		require_prepared_post_read_budget(&context, &prepared)?;
		let binary = binary_source.load()?;
		let mut provenance = verified_recovery_provenance(
			request,
			reconciled_at,
			&binary,
			&context.authorization_contract_sha256,
		)?;
		reserve_known_post_read(&recovery, &mut attempt, &prepared)?;
		let report =
			execute_known_post_read(&recovery, prepared, &mut attempt, &binary, &mut provenance)?;
		binary.require_command_time_remaining()?;
		Ok(report)
	}
}

fn load_recovery_attempt(
	attempt_path: &Path,
	attempts_dir: &Path,
	operation_id: &str,
) -> Result<XurlAttempt> {
	let attempt = ledger::load_attempt(attempt_path)?;
	if attempt.schema != ATTEMPT_SCHEMA
		|| !crate::social_publish::valid_run_id(&attempt.run_id)
		|| attempt.xurl_version != APPROVED_XURL_VERSION
		|| operation_id == attempt.run_id
		|| attempt_path
			!= attempts_dir.join(&attempt.billing_month).join(format!("{}.json", attempt.run_id))
	{
		return Err(eyre::eyre!(
			"xurl publication recovery attempt does not match its owner or canonical path"
		));
	}
	Ok(attempt)
}

fn require_monotonic_recovery_time(
	attempt: &XurlAttempt,
	reconciled_at: OffsetDateTime,
) -> Result<OffsetDateTime> {
	let created_at = OffsetDateTime::parse(&attempt.created_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("xurl publication attempt created_at is invalid"))?;
	let updated_at = OffsetDateTime::parse(&attempt.updated_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("xurl publication attempt updated_at is invalid"))?;
	if created_at > updated_at || updated_at > reconciled_at {
		return Err(eyre::eyre!("xurl publication recovery timestamps are not monotonic"));
	}
	Ok(created_at)
}

fn requires_identity_recovery(attempt: &XurlAttempt) -> Result<bool> {
	match attempt.status.as_str() {
		"identity_inflight" | "identity_reconcile_inflight" | "identity_reconcile_halted" =>
			Ok(true),
		"create_inflight" | "create_uncertain" =>
			Err(eyre::eyre!("xurl create outcome is unknown; automated create retry is forbidden")),
		"created"
		| "read_inflight"
		| "read_retry_pending"
		| "read_retry_inflight"
		| "read_reconcile_inflight"
		| "read_reconcile_halted"
		| "halted" => Ok(false),
		status => Err(eyre::eyre!(
			"xurl publication attempt is not eligible for safe read recovery from {status}"
		)),
	}
}

fn require_recovery_reservation_status(reservation: &Value) -> Result<()> {
	if !matches!(reservation.get("status").and_then(Value::as_str), Some("active" | "expired")) {
		return Err(eyre::eyre!(
			"xurl recovery requires an active or expired publication reservation"
		));
	}
	Ok(())
}

fn prepare_identity_recovery(
	request: &SocialReconcileXurlRequest,
	context: &PublishContext,
	attempt: &XurlAttempt,
) -> Result<String> {
	let recovery_count =
		attempt.calls.iter().filter(|call| call.operation == "identity_read_reconcile").count();
	if attempt.post_id.is_some()
		|| attempt.verified_user_id.is_some()
		|| attempt.calls.as_slice().last().is_none_or(|call| {
			!matches!(call.operation.as_str(), "identity_read" | "identity_read_reconcile")
				|| !matches!(call.status.as_str(), "inflight" | "failed" | "invalid")
				|| call.response_sha256.is_some()
		}) || attempt.calls.iter().any(|call| call.operation == "content_create")
		|| recovery_count >= 2
		|| attempt
			.calls
			.iter()
			.any(|call| call.operation_id.as_deref() == Some(&request.operation_id))
	{
		return Err(eyre::eyre!(
			"identity recovery requires one interrupted identity read and no create effect"
		));
	}
	let billing_month = billing_month_at(&request.reconciled_at)?;
	require_recovery_budget(context, &billing_month, IDENTITY_READ_COST_MICROUSD)?;
	Ok(billing_month)
}

fn require_recovery_budget(
	context: &PublishContext,
	billing_month: &str,
	cost_microusd: u64,
) -> Result<()> {
	ledger::ensure_budget(&context.attempts_dir, billing_month, cost_microusd)?;
	ledger::ensure_lineage_budget(
		&context.attempts_dir,
		&context.publication_lineage_sha256,
		cost_microusd,
	)?;
	Ok(())
}

fn verified_recovery_provenance(
	request: &SocialReconcileXurlRequest,
	reconciled_at: OffsetDateTime,
	binary: &runtime::TrustedXurlBinary,
	expected_contract_sha256: &str,
) -> Result<super::auth_contract::VerifiedAuthorizationContract> {
	let provenance = super::auth_contract::load_current_at(
		&request.authorization_contract_path,
		reconciled_at,
		binary,
	)?;
	if provenance.contract_sha256() != expected_contract_sha256 {
		return Err(eyre::eyre!(
			"xurl recovery authorization contract does not match its durable attempt"
		));
	}
	runtime::verify_ready(binary, &provenance)?;
	Ok(provenance)
}

fn reconcile_identity_read(
	request: &SocialReconcileXurlRequest,
	context: &PublishContext,
	reservation: &Value,
	attempt: &mut XurlAttempt,
	binary: &runtime::TrustedXurlBinary,
	provenance: &mut super::auth_contract::VerifiedAuthorizationContract,
	billing_month: &str,
) -> Result<SocialReconcileXurlReport> {
	ledger::reserve_publication_reconcile_call(
		&context.attempt_path,
		attempt,
		&context.attempts_dir,
		recovery_call(
			"identity_read_reconcile",
			IDENTITY_READ_COST_MICROUSD,
			&request.operation_id,
			Some(billing_month),
		),
		"identity_reconcile_inflight",
		&request.reconciled_at,
		true,
	)?;
	let mut output = match runtime::whoami(binary, provenance) {
		Ok(output) => output,
		Err(error) => {
			ledger::finish_last_call(
				&context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status: "failed",
					response_sha256: None,
					status: "identity_reconcile_halted",
					updated_at: &request.reconciled_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			return Err(error);
		},
	};
	let identity = match runtime::parse_identity(&mut output, provenance) {
		Ok(identity) => identity,
		Err(error) => {
			let call_status = if output.status.success() { "invalid" } else { "failed" };
			ledger::finish_last_call(
				&context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status,
					response_sha256: Some(runtime::sha256(&output.stdout)),
					status: "identity_reconcile_halted",
					updated_at: &request.reconciled_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			return Err(error);
		},
	};
	ledger::finish_last_call(
		&context.attempt_path,
		attempt,
		ledger::CallCompletion {
			call_status: "succeeded",
			response_sha256: Some(identity.response_sha256),
			status: "identity_reconciled",
			updated_at: &request.reconciled_at,
			verified_user_id: Some(&identity.user_id),
			post_id: None,
			published_url: None,
		},
	)?;
	finalize_identity_recovery(request, context, reservation, attempt, 1)
}

fn finalize_identity_recovery(
	request: &SocialReconcileXurlRequest,
	context: &PublishContext,
	reservation: &Value,
	attempt: &mut XurlAttempt,
	paid_call_count: u64,
) -> Result<SocialReconcileXurlReport> {
	let reservation_changed =
		release_identity_only_reservation(&context.reservation_path, reservation)?;
	let (_, reservation_sha256) = crate::load_json_with_sha256(&context.reservation_path)?;
	let reservation_ref = crate::path_arg(&context.root, &context.reservation_path);
	let attempt_changed = if let Some(stamp) = &attempt.reconciliation {
		super::reconcile::validate_stamp(
			stamp,
			&attempt.run_id,
			&reservation_ref,
			&reservation_sha256,
		)?;
		false
	} else {
		let stamp = super::reconcile::stamp(
			&request.operation_id,
			&request.reconciled_at,
			reservation_ref,
			reservation_sha256,
		);
		ledger::reconcile_attempt(
			&context.attempt_path,
			attempt,
			"identity_reconciled",
			&request.reconciled_at,
			stamp,
		)?;
		true
	};
	Ok(super::reconcile::report(super::reconcile::ReportInput {
		status: if reservation_changed || attempt_changed {
			"identity_recovered_no_create"
		} else {
			"already_identity_recovered_no_create"
		},
		kind: "identity_read",
		request,
		original_run_id: &attempt.run_id,
		root: &context.root,
		artifact_path: &context.reservation_path,
		attempt_path: &context.attempt_path,
		paid_call_count,
	}))
}

fn release_identity_only_reservation(path: &Path, reservation: &Value) -> Result<bool> {
	if reservation.get("status").and_then(Value::as_str) == Some("expired") {
		return Ok(false);
	}
	if reservation.get("status").and_then(Value::as_str) != Some("active") {
		return Err(eyre::eyre!(
			"identity-only recovery requires an active or expired reservation"
		));
	}
	let mut expired = reservation.clone();
	let object =
		expired.as_object_mut().ok_or_else(|| eyre::eyre!("reservation must be an object"))?;
	object.insert("status".into(), Value::String("expired".into()));
	object.insert(
		"release_reason".into(),
		Value::String("Identity recovery completed without a create call.".into()),
	);
	crate::validate_generated_social_artifact(&expired)?;
	crate::replace_existing_json(path, reservation, &expired)?;
	Ok(true)
}

fn prepare_known_post_read<'a>(
	recovery: &PublicationRecovery<'a>,
	attempt: &XurlAttempt,
) -> Result<PreparedPostRead<'a>> {
	let post_id =
		attempt.post_id.clone().filter(|value| numeric_string(value)).ok_or_else(|| {
			eyre::eyre!("safe publication read recovery requires a known post id")
		})?;
	let user_id =
		attempt.verified_user_id.clone().filter(|value| numeric_string(value)).ok_or_else(
			|| eyre::eyre!("safe publication read recovery requires a verified user id"),
		)?;
	if !attempt
		.calls
		.iter()
		.any(|call| call.operation == "content_create" && call.status == "succeeded")
		|| attempt
			.calls
			.iter()
			.any(|call| call.operation == "content_create" && call.status != "succeeded")
	{
		return Err(eyre::eyre!(
			"safe publication read recovery requires one verified create effect"
		));
	}
	let text = candidate_text(recovery.candidate)?;
	let recovery_count = attempt
		.calls
		.iter()
		.filter(|call| {
			matches!(call.operation.as_str(), "post_read_initial_reconcile" | "post_read_reconcile")
		})
		.count();
	if recovery_count >= 2
		|| attempt
			.calls
			.iter()
			.any(|call| call.operation_id.as_deref() == Some(&recovery.request.operation_id))
	{
		return Err(eyre::eyre!(
			"publication read recovery is exhausted or reuses an operation owner"
		));
	}
	let billing_month = billing_month_at(&recovery.request.reconciled_at)?;
	let uses_original_read_reservation =
		attempt.status == "created" && billing_month == attempt.billing_month;
	let operation = if attempt.status == "created" {
		"post_read_initial_reconcile"
	} else {
		"post_read_reconcile"
	};
	Ok(PreparedPostRead {
		post_id,
		user_id,
		text,
		operation,
		billing_month: (!uses_original_read_reservation).then_some(billing_month),
		reserve_additional: !uses_original_read_reservation,
	})
}

fn require_prepared_post_read_budget(
	context: &PublishContext,
	prepared: &PreparedPostRead<'_>,
) -> Result<()> {
	let (billing_month, additional_microusd) = prepared
		.billing_month
		.as_ref()
		.map_or((&context.billing_month, 0), |billing_month| (billing_month, READ_COST_MICROUSD));
	require_recovery_budget(context, billing_month, additional_microusd)
}

fn reserve_known_post_read(
	recovery: &PublicationRecovery<'_>,
	attempt: &mut XurlAttempt,
	prepared: &PreparedPostRead<'_>,
) -> Result<()> {
	ledger::reserve_publication_reconcile_call(
		&recovery.context.attempt_path,
		attempt,
		&recovery.context.attempts_dir,
		recovery_call(
			prepared.operation,
			READ_COST_MICROUSD,
			&recovery.request.operation_id,
			prepared.billing_month.as_deref(),
		),
		"read_reconcile_inflight",
		&recovery.request.reconciled_at,
		prepared.reserve_additional,
	)
}

fn execute_known_post_read(
	recovery: &PublicationRecovery<'_>,
	prepared: PreparedPostRead<'_>,
	attempt: &mut XurlAttempt,
	binary: &runtime::TrustedXurlBinary,
	provenance: &mut super::auth_contract::VerifiedAuthorizationContract,
) -> Result<SocialReconcileXurlReport> {
	let mut output = match runtime::read(binary, provenance, &prepared.post_id, "post_read") {
		Ok(output) => output,
		Err(error) => {
			ledger::finish_last_call(
				&recovery.context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status: "failed",
					response_sha256: None,
					status: "read_reconcile_halted",
					updated_at: &recovery.request.reconciled_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			return Err(error);
		},
	};
	let (_, digest) = match runtime::parse_read(
		&mut output,
		provenance,
		&prepared.post_id,
		prepared.text,
		&prepared.user_id,
	) {
		Ok(result) => result,
		Err(error) => {
			let call_status = if output.status.success() { "invalid" } else { "failed" };
			ledger::finish_last_call(
				&recovery.context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status,
					response_sha256: Some(runtime::sha256(&output.stdout)),
					status: "read_reconcile_halted",
					updated_at: &recovery.request.reconciled_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			return Err(error);
		},
	};
	let published_url = runtime::canonical_status_url(&prepared.post_id);
	ledger::finish_last_call(
		&recovery.context.attempt_path,
		attempt,
		ledger::CallCompletion {
			call_status: "succeeded",
			response_sha256: Some(digest),
			status: "verified",
			updated_at: &recovery.request.reconciled_at,
			verified_user_id: None,
			post_id: None,
			published_url: Some(&published_url),
		},
	)?;
	finalize_publication_reconciliation(
		recovery.request,
		recovery.context,
		recovery.reservation,
		recovery.candidate,
		recovery.synthetic_request,
		None,
		attempt,
	)?;

	Ok(super::reconcile::report(super::reconcile::ReportInput {
		status: "reconciled",
		kind: "publication_read",
		request: recovery.request,
		original_run_id: &attempt.run_id,
		root: &recovery.context.root,
		artifact_path: &recovery.context.post_path,
		attempt_path: &recovery.context.attempt_path,
		paid_call_count: 1,
	}))
}

fn finalize_publication_reconciliation(
	request: &SocialReconcileXurlRequest,
	context: &PublishContext,
	reservation: &Value,
	candidate: &Value,
	synthetic_request: &SocialPublishXurlRequest,
	existing_post: Option<(Value, String)>,
	attempt: &mut XurlAttempt,
) -> Result<bool> {
	let verified = verified_from_attempt(attempt)?;
	let recovered = attempt.status == "verified";
	let post = if let Some((post, _)) = existing_post {
		validate_existing_post(context, &post, candidate, &verified)?;
		post
	} else {
		if attempt.status != "verified"
			|| !matches!(
				reservation.get("status").and_then(Value::as_str),
				Some("active" | "expired")
			) {
			return Err(eyre::eyre!("publication lineage has no locally recoverable state"));
		}
		let published_count = crate::social_publish::scan::scan_social_publish_state(
			&context.reservations_dir,
			&context.posts_dir,
			required_string(reservation, "idempotency_key")?,
			required_string(reservation, "day")?,
		)?
		.published_count;
		let post = published_post_payload(
			synthetic_request,
			context,
			reservation,
			candidate,
			&verified,
			published_count,
		)?;
		crate::validate_generated_social_artifact(&post)
			.map_err(|error| eyre::eyre!("recovered social post failed validation: {error}"))?;
		crate::write_new_json(&context.post_path, &post)?;
		post
	};
	let post_ref = crate::path_arg(&context.root, &context.post_path);
	let reservation_was_terminal =
		reservation.get("status").and_then(Value::as_str) == Some("consumed");
	consume_reservation(&context.reservation_path, reservation, &post_ref, true)?;
	let (_, post_sha256) = crate::load_json_with_sha256(&context.post_path)?;
	if let Some(stamp) = &attempt.reconciliation {
		super::reconcile::validate_stamp(
			stamp,
			&synthetic_request.run_id,
			&post_ref,
			&post_sha256,
		)?;
	}
	let changed = recovered || !reservation_was_terminal;
	if changed {
		let stamp = super::reconcile::stamp(
			&request.operation_id,
			&request.reconciled_at,
			post_ref,
			post_sha256,
		);
		ledger::reconcile_attempt(
			&context.attempt_path,
			attempt,
			"published",
			&request.reconciled_at,
			stamp,
		)?;
	}
	validate_existing_post(context, &post, candidate, &verified)?;

	Ok(changed)
}

fn validate_request(request: &SocialPublishXurlRequest) -> Result<OffsetDateTime> {
	if !crate::social_publish::valid_run_id(&request.run_id) {
		return Err(eyre::eyre!("run_id must be a lowercase UUID"));
	}
	if request.monthly_budget_microusd != SOCIAL_MONTHLY_BUDGET_MICROUSD {
		return Err(eyre::eyre!(
			"monthly_budget_microusd must be {SOCIAL_MONTHLY_BUDGET_MICROUSD}"
		));
	}
	OffsetDateTime::parse(&request.posted_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("posted_at must be an RFC3339 timestamp"))
}

fn reservation_owner_run_id(reservation: &Value) -> Result<&str> {
	let owner = reservation
		.get("owner")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("reservation owner is required"))?;
	if owner.get("automation_id").and_then(Value::as_str) != Some(AUTOMATION_ID) {
		return Err(eyre::eyre!("reservation owner automation is invalid"));
	}
	let run_id = required_object_string(owner, "run_id")?;
	if !crate::social_publish::valid_run_id(run_id) {
		return Err(eyre::eyre!("reservation owner run_id is invalid"));
	}

	Ok(run_id)
}

fn load_optional_private_json(path: &Path, root: &Path) -> Result<Option<(Value, String)>> {
	match fs::symlink_metadata(path) {
		Ok(_) => {
			crate::require_contained_regular_file(path, root)
				.map_err(|error| eyre::eyre!("reconciliation artifact is invalid: {error}"))?;
			crate::load_json_with_sha256(path).map(Some)
		},
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.into()),
	}
}

fn load_reservation(path: &Path) -> Result<Value> {
	let reservation = crate::load_json(path)?;
	crate::validate_generated_social_artifact(&reservation)
		.map_err(|error| eyre::eyre!("reservation failed validation: {error}"))?;
	if reservation.get("schema").and_then(Value::as_str) != Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA)
	{
		return Err(eyre::eyre!("reservation must use {SOCIAL_PUBLISH_RESERVATION_SCHEMA}"));
	}

	Ok(reservation)
}

fn existing_posted_at(post: &Value) -> Result<OffsetDateTime> {
	if post.get("schema").and_then(Value::as_str) != Some(SOCIAL_POST_SCHEMA)
		|| post.get("status").and_then(Value::as_str) != Some("published")
	{
		return Err(eyre::eyre!("existing social post is not a published record"));
	}
	let posted_at = post
		.get("publication")
		.and_then(Value::as_object)
		.and_then(|publication| publication.get("posted_at"))
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("existing publication posted_at is required"))?;
	OffsetDateTime::parse(posted_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("existing publication posted_at is invalid"))
}

fn reservation_candidate_path(
	root: &Path,
	reservation: &Value,
	candidates_dir: &Path,
	run_id: &str,
) -> Result<PathBuf> {
	let owner = reservation
		.get("owner")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("reservation owner is required"))?;
	if owner.get("automation_id").and_then(Value::as_str) != Some(AUTOMATION_ID)
		|| owner.get("run_id").and_then(Value::as_str) != Some(run_id)
	{
		return Err(eyre::eyre!("reservation owner does not match this publisher run"));
	}
	let refs = reservation
		.get("candidate_refs")
		.and_then(Value::as_object)
		.and_then(|refs| refs.get("social_candidates"))
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("reservation must reference one social candidate"))?;
	if refs.len() != 1 {
		return Err(eyre::eyre!("reservation must reference exactly one social candidate"));
	}
	let candidate_ref = refs[0]
		.as_str()
		.ok_or_else(|| eyre::eyre!("reservation candidate reference must be a string"))?;
	let candidate_path = crate::resolve_against(root, Path::new(candidate_ref));
	crate::require_contained_regular_file(&candidate_path, candidates_dir)
		.map_err(|error| eyre::eyre!("reservation candidate is invalid: {error}"))?;

	Ok(candidate_path)
}

fn validate_lineage(
	candidate: &Value,
	reservation: &Value,
	reservation_path: &Path,
	request: &SocialPublishXurlRequest,
	posted_at: OffsetDateTime,
) -> Result<()> {
	if candidate.get("schema").and_then(Value::as_str) != Some(SOCIAL_CANDIDATE_SCHEMA)
		|| candidate
			.get("decision")
			.and_then(Value::as_object)
			.and_then(|decision| decision.get("worthiness"))
			.and_then(Value::as_str)
			!= Some("publish")
	{
		return Err(eyre::eyre!("candidate is not approved for publication"));
	}
	for field in ["slug", "mode", "target_account"] {
		if candidate.get(field) != reservation.get(field) {
			return Err(eyre::eyre!("candidate and reservation {field} do not match"));
		}
	}
	let decision = candidate["decision"]
		.as_object()
		.ok_or_else(|| eyre::eyre!("candidate decision is required"))?;
	if decision.get("idempotency_key") != reservation.get("idempotency_key") {
		return Err(eyre::eyre!("candidate and reservation idempotency_key do not match"));
	}
	let slug = required_string(candidate, "slug")?;
	let idempotency_key = required_object_string(decision, "idempotency_key")?;
	let publication_lineage_sha256 = crate::social_record::publication_lineage_sha256(candidate)?;
	if reservation.get("publication_lineage_sha256")
		!= Some(&Value::String(publication_lineage_sha256))
	{
		return Err(eyre::eyre!(
			"reservation publication lineage does not match the immutable Radar subject"
		));
	}
	if reservation.get("duplicate_keys") != Some(&json!([slug, idempotency_key])) {
		return Err(eyre::eyre!("reservation duplicate_keys do not match the candidate"));
	}
	let day = required_string(reservation, "day")?;
	let expected_name =
		format!("{}.json", crate::social_publish::idempotency_digest(idempotency_key));
	if reservation_path.file_name().and_then(|value| value.to_str()) != Some(&expected_name)
		|| reservation_path.parent().and_then(Path::file_name).and_then(|value| value.to_str())
			!= Some(day)
	{
		return Err(eyre::eyre!("reservation path does not match its day and idempotency_key"));
	}
	let expires_at = OffsetDateTime::parse(required_string(reservation, "expires_at")?, &Rfc3339)
		.map_err(|_| eyre::eyre!("reservation expires_at is invalid"))?;
	if expires_at <= posted_at {
		return Err(eyre::eyre!("reservation expired before publication"));
	}
	let current_day = format!(
		"{:04}-{:02}-{:02}",
		posted_at.year(),
		u8::from(posted_at.month()),
		posted_at.day()
	);
	if day != current_day || required_string(reservation, "timezone")? != "UTC" {
		return Err(eyre::eyre!("reservation day and timezone must match the current UTC day"));
	}
	if request.run_id
		!= reservation
			.get("owner")
			.and_then(Value::as_object)
			.and_then(|owner| owner.get("run_id"))
			.and_then(Value::as_str)
			.unwrap_or_default()
	{
		return Err(eyre::eyre!("reservation run_id does not match this publisher run"));
	}

	Ok(())
}

fn candidate_text(candidate: &Value) -> Result<&str> {
	let texts = candidate
		.get("candidate_text")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("candidate_text must be an array"))?;
	if texts.len() != 1 {
		return Err(eyre::eyre!("publish candidate_text must contain exactly one item"));
	}
	let text = texts[0]
		.as_str()
		.filter(|text| !text.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("candidate_text item must be a non-empty string"))?;
	if text.chars().count() < 80 {
		return Err(eyre::eyre!(
			"publish candidate_text item must contain at least 80 Unicode characters"
		));
	}

	Ok(text)
}

fn reject_link_like_text(text: &str) -> Result<()> {
	if crate::social_validation::contains_link_like_text(text) {
		return Err(eyre::eyre!(
			"candidate text must not contain URL, domain, email, or other link-like text"
		));
	}

	Ok(())
}

fn reservation_billing_month(reservation: &Value) -> Result<&str> {
	let day = required_string(reservation, "day")?;
	if day.len() != 10 {
		return Err(eyre::eyre!("reservation day is invalid"));
	}

	Ok(&day[..7])
}

fn load_existing_attempt(
	attempt_path: &Path,
	attempts_dir: &Path,
	root: &Path,
	reservation_path: &Path,
	request: &SocialPublishXurlRequest,
) -> Result<Option<XurlAttempt>> {
	match fs::symlink_metadata(attempt_path) {
		Ok(_) => {},
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(error.into()),
	}
	crate::require_contained_regular_file(attempt_path, attempts_dir)
		.map_err(|error| eyre::eyre!("existing publication attempt is invalid: {error}"))?;
	let attempt = ledger::load_attempt(attempt_path)?;
	ledger::validate_publication_cost_record(&attempt)?;
	if attempt.run_id != request.run_id
		|| attempt.reservation_ref != crate::path_arg(root, reservation_path)
	{
		return Err(eyre::eyre!(
			"existing publication attempt does not match its run and reservation"
		));
	}

	Ok(Some(attempt))
}

fn create_attempt(
	request: &SocialPublishXurlRequest,
	context: &PublishContext,
) -> Result<XurlAttempt> {
	ledger::ensure_budget(
		&context.attempts_dir,
		&context.billing_month,
		NORMAL_PUBLICATION_COST_MICROUSD,
	)?;
	ledger::ensure_lineage_budget(
		&context.attempts_dir,
		&context.publication_lineage_sha256,
		NORMAL_PUBLICATION_COST_MICROUSD,
	)?;
	let attempt = XurlAttempt {
		schema: ATTEMPT_SCHEMA.into(),
		run_id: request.run_id.clone(),
		reservation_ref: crate::path_arg(&context.root, &context.reservation_path),
		candidate_ref: crate::path_arg(&context.root, &context.candidate_path),
		candidate_sha256: Some(context.candidate_sha256.clone()),
		idempotency_key: context.idempotency_key.clone(),
		publication_lineage_sha256: context.publication_lineage_sha256.clone(),
		billing_month: context.billing_month.clone(),
		target_account: TARGET_ACCOUNT.into(),
		status: "reserved".into(),
		created_at: request.posted_at.clone(),
		updated_at: request.posted_at.clone(),
		reserved_cost_ceiling_microusd: NORMAL_PUBLICATION_COST_MICROUSD,
		xurl_version: context.xurl_version.clone(),
		pricing_policy_id: Some(super::model::PRICING_POLICY_ID.into()),
		authorization_contract_sha256: Some(context.authorization_contract_sha256.clone()),
		calls: Vec::new(),
		verified_user_id: None,
		post_id: None,
		published_url: None,
		reconciliation: None,
	};
	crate::write_new_json(&context.attempt_path, &serde_json::to_value(&attempt)?)?;

	Ok(attempt)
}

fn validate_attempt(
	attempt: &XurlAttempt,
	request: &SocialPublishXurlRequest,
	context: &PublishContext,
) -> Result<()> {
	if attempt.schema != ATTEMPT_SCHEMA
		|| attempt.run_id != request.run_id
		|| attempt.reservation_ref != crate::path_arg(&context.root, &context.reservation_path)
		|| attempt.candidate_ref != crate::path_arg(&context.root, &context.candidate_path)
		|| attempt.candidate_sha256.as_deref() != Some(&context.candidate_sha256)
		|| attempt.idempotency_key != context.idempotency_key
		|| attempt.publication_lineage_sha256 != context.publication_lineage_sha256
		|| attempt.billing_month != context.billing_month
		|| attempt.target_account != TARGET_ACCOUNT
		|| attempt.reserved_cost_ceiling_microusd
			!= attempt
				.calls
				.iter()
				.try_fold(NORMAL_PUBLICATION_COST_MICROUSD, |total, call| {
					if call.billing_month.is_some() {
						total.checked_add(call.recorded_cost_ceiling_microusd)
					} else {
						Some(total)
					}
				})
				.unwrap_or(u64::MAX)
		|| attempt.reserved_cost_ceiling_microusd > PUBLICATION_LINEAGE_BUDGET_MICROUSD
		|| attempt.xurl_version != APPROVED_XURL_VERSION
		|| attempt.xurl_version != context.xurl_version
		|| attempt.pricing_policy_id.as_deref() != Some(super::model::PRICING_POLICY_ID)
		|| attempt.authorization_contract_sha256.as_deref()
			!= Some(&context.authorization_contract_sha256)
		|| attempt.calls.len() > 5
		|| OffsetDateTime::parse(&attempt.created_at, &Rfc3339).is_err()
		|| OffsetDateTime::parse(&attempt.updated_at, &Rfc3339).is_err()
	{
		return Err(eyre::eyre!("existing xurl attempt does not match this publication"));
	}
	for call in &attempt.calls {
		let expected = match call.operation.as_str() {
			"identity_read" | "identity_read_reconcile" => IDENTITY_READ_COST_MICROUSD,
			"content_create" => CREATE_COST_MICROUSD,
			"post_read_initial"
			| "post_read_initial_reconcile"
			| "post_read_retry"
			| "post_read_reconcile" => READ_COST_MICROUSD,
			_ => return Err(eyre::eyre!("xurl attempt contains an unknown operation")),
		};
		let recovery = call.operation.ends_with("_reconcile");
		if recovery
			!= call.operation_id.as_deref().is_some_and(|operation_id| {
				crate::social_publish::valid_run_id(operation_id) && operation_id != attempt.run_id
			}) {
			return Err(eyre::eyre!("xurl attempt contains an invalid recovery owner"));
		}
		if call.recorded_cost_ceiling_microusd != expected
			|| !call.billing_month.as_deref().is_none_or(ledger::valid_billing_month)
			|| matches!(
				call.operation.as_str(),
				"identity_read" | "content_create" | "post_read_initial"
			) && call.billing_month.is_some()
			|| matches!(
				call.operation.as_str(),
				"identity_read_reconcile" | "post_read_retry" | "post_read_reconcile"
			) && call.billing_month.is_none()
			|| !matches!(
				call.status.as_str(),
				"inflight" | "succeeded" | "failed" | "invalid" | "uncertain"
			) {
			return Err(eyre::eyre!("xurl attempt contains an invalid call"));
		}
	}
	let mut recovery_owners =
		attempt.calls.iter().filter_map(|call| call.operation_id.as_deref()).collect::<Vec<_>>();
	recovery_owners.sort_unstable();
	if recovery_owners.windows(2).any(|window| window[0] == window[1]) {
		return Err(eyre::eyre!("xurl attempt reuses a recovery operation owner"));
	}

	Ok(())
}

fn continue_publication(
	binary: &runtime::TrustedXurlBinary,
	provenance: &mut super::auth_contract::VerifiedAuthorizationContract,
	text: &str,
	request: &SocialPublishXurlRequest,
	context: &PublishContext,
	attempt: &mut XurlAttempt,
) -> Result<VerifiedXurlPost> {
	ensure_identity(binary, provenance, request, context, attempt)?;
	ensure_created(binary, provenance, text, request, context, attempt)?;
	ensure_readback(binary, provenance, text, request, context, attempt)?;
	verified_from_attempt(attempt)
}

fn ensure_identity(
	binary: &runtime::TrustedXurlBinary,
	provenance: &mut super::auth_contract::VerifiedAuthorizationContract,
	request: &SocialPublishXurlRequest,
	context: &PublishContext,
	attempt: &mut XurlAttempt,
) -> Result<()> {
	match attempt.status.as_str() {
		"reserved" => {},
		"identity_verified"
		| "created"
		| "read_inflight"
		| "read_retry_pending"
		| "read_retry_inflight"
		| "verified"
		| "published" => return Ok(()),
		"identity_inflight" => {
			return Err(eyre::eyre!(
				"xurl identity read outcome is unknown; automated paid retry is forbidden"
			));
		},
		"identity_reconcile_inflight" | "identity_reconcile_halted" | "identity_reconciled" => {
			return Err(eyre::eyre!(
				"xurl identity recovery intentionally ended without a create call"
			));
		},
		"create_inflight" | "create_uncertain" => {
			return Err(eyre::eyre!(
				"xurl create outcome is unknown; automated create retry is forbidden"
			));
		},
		status => return Err(eyre::eyre!("xurl attempt is not resumable from {status}")),
	}
	ledger::append_call(
		&context.attempt_path,
		attempt,
		inflight_call("identity_read", IDENTITY_READ_COST_MICROUSD),
		"identity_inflight",
		&request.posted_at,
	)?;
	let mut output = match runtime::whoami(binary, provenance) {
		Ok(output) => output,
		Err(error) => {
			ledger::finish_last_call(
				&context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status: "failed",
					response_sha256: None,
					status: "halted",
					updated_at: &request.posted_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			return Err(error);
		},
	};
	match runtime::parse_identity(&mut output, provenance) {
		Ok(identity) => ledger::finish_last_call(
			&context.attempt_path,
			attempt,
			ledger::CallCompletion {
				call_status: "succeeded",
				response_sha256: Some(identity.response_sha256),
				status: "identity_verified",
				updated_at: &request.posted_at,
				verified_user_id: Some(&identity.user_id),
				post_id: None,
				published_url: None,
			},
		),
		Err(error) => {
			let call_status = if output.status.success() { "invalid" } else { "failed" };
			ledger::finish_last_call(
				&context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status,
					response_sha256: Some(runtime::sha256(&output.stdout)),
					status: "halted",
					updated_at: &request.posted_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			Err(error)
		},
	}
}

fn ensure_created(
	binary: &runtime::TrustedXurlBinary,
	provenance: &mut super::auth_contract::VerifiedAuthorizationContract,
	text: &str,
	request: &SocialPublishXurlRequest,
	context: &PublishContext,
	attempt: &mut XurlAttempt,
) -> Result<()> {
	match attempt.status.as_str() {
		"identity_verified" => {},
		"created"
		| "read_inflight"
		| "read_retry_pending"
		| "read_retry_inflight"
		| "verified"
		| "published" => return Ok(()),
		"create_inflight" | "create_uncertain" => {
			return Err(eyre::eyre!(
				"xurl create outcome is unknown; automated create retry is forbidden"
			));
		},
		status => return Err(eyre::eyre!("xurl attempt is not ready to create from {status}")),
	}
	ledger::append_call(
		&context.attempt_path,
		attempt,
		inflight_call("content_create", CREATE_COST_MICROUSD),
		"create_inflight",
		&request.posted_at,
	)?;
	let mut output = match runtime::create(binary, provenance, text) {
		Ok(output) => output,
		Err(error) => {
			ledger::finish_last_call(
				&context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status: "uncertain",
					response_sha256: None,
					status: "create_uncertain",
					updated_at: &request.posted_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			return Err(error);
		},
	};
	match runtime::parse_create(&mut output, provenance, text) {
		Ok((post_id, digest)) => ledger::finish_last_call(
			&context.attempt_path,
			attempt,
			ledger::CallCompletion {
				call_status: "succeeded",
				response_sha256: Some(digest),
				status: "created",
				updated_at: &request.posted_at,
				verified_user_id: None,
				post_id: Some(&post_id),
				published_url: None,
			},
		),
		Err(error) => {
			ledger::finish_last_call(
				&context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status: "uncertain",
					response_sha256: Some(runtime::sha256(&output.stdout)),
					status: "create_uncertain",
					updated_at: &request.posted_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			Err(error)
		},
	}
}

fn ensure_readback(
	binary: &runtime::TrustedXurlBinary,
	provenance: &mut super::auth_contract::VerifiedAuthorizationContract,
	text: &str,
	request: &SocialPublishXurlRequest,
	context: &PublishContext,
	attempt: &mut XurlAttempt,
) -> Result<()> {
	let execution = ReadbackExecution { binary, text, posted_at: &request.posted_at, context };
	if matches!(attempt.status.as_str(), "verified" | "published") {
		return Ok(());
	}
	if attempt.status == "created" {
		let succeeded = run_read(&execution, provenance, attempt, "post_read_initial", false)?;
		if succeeded {
			return Ok(());
		}
	}
	if attempt.status == "read_inflight" {
		ledger::finish_last_call(
			&context.attempt_path,
			attempt,
			ledger::CallCompletion {
				call_status: "uncertain",
				response_sha256: None,
				status: "read_retry_pending",
				updated_at: &request.posted_at,
				verified_user_id: None,
				post_id: None,
				published_url: None,
			},
		)?;
	}
	if attempt.status == "read_retry_pending" {
		let mut retry = inflight_call("post_read_retry", READ_COST_MICROUSD);
		retry.billing_month = Some(context.billing_month.clone());
		ledger::reserve_retry(
			&context.attempt_path,
			attempt,
			&context.attempts_dir,
			retry,
			&request.posted_at,
		)?;
		if run_read(&execution, provenance, attempt, "post_read_retry", true)? {
			return Ok(());
		}
	}
	if attempt.status == "read_retry_inflight" {
		return Err(eyre::eyre!(
			"xurl read retry outcome is unknown; another paid retry is forbidden"
		));
	}

	Err(eyre::eyre!("xurl post readback did not produce trusted evidence"))
}

fn run_read(
	execution: &ReadbackExecution<'_>,
	provenance: &mut super::auth_contract::VerifiedAuthorizationContract,
	attempt: &mut XurlAttempt,
	operation: &str,
	already_inflight: bool,
) -> Result<bool> {
	if !already_inflight {
		ledger::append_call(
			&execution.context.attempt_path,
			attempt,
			inflight_call(operation, READ_COST_MICROUSD),
			"read_inflight",
			execution.posted_at,
		)?;
	}
	let post_id =
		attempt.post_id.as_deref().ok_or_else(|| eyre::eyre!("xurl attempt has no post id"))?;
	let user_id = attempt
		.verified_user_id
		.as_deref()
		.ok_or_else(|| eyre::eyre!("xurl attempt has no verified user id"))?;
	let mut output = match runtime::read(execution.binary, provenance, post_id, "post_read") {
		Ok(output) => output,
		Err(error) => {
			let next_status =
				if operation == "post_read_initial" { "read_retry_pending" } else { "halted" };
			ledger::finish_last_call(
				&execution.context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status: "failed",
					response_sha256: None,
					status: next_status,
					updated_at: execution.posted_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			if operation == "post_read_initial" {
				return Ok(false);
			}
			return Err(error);
		},
	};
	match runtime::parse_read(&mut output, provenance, post_id, execution.text, user_id) {
		Ok((_, digest)) => {
			let published_url = runtime::canonical_status_url(post_id);
			ledger::finish_last_call(
				&execution.context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status: "succeeded",
					response_sha256: Some(digest),
					status: "verified",
					updated_at: execution.posted_at,
					verified_user_id: None,
					post_id: None,
					published_url: Some(&published_url),
				},
			)?;
			Ok(true)
		},
		Err(_) => {
			let call_status = if output.status.success() { "invalid" } else { "failed" };
			let next_status =
				if operation == "post_read_initial" { "read_retry_pending" } else { "halted" };
			ledger::finish_last_call(
				&execution.context.attempt_path,
				attempt,
				ledger::CallCompletion {
					call_status,
					response_sha256: Some(runtime::sha256(&output.stdout)),
					status: next_status,
					updated_at: execution.posted_at,
					verified_user_id: None,
					post_id: None,
					published_url: None,
				},
			)?;
			Ok(false)
		},
	}
}

fn inflight_call(operation: &str, cost: u64) -> XurlCall {
	XurlCall {
		operation: operation.into(),
		operation_id: None,
		billing_month: None,
		status: "inflight".into(),
		recorded_cost_ceiling_microusd: cost,
		response_sha256: None,
	}
}

fn recovery_call(
	operation: &str,
	cost: u64,
	operation_id: &str,
	billing_month: Option<&str>,
) -> XurlCall {
	let mut call = inflight_call(operation, cost);
	call.operation_id = Some(operation_id.into());
	call.billing_month = billing_month.map(str::to_owned);
	call
}

fn verified_from_attempt(attempt: &XurlAttempt) -> Result<VerifiedXurlPost> {
	if attempt.status != "verified" && attempt.status != "published" {
		return Err(eyre::eyre!("xurl attempt is not verified"));
	}
	validate_verified_call_sequence(attempt)?;
	let post_id =
		attempt.post_id.clone().filter(|value| numeric_string(value)).ok_or_else(|| {
			eyre::eyre!("verified xurl publication attempt has an invalid post id")
		})?;
	let verified_user_id =
		attempt.verified_user_id.as_deref().filter(|value| numeric_string(value)).ok_or_else(
			|| eyre::eyre!("verified xurl publication attempt has an invalid user id"),
		)?;
	let published_url =
		attempt.published_url.clone().ok_or_else(|| eyre::eyre!("published URL is missing"))?;
	if published_url != runtime::canonical_status_url(&post_id) || verified_user_id.is_empty() {
		return Err(eyre::eyre!(
			"verified xurl publication attempt has inconsistent public identity"
		));
	}
	Ok(VerifiedXurlPost {
		post_id,
		published_url,
		identity_response_sha256: call_digest(attempt, "identity_read")?,
		create_response_sha256: call_digest(attempt, "content_create")?,
		read_response_sha256: attempt
			.calls
			.iter()
			.rev()
			.find(|call| call.operation.starts_with("post_read") && call.status == "succeeded")
			.and_then(|call| call.response_sha256.clone())
			.ok_or_else(|| eyre::eyre!("read response digest is missing"))?,
		recorded_cost_ceiling_microusd: attempt.reserved_cost_ceiling_microusd,
	})
}

fn validate_verified_call_sequence(attempt: &XurlAttempt) -> Result<()> {
	let create_index = attempt
		.calls
		.iter()
		.position(|call| call.operation == "content_create")
		.ok_or_else(|| eyre::eyre!("verified xurl attempt has no create call"))?;
	if attempt.calls.iter().filter(|call| call.operation == "content_create").count() != 1
		|| attempt.calls[create_index].status != "succeeded"
	{
		return Err(eyre::eyre!("verified xurl attempt has an invalid create sequence"));
	}
	let identity_calls = &attempt.calls[..create_index];
	let read_calls = &attempt.calls[create_index + 1..];
	if identity_calls.is_empty()
		|| identity_calls.len() > 2
		|| identity_calls.last().is_none_or(|call| {
			!matches!(call.operation.as_str(), "identity_read" | "identity_read_reconcile")
				|| call.status != "succeeded"
		}) || identity_calls[..identity_calls.len() - 1].iter().any(|call| {
		call.operation != "identity_read"
			|| !matches!(call.status.as_str(), "failed" | "invalid" | "uncertain")
	}) || read_calls.is_empty()
		|| read_calls.len() > 3
		|| read_calls.last().is_none_or(|call| {
			!matches!(
				call.operation.as_str(),
				"post_read_initial"
					| "post_read_initial_reconcile"
					| "post_read_retry"
					| "post_read_reconcile"
			) || call.status != "succeeded"
		}) || read_calls[..read_calls.len() - 1].iter().any(|call| {
		!matches!(
			call.operation.as_str(),
			"post_read_initial"
				| "post_read_initial_reconcile"
				| "post_read_retry"
				| "post_read_reconcile"
		) || !matches!(call.status.as_str(), "failed" | "invalid" | "uncertain")
	}) {
		return Err(eyre::eyre!("verified xurl attempt has an invalid paid-call sequence"));
	}
	let reserved = attempt
		.calls
		.iter()
		.try_fold(NORMAL_PUBLICATION_COST_MICROUSD, |total, call| {
			if call.billing_month.is_some() {
				total.checked_add(call.recorded_cost_ceiling_microusd)
			} else {
				Some(total)
			}
		})
		.ok_or_else(|| eyre::eyre!("verified xurl attempt cost arithmetic overflowed"))?;
	if reserved != attempt.reserved_cost_ceiling_microusd {
		return Err(eyre::eyre!("verified xurl attempt cost bindings are inconsistent"));
	}
	for call in &attempt.calls {
		if call.status == "succeeded"
			&& !call.response_sha256.as_deref().is_some_and(lowercase_digest)
		{
			return Err(eyre::eyre!("verified xurl attempt is missing a response digest"));
		}
	}

	Ok(())
}

fn numeric_string(value: &str) -> bool {
	!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn lowercase_digest(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn billing_month_at(value: &str) -> Result<String> {
	let timestamp = OffsetDateTime::parse(value, &Rfc3339)
		.map_err(|_| eyre::eyre!("xurl recovery billing timestamp is invalid"))?;
	Ok(format!("{:04}-{:02}", timestamp.year(), u8::from(timestamp.month())))
}

fn required_authorization_contract_digest(attempt: &XurlAttempt) -> Result<String> {
	attempt
		.authorization_contract_sha256
		.as_deref()
		.filter(|digest| lowercase_digest(digest))
		.map(str::to_owned)
		.ok_or_else(|| {
			eyre::eyre!("xurl publication attempt lacks its authorization contract digest")
		})
}

fn call_digest(attempt: &XurlAttempt, operation: &str) -> Result<String> {
	attempt
		.calls
		.iter()
		.find(|call| {
			(call.operation == operation
				|| operation == "identity_read" && call.operation == "identity_read_reconcile")
				&& call.status == "succeeded"
		})
		.and_then(|call| call.response_sha256.clone())
		.ok_or_else(|| eyre::eyre!("{operation} response digest is missing"))
}

fn finish_new(
	request: &SocialPublishXurlRequest,
	context: &PublishContext,
	reservation: &Value,
	candidate: &Value,
	attempt: &mut XurlAttempt,
	verified: &VerifiedXurlPost,
) -> Result<SocialPublishXurlReport> {
	let published_count = crate::social_publish::scan::scan_social_publish_state(
		&context.reservations_dir,
		&context.posts_dir,
		required_string(reservation, "idempotency_key")?,
		required_string(reservation, "day")?,
	)?
	.published_count;
	let post = published_post_payload(
		request,
		context,
		reservation,
		candidate,
		verified,
		published_count,
	)?;
	crate::validate_generated_social_artifact(&post)
		.map_err(|error| eyre::eyre!("generated published post failed validation: {error}"))?;
	crate::write_new_json(&context.post_path, &post)?;
	let post_ref = crate::path_arg(&context.root, &context.post_path);
	consume_reservation(&context.reservation_path, reservation, &post_ref, false)?;
	ledger::update_attempt(&context.attempt_path, attempt, "published", &request.posted_at)?;
	report("published", context, reservation, verified)
}

fn finish_existing(
	request: &SocialPublishXurlRequest,
	context: &PublishContext,
	reservation: &Value,
	candidate: &Value,
	post: &Value,
) -> Result<SocialPublishXurlReport> {
	let verified = verified_post_from_record(post)?;
	validate_existing_post(context, post, candidate, &verified)?;
	let mut attempt = ledger::load_attempt(&context.attempt_path)?;
	validate_attempt(&attempt, request, context)?;
	if !matches!(attempt.status.as_str(), "verified" | "published") {
		return Err(eyre::eyre!("existing publication has no verified xurl publication attempt"));
	}
	let post_ref = crate::path_arg(&context.root, &context.post_path);
	consume_reservation(&context.reservation_path, reservation, &post_ref, true)?;
	if attempt.status == "verified" {
		ledger::update_attempt(
			&context.attempt_path,
			&mut attempt,
			"published",
			&request.posted_at,
		)?;
	}
	report("already_published", context, reservation, &verified)
}

fn published_post_payload(
	request: &SocialPublishXurlRequest,
	context: &PublishContext,
	reservation: &Value,
	candidate: &Value,
	verified: &VerifiedXurlPost,
	published_count: usize,
) -> Result<Value> {
	let decision = candidate["decision"]
		.as_object()
		.ok_or_else(|| eyre::eyre!("candidate decision is required"))?;
	let mut payload = json!({
		"schema": SOCIAL_POST_SCHEMA,
		"slug": required_string(candidate, "slug")?,
		"channel": "x",
		"target_account": TARGET_ACCOUNT,
		"owner": {
			"automation_id": AUTOMATION_ID,
			"run_id": request.run_id,
		},
		"mode": required_string(candidate, "mode")?,
		"status": "published",
		"audience": required_string(candidate, "audience")?,
		"text": candidate.get("candidate_text").cloned().ok_or_else(|| eyre::eyre!("candidate_text is required"))?,
		"source_refs": crate::social_evidence::source_refs_with_lineage(
			candidate,
			crate::path_arg(&context.root, &context.candidate_path),
			Some(crate::path_arg(&context.root, &context.reservation_path)),
		)?,
		"evidence_digests": crate::social_evidence::evidence_digests_value(candidate),
		"evidence_notes": candidate.get("evidence_notes").cloned().ok_or_else(|| eyre::eyre!("evidence_notes are required"))?,
		"claims": candidate.get("claims").cloned().ok_or_else(|| eyre::eyre!("claims are required"))?,
		"decision": {
			"worthiness": "publish",
			"priority": required_string(candidate, "priority")?,
			"idempotency_key": required_string(reservation, "idempotency_key")?,
			"reason": decision.get("reason").cloned().ok_or_else(|| eyre::eyre!("candidate decision reason is required"))?,
			"daily_limit": 1,
			"daily_count_before": published_count,
			"daily_count_after": published_count + 1,
			"day": required_string(reservation, "day")?,
			"timezone": required_string(reservation, "timezone")?,
		},
		"publication": {
			"posted_at": request.posted_at,
			"published_urls": [verified.published_url],
			"post_id": verified.post_id,
			"publisher": "xurl",
			"xurl_version": context.xurl_version,
			"xurl_app": XURL_APP,
			"verified_account": TARGET_ACCOUNT,
			"verified_user_id": verified_user_id_from_context(context)?,
			"account_verified": true,
			"made_with_ai": true,
			"identity_response_sha256": verified.identity_response_sha256,
			"create_response_sha256": verified.create_response_sha256,
				"read_response_sha256": verified.read_response_sha256,
				"publication_lineage_sha256": context.publication_lineage_sha256,
				"recorded_cost_ceiling_microusd": verified.recorded_cost_ceiling_microusd,
		},
	});
	if let Some(value) = candidate.get("caveats") {
		payload["caveats"] = value.clone();
	}

	Ok(payload)
}

fn verified_user_id_from_context(context: &PublishContext) -> Result<String> {
	ledger::load_attempt(&context.attempt_path)?
		.verified_user_id
		.ok_or_else(|| eyre::eyre!("verified user id is missing"))
}

fn verified_post_from_record(post: &Value) -> Result<VerifiedXurlPost> {
	let publication = post
		.get("publication")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("existing publication is required"))?;
	let post_id = required_object_string(publication, "post_id")?.to_owned();
	let published_url = publication
		.get("published_urls")
		.and_then(Value::as_array)
		.and_then(|urls| urls.first())
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("existing published URL is required"))?
		.to_owned();
	if published_url != runtime::canonical_status_url(&post_id) {
		return Err(eyre::eyre!("existing published URL does not match its post id"));
	}
	Ok(VerifiedXurlPost {
		post_id,
		published_url,
		identity_response_sha256: required_object_string(publication, "identity_response_sha256")?
			.into(),
		create_response_sha256: required_object_string(publication, "create_response_sha256")?
			.into(),
		read_response_sha256: required_object_string(publication, "read_response_sha256")?.into(),
		recorded_cost_ceiling_microusd: publication
			.get("recorded_cost_ceiling_microusd")
			.and_then(Value::as_u64)
			.ok_or_else(|| eyre::eyre!("publication cost ceiling is missing"))?,
	})
}

fn validate_existing_post(
	context: &PublishContext,
	post: &Value,
	candidate: &Value,
	verified: &VerifiedXurlPost,
) -> Result<()> {
	crate::validate_generated_social_artifact(post)
		.map_err(|error| eyre::eyre!("existing social post failed validation: {error}"))?;
	if post.get("status").and_then(Value::as_str) != Some("published")
		|| post.get("slug") != candidate.get("slug")
		|| post.get("text") != candidate.get("candidate_text")
		|| post.get("source_refs")
			!= Some(&crate::social_evidence::source_refs_with_lineage(
				candidate,
				crate::path_arg(&context.root, &context.candidate_path),
				Some(crate::path_arg(&context.root, &context.reservation_path)),
			)?) || post.get("evidence_digests")
		!= Some(&crate::social_evidence::evidence_digests_value(candidate))
	{
		return Err(eyre::eyre!("existing social post does not match its source artifacts"));
	}
	let publication = post["publication"]
		.as_object()
		.ok_or_else(|| eyre::eyre!("existing publication is required"))?;
	if post
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("idempotency_key"))
		.and_then(Value::as_str)
		!= Some(&context.idempotency_key)
		|| publication.get("post_id").and_then(Value::as_str) != Some(&verified.post_id)
		|| publication.get("publication_lineage_sha256").and_then(Value::as_str)
			!= Some(&context.publication_lineage_sha256)
		|| publication.get("xurl_version").and_then(Value::as_str) != Some(&context.xurl_version)
	{
		return Err(eyre::eyre!("existing publication does not match this request"));
	}

	Ok(())
}

fn consume_reservation(
	path: &Path,
	reservation: &Value,
	post_ref: &str,
	allow_expired_recovery: bool,
) -> Result<()> {
	match reservation.get("status").and_then(Value::as_str) {
		Some("consumed")
			if reservation.get("consumed_by_social_post").and_then(Value::as_str)
				== Some(post_ref) =>
		{
			return Ok(());
		},
		Some("consumed") => {
			return Err(eyre::eyre!("consumed reservation references a different post"));
		},
		Some("active") => {},
		Some("expired") if allow_expired_recovery => {},
		_ => return Err(eyre::eyre!("reservation is not active or consumed")),
	}
	let mut consumed = reservation.clone();
	let object =
		consumed.as_object_mut().ok_or_else(|| eyre::eyre!("reservation must be an object"))?;
	object.insert("status".into(), Value::String("consumed".into()));
	object.insert("consumed_by_social_post".into(), Value::String(post_ref.into()));
	object.remove("release_reason");
	crate::validate_generated_social_artifact(&consumed)?;
	crate::replace_existing_json(path, reservation, &consumed)
}

fn report(
	status: &str,
	context: &PublishContext,
	reservation: &Value,
	verified: &VerifiedXurlPost,
) -> Result<SocialPublishXurlReport> {
	Ok(SocialPublishXurlReport {
		status: status.into(),
		post_path: crate::path_arg(&context.root, &context.post_path),
		reservation_path: crate::path_arg(&context.root, &context.reservation_path),
		candidate_path: crate::path_arg(&context.root, &context.candidate_path),
		attempt_path: crate::path_arg(&context.root, &context.attempt_path),
		idempotency_key: required_string(reservation, "idempotency_key")?.into(),
		published_url: verified.published_url.clone(),
		post_id: verified.post_id.clone(),
		verified_account: TARGET_ACCOUNT.into(),
		xurl_version: context.xurl_version.clone(),
		publication_recorded_cost_ceiling_microusd: verified.recorded_cost_ceiling_microusd,
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
