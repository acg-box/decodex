//! Narrow provider session keeps the account lock alive across preflight and send.
use crate::{
	account_api::{AccountApiInventory, AccountApiRuntime, AccountApiRuntimeError},
	account_launch::ResetCardServiceError,
	account_service::AccountApiCredential,
};
use decodex_codex::{ExactResetCreditId, ResetCardIdempotencyKey};
use decodex_core::{AccountId, ResetCardConsumeOutcome};
use std::{future::Future, pin::Pin};

type ResultFuture<'a, T> =
	Pin<Box<dyn Future<Output = Result<T, ResetCardServiceError>> + Send + 'a>>;
pub(super) trait ResetCardProvider: Send + Sync {
	fn session(
		&self,
		account: &AccountId,
		revision: i64,
	) -> ResultFuture<'_, Box<dyn ResetCardSession>>;
}
pub(super) trait ResetCardSession: Send {
	fn inventory(&mut self) -> ResultFuture<'_, AccountApiInventory>;
	fn consume<'a>(
		&'a mut self,
		key: &'a ResetCardIdempotencyKey,
		credit: &'a ExactResetCreditId,
	) -> ResultFuture<'a, ResetCardConsumeOutcome>;
}
struct ApiSession {
	api: AccountApiRuntime,
	credential: AccountApiCredential,
}
impl ResetCardProvider for AccountApiRuntime {
	fn session(
		&self,
		account: &AccountId,
		revision: i64,
	) -> ResultFuture<'_, Box<dyn ResetCardSession>> {
		let account = account.clone();
		Box::pin(async move {
			let credential = self.reset_session(&account, revision).await.map_err(map_error)?;
			Ok(Box::new(ApiSession { api: self.clone(), credential }) as Box<dyn ResetCardSession>)
		})
	}
}
impl ResetCardSession for ApiSession {
	fn inventory(&mut self) -> ResultFuture<'_, AccountApiInventory> {
		Box::pin(async { self.api.reset_inventory(&self.credential).await.map_err(map_error) })
	}

	fn consume<'a>(
		&'a mut self,
		key: &'a ResetCardIdempotencyKey,
		credit: &'a ExactResetCreditId,
	) -> ResultFuture<'a, ResetCardConsumeOutcome> {
		Box::pin(async move {
			self.api
				.consume_exact_reset_credit(&self.credential, key, credit)
				.await
				.map_err(map_error)
		})
	}
}
fn map_error(error: AccountApiRuntimeError) -> ResetCardServiceError {
	match error {
		AccountApiRuntimeError::AccountChanged => ResetCardServiceError::AccountChanged,
		AccountApiRuntimeError::AccountUnavailable => ResetCardServiceError::AccountStateRejected,
		AccountApiRuntimeError::CredentialUnavailable => ResetCardServiceError::VaultUnavailable,
		_ => ResetCardServiceError::ProviderUnavailable,
	}
}
