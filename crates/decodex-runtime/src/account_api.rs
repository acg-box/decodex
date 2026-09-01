//! One authenticated OpenAI/Codex backend API client for account observation.
#![cfg_attr(all(feature = "process-acceptance-fixture", debug_assertions), allow(dead_code))]

use std::{sync::Arc, time::Duration};

use decodex_codex::{
	AccountApiProfile, AccountApiProtocolError, AccountApiQuotaWindow, AccountApiResetCredit,
	AccountApiResetCredits, AccountApiUsage, decode_account_api_profile,
	decode_account_api_reset_credits, decode_account_api_usage,
};
use decodex_core::{AccountId, AccountOperationId, AccountProvider, ProviderIdentity};
use reqwest::{Method, StatusCode};

use crate::account_service::{AccountApiCredential, AccountLifecycleError, AccountService};

const BACKEND_API_BASE: &str = "https://chatgpt.com/backend-api";
const USAGE_PATH: &str = "/wham/usage";
const PROFILE_PATH: &str = "/wham/profiles/me";
const RESET_CREDITS_PATH: &str = "/wham/rate-limit-reset-credits";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MINIMUM_ACCESS_TOKEN_VALIDITY: Duration = Duration::from_secs(20);
const RESET_CREDIT_DETAIL_RETRY_DELAY: Duration = Duration::from_millis(250);

/// Closed provider failure safe for UI and durable operation mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountApiRuntimeError {
	AccountUnavailable,
	CredentialUnavailable,
	Unauthorized,
	ProviderUnavailable,
	ProtocolUnavailable,
	AccountChanged,
}

/// One provider API inventory after optional reset-credit detail enrichment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountApiInventory {
	pub(crate) account_revision: i64,
	pub(crate) quota_windows: [AccountApiQuotaWindow; 2],
	pub(crate) reported_available_count: Option<u64>,
	pub(crate) details_complete: bool,
	pub(crate) credits: Vec<AccountApiResetCredit>,
}

/// Result of one coalesced provider refresh round.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountApiObservation {
	pub(crate) account_revision: i64,
	pub(crate) provider: Option<ProviderIdentity>,
	pub(crate) inventory: Result<AccountApiInventory, AccountApiRuntimeError>,
	pub(crate) profile: Result<AccountApiProfile, AccountApiRuntimeError>,
}

/// The singleton backend API adapter shared by background observation and reset-card effects.
#[derive(Clone)]
pub(crate) struct AccountApiRuntime {
	accounts: Arc<AccountService>,
	client: reqwest::Client,
}
impl AccountApiRuntime {
	/// Build one bounded client.  No Codex executable or protocol receipt is consulted.
	pub(crate) fn new(accounts: Arc<AccountService>) -> Result<Self, AccountApiRuntimeError> {
		let client = reqwest::Client::builder()
			.connect_timeout(CONNECT_TIMEOUT)
			.timeout(HTTP_TIMEOUT)
			.redirect(reqwest::redirect::Policy::none())
			.user_agent("decodexd")
			.build()
			.map_err(|_| AccountApiRuntimeError::ProviderUnavailable)?;
		Ok(Self { accounts, client })
	}

	/// Observe profile, usage, and reset-credit details through one shared auth/request boundary.
	pub(crate) async fn observe_account(&self, account_id: &AccountId) -> AccountApiObservation {
		let first = self.observe_with_current_credential(account_id).await;
		if !first.requires_auth_retry() {
			return first.into_observation();
		}

		let first_revision = first.account_revision();
		if self.refresh_after_unauthorized(account_id, first_revision).await.is_err() {
			return first.into_observation();
		}
		self.observe_with_current_credential(account_id).await.into_observation()
	}

