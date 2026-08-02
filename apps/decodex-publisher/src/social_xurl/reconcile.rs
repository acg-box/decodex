use std::path::Path;
#[cfg(test)] use std::path::PathBuf;

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
	ledger,
	model::{
		ATTEMPT_SCHEMA, OBSERVATION_ATTEMPT_SCHEMA, XurlAttempt, XurlObservationAttempt,
		XurlReconciliation,
	},
	observe, publish,
	runtime::{self, TrustedXurlBinary},
};
use crate::{
	SocialReconcileXurlReport, SocialReconcileXurlRequest,
	prelude::{Result, eyre},
};

pub(super) fn run(request: &SocialReconcileXurlRequest) -> Result<SocialReconcileXurlReport> {
	run_inner(request, &BinarySource::Production, true)
}

#[cfg(test)]
pub(super) fn run_without_pricing_for_test(
	request: &SocialReconcileXurlRequest,
	xurl_binary: &Path,
) -> Result<SocialReconcileXurlReport> {
	run_inner(request, &BinarySource::Test(xurl_binary.to_path_buf()), false)
}

fn run_inner(
	request: &SocialReconcileXurlRequest,
	binary_source: &BinarySource,
	require_pricing: bool,
) -> Result<SocialReconcileXurlReport> {
	let reconciled_at = validate_request(request)?;
	let root = crate::repo_root()?;
	let _state_lock = crate::social_publish::scan::acquire_social_state_lock(&request.locks_dir)?;
	if let Some(attempt_path) = &request.attempt_path {
		let attempt_path = crate::resolve_against(&root, attempt_path);
		let attempts_dir = crate::resolve_against(&root, &request.attempts_dir);
		crate::require_contained_regular_file(&attempt_path, &attempts_dir)
			.map_err(|error| eyre::eyre!("reconciliation attempt is invalid: {error}"))?;
		let payload = crate::load_json(&attempt_path)?;
		let schema = payload.get("schema").and_then(serde_json::Value::as_str);
		match schema {
			Some(ATTEMPT_SCHEMA) => {
				let attempt: XurlAttempt = serde_json::from_value(payload.clone())
					.map_err(|_| eyre::eyre!("xurl publication recovery attempt is invalid"))?;
				ledger::validate_publication_cost_record(&attempt)?;
			},
			Some(OBSERVATION_ATTEMPT_SCHEMA) => {
				let attempt: XurlObservationAttempt = serde_json::from_value(payload.clone())
					.map_err(|_| eyre::eyre!("xurl observation recovery attempt is invalid"))?;
				ledger::validate_observation_cost_record(&attempt)?;
			},
			_ => return Err(eyre::eyre!("reconciliation attempt uses an unsupported schema")),
		}
		return match schema {
			Some(ATTEMPT_SCHEMA) => publish::reconcile_safe_read(
				request,
				&attempt_path,
				reconciled_at,
				binary_source,
				require_pricing,
			),
			Some(OBSERVATION_ATTEMPT_SCHEMA) => observe::reconcile_safe_read(
				request,
				&attempt_path,
				reconciled_at,
				binary_source,
				require_pricing,
			),
			_ => unreachable!("attempt schema was validated before xurl readiness"),
		};
	}

	let evidence_path = crate::resolve_against(&root, &request.evidence_path);
	let reservations_dir = crate::resolve_against(&root, &request.reservations_dir);
	let outcomes_dir = crate::resolve_against(&root, &request.outcomes_dir);

	if evidence_path.starts_with(&reservations_dir) {
		return publish::reconcile_local(request, &evidence_path, reconciled_at);
	}
	if evidence_path.starts_with(&outcomes_dir) {
		return observe::reconcile_local(request, &evidence_path, reconciled_at);
	}

	Err(eyre::eyre!("reconciliation evidence must be a configured reservation or outcome"))
}

pub(super) enum BinarySource {
	Production,
	#[cfg(test)]
	Test(PathBuf),
}

impl BinarySource {
	pub(super) fn load(&self) -> Result<TrustedXurlBinary> {
		match self {
			Self::Production => runtime::trusted_xurl_binary(),
			#[cfg(test)]
			Self::Test(path) => TrustedXurlBinary::open_for_test(path),
		}
	}
}

pub(super) fn stamp(
	operation_id: &str,
	reconciled_at: &str,
	evidence_ref: String,
	evidence_sha256: String,
) -> XurlReconciliation {
	XurlReconciliation {
		operation_id: operation_id.into(),
		reconciled_at: reconciled_at.into(),
		evidence_ref,
		evidence_sha256,
	}
}

pub(super) fn validate_stamp(
	stamp: &XurlReconciliation,
	original_run_id: &str,
	evidence_ref: &str,
	evidence_sha256: &str,
) -> Result<()> {
	if !crate::social_publish::valid_run_id(&stamp.operation_id)
		|| stamp.operation_id == original_run_id
		|| OffsetDateTime::parse(&stamp.reconciled_at, &Rfc3339).is_err()
		|| stamp.evidence_ref != evidence_ref
		|| stamp.evidence_sha256 != evidence_sha256
	{
		return Err(eyre::eyre!("xurl reconciliation stamp does not match durable evidence"));
	}

	Ok(())
}

pub(super) struct ReportInput<'a> {
	pub(super) status: &'a str,
	pub(super) kind: &'a str,
	pub(super) request: &'a SocialReconcileXurlRequest,
	pub(super) original_run_id: &'a str,
	pub(super) root: &'a Path,
	pub(super) artifact_path: &'a Path,
	pub(super) attempt_path: &'a Path,
	pub(super) paid_call_count: u64,
}

pub(super) fn report(input: ReportInput<'_>) -> SocialReconcileXurlReport {
	SocialReconcileXurlReport {
		status: input.status.into(),
		kind: input.kind.into(),
		operation_id: input.request.operation_id.clone(),
		original_run_id: input.original_run_id.into(),
		artifact_path: crate::path_arg(input.root, input.artifact_path),
		attempt_path: crate::path_arg(input.root, input.attempt_path),
		paid_call_count: input.paid_call_count,
	}
}

fn validate_request(request: &SocialReconcileXurlRequest) -> Result<OffsetDateTime> {
	if !crate::social_publish::valid_run_id(&request.operation_id) {
		return Err(eyre::eyre!("operation_id must be a lowercase UUID"));
	}
	let has_evidence = !request.evidence_path.as_os_str().is_empty();
	if has_evidence == request.attempt_path.is_some() {
		return Err(eyre::eyre!(
			"reconciliation requires exactly one local evidence path or interrupted attempt path"
		));
	}
	OffsetDateTime::parse(&request.reconciled_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("reconciled_at must be an RFC3339 timestamp"))
}
