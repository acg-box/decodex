use serde::{Deserialize, Serialize};

pub(crate) const ATTEMPT_SCHEMA: &str = "decodex/xurl-publish-attempt/4";
pub(crate) const OBSERVATION_ATTEMPT_SCHEMA: &str = "decodex/xurl-observation-attempt/4";
pub(crate) const AUTOMATION_ID: &str = "decodex-xurl-publisher";
pub(crate) const TARGET_ACCOUNT: &str = "decodexspace";
pub(crate) const XURL_APP: &str = "default";
pub(crate) const PRICING_POLICY_ID: &str = "x-api-pay-per-usage/2026-07-27";
pub(crate) const IDENTITY_READ_COST_MICROUSD: u64 = 10_000;
pub(crate) const CREATE_COST_MICROUSD: u64 = 15_000;
pub(crate) const READ_COST_MICROUSD: u64 = 5_000;
pub(crate) const NORMAL_PUBLICATION_COST_MICROUSD: u64 =
	IDENTITY_READ_COST_MICROUSD + CREATE_COST_MICROUSD + READ_COST_MICROUSD;
pub(crate) const PUBLICATION_LINEAGE_BUDGET_MICROUSD: u64 = 40_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XurlCall {
	pub(crate) operation: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) operation_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) billing_month: Option<String>,
	pub(crate) status: String,
	pub(crate) recorded_cost_ceiling_microusd: u64,
	pub(crate) response_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XurlAttempt {
	pub(crate) schema: String,
	pub(crate) run_id: String,
	pub(crate) reservation_ref: String,
	pub(crate) candidate_ref: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) candidate_sha256: Option<String>,
	pub(crate) idempotency_key: String,
	pub(crate) publication_lineage_sha256: String,
	pub(crate) billing_month: String,
	pub(crate) target_account: String,
	pub(crate) status: String,
	pub(crate) created_at: String,
	pub(crate) updated_at: String,
	pub(crate) reserved_cost_ceiling_microusd: u64,
	pub(crate) xurl_version: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) pricing_policy_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) authorization_contract_sha256: Option<String>,
	pub(crate) calls: Vec<XurlCall>,
	pub(crate) verified_user_id: Option<String>,
	pub(crate) post_id: Option<String>,
	pub(crate) published_url: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) reconciliation: Option<XurlReconciliation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XurlObservationAttempt {
	pub(crate) schema: String,
	pub(crate) run_id: String,
	pub(crate) billing_month: String,
	pub(crate) reserved_cost_ceiling_microusd: u64,
	pub(crate) status: String,
	pub(crate) post_ref: String,
	pub(crate) post_id: String,
	pub(crate) publication_lineage_sha256: String,
	pub(crate) window: String,
	pub(crate) created_at: String,
	pub(crate) updated_at: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) pricing_policy_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) authorization_contract_sha256: Option<String>,
	pub(crate) call: XurlCall,
	pub(crate) calls: Vec<XurlCall>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) reconciliation: Option<XurlReconciliation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XurlReconciliation {
	pub(crate) operation_id: String,
	pub(crate) reconciled_at: String,
	pub(crate) evidence_ref: String,
	pub(crate) evidence_sha256: String,
}

#[derive(Debug)]
pub(super) struct VerifiedIdentity {
	pub(super) user_id: String,
	pub(super) response_sha256: String,
}

#[derive(Debug)]
pub(super) struct VerifiedXurlPost {
	pub(super) post_id: String,
	pub(super) published_url: String,
	pub(super) identity_response_sha256: String,
	pub(super) create_response_sha256: String,
	pub(super) read_response_sha256: String,
	pub(super) recorded_cost_ceiling_microusd: u64,
}
