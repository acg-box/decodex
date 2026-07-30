//! Bounded X publication through the official xurl CLI.

pub(crate) mod auth_contract;
pub(crate) mod ledger;
pub(crate) mod model;
mod observe;
mod pricing;
mod publish;
mod reconcile;
mod runtime;

use std::path::{Path, PathBuf};

use crate::{
	SocialObserveXurlReport, SocialObserveXurlRequest, SocialProbeXurlReport,
	SocialPublishXurlReport, SocialPublishXurlRequest, SocialReconcileXurlReport,
	SocialReconcileXurlRequest, SocialSealXurlAuthReport, SocialSealXurlAuthRequest,
	SocialXurlCostReport, prelude::Result,
};

pub(crate) fn seal_auth(request: &SocialSealXurlAuthRequest) -> Result<SocialSealXurlAuthReport> {
	let binary = runtime::trusted_xurl_binary()?;
	let report = auth_contract::seal(request, &binary)?;
	binary.require_command_time_remaining()?;
	Ok(report)
}

pub(crate) fn publish(request: &SocialPublishXurlRequest) -> Result<SocialPublishXurlReport> {
	let binary = runtime::trusted_xurl_binary()?;
	let report = publish::run(request, &binary)?;
	binary.require_command_time_remaining()?;
	Ok(report)
}

pub(crate) fn observe(request: &SocialObserveXurlRequest) -> Result<SocialObserveXurlReport> {
	let binary = runtime::trusted_xurl_binary()?;
	let report = observe::run(request, &binary)?;
	binary.require_command_time_remaining()?;
	Ok(report)
}

pub(crate) fn probe(now: &str) -> Result<SocialProbeXurlReport> {
	let now = parse_probe_time(now)?;
	let binary = runtime::trusted_xurl_binary()?;
	let contract = auth_contract::load_current_at(
		Path::new(crate::DEFAULT_XURL_AUTH_CONTRACT_PATH),
		now,
		&binary,
	)?;
	let report = probe_with_verified(now, &binary, &contract)?;
	binary.require_command_time_remaining()?;
	Ok(report)
}

pub(crate) fn cost_report(billing_month: &str) -> Result<SocialXurlCostReport> {
	let root = crate::repo_root()?;
	ledger::cost_report(
		&crate::resolve_against(&root, Path::new(crate::DEFAULT_SOCIAL_ATTEMPTS_DIR)),
		billing_month,
	)
}

#[cfg(test)]
pub(crate) fn cost_report_for_test(
	attempts_dir: &Path,
	billing_month: &str,
) -> Result<SocialXurlCostReport> {
	ledger::cost_report(attempts_dir, billing_month)
}

pub(crate) fn reconcile(request: &SocialReconcileXurlRequest) -> Result<SocialReconcileXurlReport> {
	reconcile::run(request)
}

#[cfg(test)]
pub(crate) fn reconcile_with_test_binary(
	request: &SocialReconcileXurlRequest,
	xurl_binary: &Path,
) -> Result<SocialReconcileXurlReport> {
	reconcile::run_with_test_binary(request, xurl_binary)
}

#[cfg(test)]
pub(crate) fn reconcile_attempt_with_test_binary_without_pricing(
	request: &SocialReconcileXurlRequest,
	xurl_binary: &Path,
) -> Result<SocialReconcileXurlReport> {
	reconcile::run_attempt_with_test_binary_without_pricing(request, xurl_binary)
}

pub(crate) fn publication_effect_conflict(
	attempts_dir: &Path,
	publication_lineage_sha256: &str,
	excluded_attempt_path: Option<&Path>,
) -> Result<Option<PathBuf>> {
	ledger::publication_effect_conflict(
		attempts_dir,
		publication_lineage_sha256,
		excluded_attempt_path,
	)
}

pub(crate) fn daily_publication_effect_conflict(
	attempts_dir: &Path,
	day: &str,
) -> Result<Option<PathBuf>> {
	ledger::daily_publication_effect_conflict(attempts_dir, day)
}

#[cfg(test)]
pub(crate) fn publish_with_test_binary(
	request: &SocialPublishXurlRequest,
	xurl_binary: &Path,
) -> Result<SocialPublishXurlReport> {
	let binary = runtime::TrustedXurlBinary::open_for_test(xurl_binary)?;
	publish::run_without_pricing_for_test(request, &binary)
}

#[cfg(test)]
pub(crate) fn publish_with_test_binary_and_stale_pricing(
	request: &SocialPublishXurlRequest,
	xurl_binary: &Path,
) -> Result<SocialPublishXurlReport> {
	let binary = runtime::TrustedXurlBinary::open_for_test(xurl_binary)?;
	publish::run_with_stale_pricing_for_test(request, &binary)
}