	async fn observe_with_current_credential(
		&self,
		account_id: &AccountId,
	) -> PendingAccountApiObservation {
		let credential = match self
			.accounts
			.api_credential_for_observation(account_id, MINIMUM_ACCESS_TOKEN_VALIDITY)
			.await
		{
			Ok(credential) => credential,
			Err(error) => {
				let error = map_account_service_error(error);
				return PendingAccountApiObservation::credential_error(error);
			},
		};
		if credential.binding.provider.provider() != AccountProvider::Chatgpt {
			return PendingAccountApiObservation::failed(
				credential.account_revision,
				AccountApiRuntimeError::ProtocolUnavailable,
			);
		}

		let usage = self.request_json(Method::GET, USAGE_PATH, &credential, None);
		let profile = self.request_json(Method::GET, PROFILE_PATH, &credential, None);
		let (usage, profile) = tokio::join!(usage, profile);
		let usage = usage
			.map_err(map_request_error)
			.and_then(|body| decode_account_api_usage(&body).map_err(map_protocol_error));
		let profile = profile
			.map_err(map_request_error)
			.and_then(|body| decode_account_api_profile(&body).map_err(map_protocol_error));

		let inventory = match usage {
			Ok(usage) => self.enrich_inventory(&credential, usage).await,
			Err(error) => Err(error),
		};
		PendingAccountApiObservation::Pending(Box::new(AccountApiObservation {
			account_revision: credential.account_revision,
			provider: Some(credential.binding.provider.clone()),
			inventory,
			profile,
		}))
	}

	async fn enrich_inventory(
		&self,
		credential: &AccountApiCredential,
		usage: AccountApiUsage,
	) -> Result<AccountApiInventory, AccountApiRuntimeError> {
		let Some(reported_available_count) = usage.reported_available_count else {
			return Ok(AccountApiInventory {
				account_revision: credential.account_revision,
				quota_windows: usage.quota_windows,
				reported_available_count: None,
				details_complete: false,
				credits: Vec::new(),
			});
		};
		if reported_available_count == 0 {
			return Ok(AccountApiInventory {
				account_revision: credential.account_revision,
				quota_windows: usage.quota_windows,
				reported_available_count: Some(0),
				details_complete: true,
				credits: Vec::new(),
			});
		}
		let mut details = self.request_reset_credit_details(credential).await;
		if should_retry_reset_credit_details(reported_available_count, &details) {
			// The summary and detail endpoints are independent provider projections. One bounded
			// successor read absorbs their common short convergence window without delaying any
			// other account's observation owner.
			tokio::time::sleep(RESET_CREDIT_DETAIL_RETRY_DELAY).await;
			details = self.request_reset_credit_details(credential).await;
		}
		match details {
			Ok(details)
				if reset_credit_details_are_complete(reported_available_count, &details) =>
				Ok(AccountApiInventory {
					account_revision: credential.account_revision,
					quota_windows: usage.quota_windows,
					reported_available_count: Some(reported_available_count),
					details_complete: true,
					credits: details.credits,
				}),
			Err(AccountApiRuntimeError::Unauthorized) => Err(AccountApiRuntimeError::Unauthorized),
			Ok(_) | Err(_) => Ok(AccountApiInventory {
				// Preserve the fresh quota projection even when the optional details do not
				// converge. The observation cache retains a same-revision complete public
				// inventory, if one exists, and a later daemon round retries this bounded
				// provider read.
				account_revision: credential.account_revision,
				quota_windows: usage.quota_windows,
				reported_available_count: Some(reported_available_count),
				details_complete: false,
				credits: Vec::new(),
			}),
		}
	}

	async fn request_reset_credit_details(
		&self,
		credential: &AccountApiCredential,
	) -> Result<AccountApiResetCredits, AccountApiRuntimeError> {
		match self.request_json(Method::GET, RESET_CREDITS_PATH, credential, None).await {
			Ok(body) => decode_account_api_reset_credits(&body).map_err(map_protocol_error),
			Err(error) => Err(map_request_error(error)),
		}
	}

