//! Small account reads used at process admission boundaries.

use decodex_core::{AccountId, AccountLifecycleReadiness, AccountState};
use serde_json::{Value, json};

use crate::{SqliteStore, StoreError};

/// Credential-negative metadata for one exact account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountMetadata {
	pub account_id: AccountId,
	pub display_label: String,
	pub state: AccountState,
	pub metadata: Value,
	pub revision: i64,
}

impl SqliteStore {
	/// Observe whether one account is process-ready at one exact registry revision.
	pub async fn account_is_ready_at_revision(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
	) -> Result<bool, StoreError> {
		if expected_revision < 1 {
			return Err(StoreError::InvalidInput("expected revision must be positive"));
		}
		let account_id = account_id.clone();
		self.run(move |connection| {
			let account = super::account_lifecycle::read_account_registry_sync(
				connection,
				Some(account_id.as_str()),
				1,
			)?
			.into_iter()
			.next();
			Ok(account.is_some_and(|account| {
				account.revision == expected_revision
					&& account.enabled
					&& matches!(
						account.observed_state,
						AccountState::Available | AccountState::Unknown | AccountState::Depleted
					) && account.lifecycle_readiness == AccountLifecycleReadiness::Ready
					&& !account.tombstoned
			}))
		})
		.await
	}

	/// Read inert metadata without returning credential material or launch authority.
	pub async fn account(
		&self,
		account_id: &AccountId,
	) -> Result<Option<AccountMetadata>, StoreError> {
		let account_id = account_id.clone();
		self.run(move |connection| {
			Ok(super::account_lifecycle::read_account_registry_sync(
				connection,
				Some(account_id.as_str()),
				1,
			)?
			.into_iter()
			.next()
			.map(|account| AccountMetadata {
				account_id: account.account_id,
				display_label: account.label,
				state: account.observed_state,
				metadata: json!({
					"enabled": account.enabled,
					"lifecycle_ready": account.lifecycle_readiness == AccountLifecycleReadiness::Ready,
				}),
				revision: account.revision,
			}))
		})
		.await
	}
}
