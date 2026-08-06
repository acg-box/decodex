//! One authenticated OpenAI/Codex backend API client for account observation and reset cards.

use std::{sync::Arc, time::Duration};

use decodex_codex::{
	AccountApiConsumeOutcome, AccountApiProfile, AccountApiProtocolError, AccountApiQuotaWindow,
	AccountApiResetCredit, AccountApiUsage, decode_account_api_consume, decode_account_api_profile,
	decode_account_api_reset_credits, decode_account_api_usage,
};
use decodex_core::{
	AccountId, AccountOperationId, AccountProvider, ProviderIdentity, ResetCardConsumeOutcome,
};
use reqwest::{Method, StatusCode};

use crate::account_service::{AccountApiCredential, AccountLifecycleError, AccountService};

const BACKEND_API_BASE: &str = "https://chatgpt.com/backend-api";
const USAGE_PATH: &str = "/wham/usage";
const PROFILE_PATH: &str = "/wham/profiles/me";
const RESET_CREDITS_PATH: &str = "/wham/rate-limit-reset-credits";
const CONSUME_RESET_CREDIT_PATH: &str = "/wham/rate-limit-reset-credits/consume";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MINIMUM_ACCESS_TOKEN_VALIDITY: Duration = Duration::from_secs(20);

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
impl AccountApiInventory {
	pub(crate) fn resolve_exact_credit_id(
		&self,
		descriptor: decodex_core::ResetCardDescriptor,
	) -> Result<decodex_codex::ExactResetCreditId, AccountApiRuntimeError> {
		if !self.details_complete {
			return Err(AccountApiRuntimeError::ProtocolUnavailable);
		}
		let mut matches = self.credits.iter().filter(|credit| credit.descriptor() == descriptor);
		let Some(credit) = matches.next() else {
			return Err(AccountApiRuntimeError::ProtocolUnavailable);
		};
		if matches.next().is_some() {
			return Err(AccountApiRuntimeError::ProtocolUnavailable);
		}
		Ok(credit.exact_id().clone())
	}

	pub(crate) fn contains_exact_credit_id(
		&self,
		exact_id: &decodex_codex::ExactResetCreditId,
	) -> bool {
		self.credits.iter().any(|credit| credit.exact_id() == exact_id)
	}
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

	/// Consume one exact reset credit with the provider's direct API contract.
	pub(crate) async fn consume_reset_credit(
		&self,
		account_id: &AccountId,
		account_revision: i64,
		redeem_request_id: &str,
		credit_id: &decodex_codex::ExactResetCreditId,
	) -> Result<ResetCardConsumeOutcome, AccountApiRuntimeError> {
		let response = match self
			.consume_reset_credit_request(
				account_id,
				account_revision,
				redeem_request_id,
				credit_id,
			)
			.await
		{
			Ok(response) => response,
			Err(AccountApiRuntimeError::Unauthorized) => {
				self.refresh_after_unauthorized(account_id, account_revision).await?;
				self.consume_reset_credit_request(
					account_id,
					account_revision,
					redeem_request_id,
					credit_id,
				)
				.await?
			},
			Err(error) => return Err(error),
		};
		decode_account_api_consume(&response)
			.map(map_consume_outcome)
			.map_err(|_| AccountApiRuntimeError::ProtocolUnavailable)
	}

	async fn consume_reset_credit_request(
		&self,
		account_id: &AccountId,
		account_revision: i64,
		redeem_request_id: &str,
		credit_id: &decodex_codex::ExactResetCreditId,
	) -> Result<Vec<u8>, AccountApiRuntimeError> {
		let credential = self
			.accounts
			.api_credential_for_observation(account_id, MINIMUM_ACCESS_TOKEN_VALIDITY)
			.await
			.map_err(map_account_service_error)?;
		if credential.account_revision != account_revision {
			return Err(AccountApiRuntimeError::AccountChanged);
		}
		let body = serde_json::json!({
			"redeem_request_id": redeem_request_id,
			"credit_id": credit_id.as_str(),
		});
		let response = self
			.request_json(Method::POST, CONSUME_RESET_CREDIT_PATH, &credential, Some(&body))
			.await
			.map_err(map_request_error)?;
		Ok(response)
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
		let details =
			match self.request_json(Method::GET, RESET_CREDITS_PATH, credential, None).await {
				Ok(body) => match decode_account_api_reset_credits(&body) {
					Ok(details) => Ok(details),
					Err(error) => Err(map_protocol_error(error)),
				},
				Err(error) => Err(map_request_error(error)),
			};
		match details {
			Ok(details) => Ok(AccountApiInventory {
				account_revision: credential.account_revision,
				quota_windows: usage.quota_windows,
				reported_available_count: Some(reported_available_count),
				details_complete: details.details_complete
					&& details.reported_available_count == reported_available_count,
				credits: if details.details_complete
					&& details.reported_available_count == reported_available_count
				{
					details.credits
				} else {
					Vec::new()
				},
			}),
			Err(error) => {
				// A detail failure must not erase a valid quota snapshot.  The row stays selectable
				// only for display; manual selection requires a complete later detail read.
				if error == AccountApiRuntimeError::Unauthorized {
					Err(error)
				} else {
					Ok(AccountApiInventory {
						account_revision: credential.account_revision,
						quota_windows: usage.quota_windows,
						reported_available_count: Some(reported_available_count),
						details_complete: false,
						credits: Vec::new(),
					})
				}
			},
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

fn map_consume_outcome(outcome: AccountApiConsumeOutcome) -> ResetCardConsumeOutcome {
	match outcome {
		AccountApiConsumeOutcome::Reset => ResetCardConsumeOutcome::Reset,
		AccountApiConsumeOutcome::NothingToReset => ResetCardConsumeOutcome::NothingToReset,
		AccountApiConsumeOutcome::NoCredit => ResetCardConsumeOutcome::NoCredit,
		AccountApiConsumeOutcome::AlreadyRedeemed => ResetCardConsumeOutcome::AlreadyRedeemed,
	}
}
