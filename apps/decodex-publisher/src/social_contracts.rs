use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug)]
pub(crate) struct SocialReservePublishRequest {
	pub(crate) candidate_path: PathBuf,
	pub(crate) candidates_dir: PathBuf,
	pub(crate) reserved_at: String,
	pub(crate) expires_at: String,
	pub(crate) day: String,
	pub(crate) timezone: String,
	pub(crate) out_dir: PathBuf,
	pub(crate) posts_dir: PathBuf,
	pub(crate) attempts_dir: PathBuf,
	pub(crate) locks_dir: PathBuf,
	pub(crate) run_id: String,
	pub(crate) daily_limit: usize,
	pub(crate) dry_run: bool,
}

#[derive(Debug)]
pub(crate) struct SocialPublishXurlRequest {
	pub(crate) reservation_path: PathBuf,
	pub(crate) authorization_contract_path: PathBuf,
	pub(crate) reservations_dir: PathBuf,
	pub(crate) candidates_dir: PathBuf,
	pub(crate) posts_dir: PathBuf,
	pub(crate) attempts_dir: PathBuf,
	pub(crate) locks_dir: PathBuf,
	pub(crate) run_id: String,
	pub(crate) posted_at: String,
	pub(crate) monthly_budget_microusd: u64,
}

#[derive(Debug)]
pub(crate) struct SocialObserveXurlRequest {
	pub(crate) run_id: String,
	pub(crate) post_path: PathBuf,
	pub(crate) authorization_contract_path: PathBuf,
	pub(crate) posts_dir: PathBuf,
	pub(crate) outcomes_dir: PathBuf,
	pub(crate) attempts_dir: PathBuf,
	pub(crate) locks_dir: PathBuf,
	pub(crate) observed_at: String,
	pub(crate) window: String,
	pub(crate) monthly_budget_microusd: u64,
}

#[derive(Debug)]
pub(crate) struct SocialSealXurlAuthRequest {
	pub(crate) receipt_path: PathBuf,
	pub(crate) sealed_at: String,
}

#[derive(Debug)]
pub(crate) struct SocialReconcileXurlRequest {
	pub(crate) evidence_path: PathBuf,
	pub(crate) attempt_path: Option<PathBuf>,
	pub(crate) authorization_contract_path: PathBuf,
	pub(crate) reservations_dir: PathBuf,
	pub(crate) candidates_dir: PathBuf,
	pub(crate) posts_dir: PathBuf,
	pub(crate) outcomes_dir: PathBuf,
	pub(crate) attempts_dir: PathBuf,
	pub(crate) locks_dir: PathBuf,
	pub(crate) operation_id: String,
	pub(crate) reconciled_at: String,
}

#[derive(Debug)]
pub(crate) struct SocialTerminalizeSkipRequest {
	pub(crate) candidate_path: PathBuf,
	pub(crate) candidates_dir: PathBuf,
	pub(crate) reservations_dir: PathBuf,
	pub(crate) posts_dir: PathBuf,
	pub(crate) locks_dir: PathBuf,
	pub(crate) run_id: String,
	pub(crate) day: String,
	pub(crate) timezone: String,
	pub(crate) daily_limit: usize,
	pub(crate) dry_run: bool,
	pub(crate) reason: Option<String>,
}

#[derive(Debug)]
pub(crate) struct SocialPublishNextRequest {
	pub(crate) run_id: String,
	pub(crate) decision: String,
	pub(crate) reason: Option<String>,
	pub(crate) clock: crate::SocialClock,
}

