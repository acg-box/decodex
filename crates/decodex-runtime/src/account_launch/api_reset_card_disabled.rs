//! Explicitly unavailable reset-card capability for the bounded SQLite Conversation slice.

use std::sync::Arc;

use decodex_core::{AccountId, ResetCardDescriptor};
use tokio::sync::{Notify, watch};

use super::{ResetCardOperationStatus, ResetCardPreparation, ResetCardServiceError};

#[derive(Clone, Default)]
pub(crate) struct ApiResetCardRuntime;

impl ApiResetCardRuntime {
	pub(crate) async fn daemon_service(self, mut stop: watch::Receiver<bool>) {
		while !*stop.borrow() && stop.changed().await.is_ok() {}
	}

	pub(crate) fn begin_shutdown(&self) {}

	pub(crate) async fn wait_for_shutdown(&self) {}

	pub(crate) fn observation_wakeup(&self) -> Arc<Notify> {
		Arc::new(Notify::new())
	}

	pub(crate) async fn prepare(
		&self,
		_idempotency_key: &str,
		_account_id: &AccountId,
		_expected_revision: i64,
		_descriptor: ResetCardDescriptor,
	) -> Result<ResetCardPreparation, ResetCardServiceError> {
		Err(ResetCardServiceError::ProductStateUnavailable)
	}

	pub(crate) async fn operation_status(
		&self,
		_idempotency_key: &str,
	) -> Result<ResetCardOperationStatus, ResetCardServiceError> {
		Err(ResetCardServiceError::ProductStateUnavailable)
	}
}
