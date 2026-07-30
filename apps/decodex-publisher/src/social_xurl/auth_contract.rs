use std::path::Path;

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
	model::{TARGET_ACCOUNT, XURL_APP},
	runtime::TrustedXurlBinary,
};
use crate::{
	SocialSealXurlAuthReport, SocialSealXurlAuthRequest, XurlAuthorizationContractReport,
	prelude::{Result, eyre},
};

pub(crate) const APPROVED_XURL_VERSION: &str = "1.3.1";
pub(super) const APPROVED_XURL_SHA256: &str =
	"7b85a210009db7a3f2d6183684674441fbf81276f1101f73d36d0266ec9aa01e";

const CONTRACT_SCHEMA: &str = "decodex/xurl-authorization-contract/1";
const POLICY_ID: &str = "xurl-oauth-least-privilege/3";
const MAX_CONTRACT_BYTES: u64 = 16 * 1024;
const REQUIRED_SCOPES: [&str; 4] = ["tweet.read", "users.read", "tweet.write", "offline.access"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct XurlAuthorizationContract {
	schema: String,
	policy_id: String,
	target_account: String,
	xurl_app: String,
	required_operator_authorized_scopes: Vec<String>,
	xurl_version: String,
	xurl_binary_sha256: String,
	sealed_at: String,
}

#[derive(Debug)]
pub(super) struct VerifiedAuthorizationContract {
	contract_sha256: String,
	report: XurlAuthorizationContractReport,
}

pub(super) fn seal(
	request: &SocialSealXurlAuthRequest,
	binary: &TrustedXurlBinary,
) -> Result<SocialSealXurlAuthReport> {
	let sealed_at = parse_time(&request.sealed_at, "sealed_at")?;
	binary.require_approved_release()?;
	let xurl_version = super::runtime::verify_runtime(binary)?;
	super::runtime::verify_auth_status(binary)?;

	let root = crate::repo_root()?;
	let contract_path = crate::resolve_against(&root, &request.receipt_path);
	let contract = XurlAuthorizationContract {
		schema: CONTRACT_SCHEMA.into(),
		policy_id: POLICY_ID.into(),
		target_account: TARGET_ACCOUNT.into(),
		xurl_app: XURL_APP.into(),
		required_operator_authorized_scopes: required_scopes(),
		xurl_version,
		xurl_binary_sha256: APPROVED_XURL_SHA256.into(),
		sealed_at: request.sealed_at.clone(),
	};
	validate_contract(&contract, sealed_at, binary)?;
	crate::write_new_json(&contract_path, &serde_json::to_value(&contract)?)?;

	Ok(SocialSealXurlAuthReport {
		status: "sealed".into(),
		receipt_path: crate::path_arg(&root, &contract_path),
		policy_id: POLICY_ID.into(),
		xurl_version: APPROVED_XURL_VERSION.into(),
		xurl_binary_sha256: APPROVED_XURL_SHA256.into(),
		xurl_app: XURL_APP.into(),
		account_label: TARGET_ACCOUNT.into(),
		required_operator_authorized_scopes: required_scopes(),
	})
}

pub(super) fn load_current_at(
	path: &Path,
	now: OffsetDateTime,
	binary: &TrustedXurlBinary,
) -> Result<VerifiedAuthorizationContract> {
	binary.require_approved_release()?;
	let root = crate::repo_root()?;
	let contract_path = crate::resolve_against(&root, path);
	let (payload, contract_sha256) =
		crate::load_json_with_sha256_bounded(&contract_path, MAX_CONTRACT_BYTES)
			.map_err(|error| eyre::eyre!("xurl authorization contract is unavailable: {error}"))?;
	let contract: XurlAuthorizationContract = serde_json::from_value(payload)
		.map_err(|_| eyre::eyre!("xurl authorization contract is invalid"))?;
	validate_contract(&contract, now, binary)?;

	Ok(VerifiedAuthorizationContract {
		contract_sha256,
		report: XurlAuthorizationContractReport {
			policy_id: POLICY_ID.into(),
			status: "current".into(),
			target_account: TARGET_ACCOUNT.into(),
			xurl_app: XURL_APP.into(),
			required_operator_authorized_scopes: required_scopes(),
			xurl_version: APPROVED_XURL_VERSION.into(),
			xurl_binary_sha256: APPROVED_XURL_SHA256.into(),
			sealed_at: contract.sealed_at,
		},
	})
}

impl VerifiedAuthorizationContract {
	pub(super) fn require_runtime(&self, binary: &TrustedXurlBinary) -> Result<()> {
		binary.require_approved_release()
	}

	pub(super) fn contract_sha256(&self) -> &str {
		&self.contract_sha256
	}

	pub(super) fn report(&self) -> XurlAuthorizationContractReport {
		self.report.clone()
	}
}

fn validate_contract(
	contract: &XurlAuthorizationContract,
	now: OffsetDateTime,
	binary: &TrustedXurlBinary,
) -> Result<()> {
	let sealed_at = parse_time(&contract.sealed_at, "sealed_at")?;
	if contract.schema != CONTRACT_SCHEMA
		|| contract.policy_id != POLICY_ID
		|| contract.target_account != TARGET_ACCOUNT
		|| contract.xurl_app != XURL_APP
		|| contract.required_operator_authorized_scopes != required_scopes()
		|| contract.xurl_version != APPROVED_XURL_VERSION
		|| contract.xurl_binary_sha256 != APPROVED_XURL_SHA256
		|| sealed_at > now
	{
		return Err(eyre::eyre!(
			"xurl authorization contract does not match the approved fixed authority"
		));
	}
	binary.require_approved_release()
}

fn required_scopes() -> Vec<String> {
	REQUIRED_SCOPES.iter().map(|scope| (*scope).into()).collect()
}

fn parse_time(value: &str, field: &str) -> Result<OffsetDateTime> {
	OffsetDateTime::parse(value, &Rfc3339)
		.map_err(|_| eyre::eyre!("xurl authorization contract {field} is invalid"))
}