#[derive(Debug)]
pub(crate) struct SocialObserveDueRequest {
	pub(crate) run_id: String,
	pub(crate) observed_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SocialReservePublishReport {
	pub(crate) status: String,
	pub(crate) path: String,
	pub(crate) idempotency_key: String,
	pub(crate) daily_limit: usize,
	pub(crate) published_count: usize,
	pub(crate) active_reservation_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct SocialPublishXurlReport {
	pub(crate) status: String,
	pub(crate) post_path: String,
	pub(crate) reservation_path: String,
	pub(crate) candidate_path: String,
	pub(crate) attempt_path: String,
	pub(crate) idempotency_key: String,
	pub(crate) published_url: String,
	pub(crate) post_id: String,
	pub(crate) verified_account: String,
	pub(crate) xurl_version: String,
	pub(crate) publication_recorded_cost_ceiling_microusd: u64,
	pub(crate) monthly_reserved_cost_ceiling_microusd: u64,
	pub(crate) monthly_budget_microusd: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct SocialObserveXurlReport {
	pub(crate) status: String,
	pub(crate) outcome_path: String,
	pub(crate) post_path: String,
	pub(crate) published_url: String,
	pub(crate) window: String,
	pub(crate) verified_account: String,
	pub(crate) xurl_version: String,
	pub(crate) observation_recorded_cost_ceiling_microusd: u64,
	pub(crate) monthly_reserved_cost_ceiling_microusd: u64,
	pub(crate) monthly_budget_microusd: u64,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SocialProbeXurlReport {
	pub(crate) status: String,
	pub(crate) ready: bool,
	pub(crate) xurl_version: String,
	pub(crate) xurl_app: String,
	pub(crate) account_label: String,
	pub(crate) authorization_contract: XurlAuthorizationContractReport,
	pub(crate) pricing_policy: XPricingPolicyReport,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SocialRefreshPricingReport {
	pub(crate) status: String,
	pub(crate) official_source: String,
	pub(crate) fetched_at: Option<String>,
	pub(crate) receipt_status: String,
	pub(crate) rates_microusd: Option<XPricingRatesReport>,
	pub(crate) error_code: Option<String>,
	pub(crate) ordinary_https_get_count: u64,
	pub(crate) x_api_call_count: u64,
	pub(crate) x_api_cost_microusd: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct XurlAuthorizationContractReport {
	pub(crate) policy_id: String,
	pub(crate) status: String,
	pub(crate) target_account: String,
	pub(crate) xurl_app: String,
	pub(crate) required_operator_authorized_scopes: Vec<String>,
	pub(crate) xurl_version: String,
	pub(crate) xurl_binary_sha256: String,
	pub(crate) sealed_at: String,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SocialSealXurlAuthReport {
	pub(crate) status: String,
	pub(crate) receipt_path: String,
	pub(crate) policy_id: String,
	pub(crate) xurl_version: String,
	pub(crate) xurl_binary_sha256: String,
	pub(crate) xurl_app: String,
	pub(crate) account_label: String,
	pub(crate) required_operator_authorized_scopes: Vec<String>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SocialXurlCostReport {
	pub(crate) status: String,
	pub(crate) billing_month: String,
	pub(crate) used_cost_ceiling_microusd: u64,
	pub(crate) reserved_cost_ceiling_microusd: u64,
	pub(crate) monthly_cap_microusd: u64,
	pub(crate) remaining_cost_ceiling_microusd: u64,
	pub(crate) publication_attempt_count: u64,
	pub(crate) observation_attempt_count: u64,
	pub(crate) identity_read_call_count: u64,
	pub(crate) content_create_call_count: u64,
	pub(crate) post_read_call_count: u64,
	pub(crate) total_call_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct XPricingPolicyReport {
	pub(crate) policy_id: String,
	pub(crate) official_source: String,
	pub(crate) reviewed_at: String,
	pub(crate) effective_at: String,
	pub(crate) expires_at: String,
	pub(crate) status: String,
	pub(crate) user_read_cost_microusd: u64,
	pub(crate) url_free_content_create_cost_microusd: u64,
	pub(crate) post_read_cost_ceiling_microusd: u64,
	pub(crate) monthly_reservation_cap_microusd: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct XPricingRatesReport {
	pub(crate) post_create: u64,
	pub(crate) post_create_with_url: u64,
	pub(crate) post_read: u64,
	pub(crate) user_read: u64,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SocialReconcileXurlReport {
	pub(crate) status: String,
	pub(crate) kind: String,
	pub(crate) operation_id: String,
	pub(crate) original_run_id: String,
	pub(crate) artifact_path: String,
	pub(crate) attempt_path: String,
	pub(crate) paid_call_count: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct SocialTerminalizeSkipReport {
	pub(crate) status: String,
	pub(crate) path: String,
	pub(crate) candidate: String,
	pub(crate) idempotency_key: String,
	pub(crate) published_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct SocialPublishNextReport {
	pub(crate) status: String,
	pub(crate) candidate_path: Option<String>,
	pub(crate) effect_path: Option<String>,
	pub(crate) published_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SocialObserveDueReport {
	pub(crate) status: String,
	pub(crate) post_path: Option<String>,
	pub(crate) outcome_path: Option<String>,
	pub(crate) window: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SocialValidationReport {
	pub(crate) checked_files: usize,
	pub(crate) errors: Vec<String>,
}