#[cfg(test)]
pub(crate) fn observe_with_test_binary(
	request: &SocialObserveXurlRequest,
	xurl_binary: &Path,
) -> Result<SocialObserveXurlReport> {
	let binary = runtime::TrustedXurlBinary::open_for_test(xurl_binary)?;
	observe::run_without_pricing_for_test(request, &binary)
}

#[cfg(test)]
pub(crate) fn probe_with_test_binary(
	now: &str,
	xurl_binary: &Path,
	auth_contract_path: &Path,
) -> Result<SocialProbeXurlReport> {
	let now = parse_probe_time(now)?;
	let binary = runtime::TrustedXurlBinary::open_for_test(xurl_binary)?;
	let contract = auth_contract::load_current_at(auth_contract_path, now, &binary)?;
	probe_with_verified_and_pricing(now, &binary, &contract, test_pricing_policy)
}

#[cfg(test)]
pub(crate) fn probe_with_test_binary_after_bind(
	now: &str,
	xurl_binary: &Path,
	auth_contract_path: &Path,
	after_bind: impl FnOnce(),
) -> Result<SocialProbeXurlReport> {
	let now = parse_probe_time(now)?;
	let binary = runtime::TrustedXurlBinary::open_for_test(xurl_binary)?;
	let contract = auth_contract::load_current_at(auth_contract_path, now, &binary)?;
	after_bind();
	probe_with_verified_and_pricing(now, &binary, &contract, test_pricing_policy)
}

#[cfg(test)]
pub(crate) fn seal_auth_with_test_binary(
	request: &SocialSealXurlAuthRequest,
	xurl_binary: &Path,
) -> Result<SocialSealXurlAuthReport> {
	let binary = runtime::TrustedXurlBinary::open_for_test(xurl_binary)?;
	auth_contract::seal(request, &binary)
}

fn probe_with_verified(
	now: time::OffsetDateTime,
	binary: &runtime::TrustedXurlBinary,
	contract: &auth_contract::VerifiedAuthorizationContract,
) -> Result<SocialProbeXurlReport> {
	probe_with_verified_and_pricing(now, binary, contract, pricing::report_at)
}

fn probe_with_verified_and_pricing(
	now: time::OffsetDateTime,
	binary: &runtime::TrustedXurlBinary,
	contract: &auth_contract::VerifiedAuthorizationContract,
	pricing_policy_at: impl FnOnce(time::OffsetDateTime) -> Result<crate::XPricingPolicyReport>,
) -> Result<SocialProbeXurlReport> {
	let xurl_version = runtime::verify_ready(binary, contract)?;
	let pricing_policy = pricing_policy_at(now)?;
	let ready = pricing_policy.status == "current";

	Ok(SocialProbeXurlReport {
		status: if ready { "ready".into() } else { "blocked".into() },
		ready,
		xurl_version,
		xurl_app: model::XURL_APP.into(),
		account_label: model::TARGET_ACCOUNT.into(),
		authorization_contract: contract.report(),
		pricing_policy,
	})
}

#[cfg(test)]
fn test_pricing_policy(now: time::OffsetDateTime) -> Result<crate::XPricingPolicyReport> {
	let expires_at = parse_probe_time("2026-07-28T12:00:00Z")?;
	Ok(crate::XPricingPolicyReport {
		policy_id: model::PRICING_POLICY_ID.into(),
		official_source: "https://docs.x.com/x-api/getting-started/pricing.md".into(),
		reviewed_at: "2026-07-27T00:00:00Z".into(),
		effective_at: "2026-07-27T00:00:00Z".into(),
		expires_at: "2026-07-28T12:00:00Z".into(),
		status: if now <= expires_at { "current".into() } else { "stale".into() },
		user_read_cost_microusd: model::IDENTITY_READ_COST_MICROUSD,
		url_free_content_create_cost_microusd: model::CREATE_COST_MICROUSD,
		post_read_cost_ceiling_microusd: model::READ_COST_MICROUSD,
		monthly_reservation_cap_microusd: crate::SOCIAL_MONTHLY_BUDGET_MICROUSD,
	})
}

fn parse_probe_time(now: &str) -> Result<time::OffsetDateTime> {
	time::OffsetDateTime::parse(now, &time::format_description::well_known::Rfc3339)
		.map_err(|_| crate::prelude::eyre::eyre!("probe time must be an RFC3339 timestamp"))
}