	async fn refresh_after_unauthorized(
		&self,
		account_id: &AccountId,
		account_revision: i64,
	) -> Result<(), AccountApiRuntimeError> {
		let operation_id =
			AccountOperationId::generate().map_err(|_| AccountApiRuntimeError::AccountChanged)?;
		self.accounts
			.refresh(operation_id, account_id, Some(account_revision), None, None)
			.await
			.map(|_| ())
			.map_err(map_account_service_error)
	}

	async fn request_json(
		&self,
		method: Method,
		path: &str,
		credential: &AccountApiCredential,
		json: Option<&serde_json::Value>,
	) -> Result<Vec<u8>, AccountApiRequestError> {
		let mut request = self
			.client
			.request(method, format!("{BACKEND_API_BASE}{path}"))
			.bearer_auth(credential.stored.bundle().access_token())
			.header("ChatGPT-Account-Id", credential.binding.provider.account_id())
			.header("Accept", "application/json")
			.header("Cache-Control", "no-cache, no-store");
		if let Some(json) = json {
			request = request.json(json);
		}
		let response =
			request.send().await.map_err(|_| AccountApiRequestError::ProviderUnavailable)?;
		let status = response.status();
		if status == StatusCode::UNAUTHORIZED {
			return Err(AccountApiRequestError::Unauthorized);
		}
		if !status.is_success() {
			return Err(AccountApiRequestError::ProviderUnavailable);
		}
		if response
			.content_length()
			.is_some_and(|length| length > decodex_codex::MAX_ACCOUNT_API_BODY_BYTES as u64)
		{
			return Err(AccountApiRequestError::ProtocolUnavailable);
		}
		let body =
			response.bytes().await.map_err(|_| AccountApiRequestError::ProviderUnavailable)?;
		if body.len() > decodex_codex::MAX_ACCOUNT_API_BODY_BYTES {
			return Err(AccountApiRequestError::ProtocolUnavailable);
		}
		Ok(body.to_vec())
	}
}

enum PendingAccountApiObservation {
	Pending(Box<AccountApiObservation>),
	CredentialError { error: AccountApiRuntimeError },
}
impl PendingAccountApiObservation {
	fn credential_error(error: AccountApiRuntimeError) -> Self {
		Self::CredentialError { error }
	}

	fn failed(account_revision: i64, error: AccountApiRuntimeError) -> Self {
		Self::Pending(Box::new(AccountApiObservation {
			account_revision,
			provider: None,
			inventory: Err(error),
			profile: Err(error),
		}))
	}

	fn account_revision(&self) -> i64 {
		match self {
			Self::Pending(observation) => observation.account_revision,
			Self::CredentialError { .. } => 0,
		}
	}

	fn requires_auth_retry(&self) -> bool {
		match self {
			Self::Pending(observation) =>
				matches!(observation.inventory, Err(AccountApiRuntimeError::Unauthorized))
					|| matches!(observation.profile, Err(AccountApiRuntimeError::Unauthorized)),
			Self::CredentialError { .. } => false,
		}
	}

	fn into_observation(self) -> AccountApiObservation {
		match self {
			Self::Pending(observation) => *observation,
			Self::CredentialError { error } => AccountApiObservation {
				account_revision: 0,
				provider: None,
				inventory: Err(error),
				profile: Err(error),
			},
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AccountApiRequestError {
	Unauthorized,
	ProviderUnavailable,
	ProtocolUnavailable,
}

fn map_request_error(error: AccountApiRequestError) -> AccountApiRuntimeError {
	match error {
		AccountApiRequestError::Unauthorized => AccountApiRuntimeError::Unauthorized,
		AccountApiRequestError::ProviderUnavailable => AccountApiRuntimeError::ProviderUnavailable,
		AccountApiRequestError::ProtocolUnavailable => AccountApiRuntimeError::ProtocolUnavailable,
	}
}

fn map_protocol_error(error: AccountApiProtocolError) -> AccountApiRuntimeError {
	match error {
		AccountApiProtocolError::BodyLimitExceeded
		| AccountApiProtocolError::MalformedResponse
		| AccountApiProtocolError::InvalidValue
		| AccountApiProtocolError::InvalidCreditId
		| AccountApiProtocolError::InvalidIdempotencyKey
		| AccountApiProtocolError::UnknownConsumeOutcome => AccountApiRuntimeError::ProtocolUnavailable,
	}
}

fn map_account_service_error(error: AccountLifecycleError) -> AccountApiRuntimeError {
	match error {
		AccountLifecycleError::AccountMissing => AccountApiRuntimeError::AccountUnavailable,
		AccountLifecycleError::CredentialAbsent
		| AccountLifecycleError::CredentialStore(_)
		| AccountLifecycleError::NotReady(
			decodex_core::AccountLifecycleReadiness::CredentialAbsent
			| decodex_core::AccountLifecycleReadiness::StoreUnavailable
			| decodex_core::AccountLifecycleReadiness::StoreMismatch,
		) => AccountApiRuntimeError::CredentialUnavailable,
		AccountLifecycleError::ProviderMismatch
		| AccountLifecycleError::NotReady(
			decodex_core::AccountLifecycleReadiness::ProviderMismatch,
		)
		| AccountLifecycleError::StaleAccount => AccountApiRuntimeError::AccountChanged,
		AccountLifecycleError::Refresh(_) => AccountApiRuntimeError::CredentialUnavailable,
		AccountLifecycleError::AccountDisabled
		| AccountLifecycleError::NotReady(_)
		| AccountLifecycleError::OperationRejected(_)
		| AccountLifecycleError::CredentialImport
		| AccountLifecycleError::InvalidOperation
		| AccountLifecycleError::Persistence(_)
		| AccountLifecycleError::CoordinatorUnavailable => AccountApiRuntimeError::AccountUnavailable,
	}
}

fn reset_credit_details_are_complete(
	reported_available_count: u64,
	details: &AccountApiResetCredits,
) -> bool {
	details.details_complete
		&& details.reported_available_count == reported_available_count
		&& u64::try_from(details.credits.len()).ok() == Some(reported_available_count)
}

fn should_retry_reset_credit_details(
	reported_available_count: u64,
	details: &Result<AccountApiResetCredits, AccountApiRuntimeError>,
) -> bool {
	match details {
		Ok(details) => !reset_credit_details_are_complete(reported_available_count, details),
		Err(AccountApiRuntimeError::Unauthorized) => false,
		Err(_) => true,
	}
}

#[cfg(test)]
mod tests {
	use decodex_codex::{AccountApiResetCredits, decode_account_api_reset_credits};

	use super::{
		AccountApiRuntimeError, reset_credit_details_are_complete,
		should_retry_reset_credit_details,
	};

	#[test]
	fn incomplete_or_mismatched_reset_credit_details_get_one_bounded_retry() {
		let complete = decode_account_api_reset_credits(
			br#"{"available_count":1,"credits":[{"id":"credit-1","reset_type":"codexRateLimits","status":"available","granted_at":1800000000,"expires_at":1800003600}]}"#,
		)
		.expect("complete fixture");
		assert!(reset_credit_details_are_complete(1, &complete));
		assert!(!should_retry_reset_credit_details(1, &Ok(complete.clone())));
		assert!(should_retry_reset_credit_details(2, &Ok(complete)));

		let incomplete = AccountApiResetCredits {
			reported_available_count: 1,
			credits: Vec::new(),
			details_complete: false,
		};
		assert!(should_retry_reset_credit_details(1, &Ok(incomplete)));
		assert!(should_retry_reset_credit_details(
			1,
			&Err(AccountApiRuntimeError::ProviderUnavailable),
		));
		assert!(!should_retry_reset_credit_details(1, &Err(AccountApiRuntimeError::Unauthorized),));
	}
}
