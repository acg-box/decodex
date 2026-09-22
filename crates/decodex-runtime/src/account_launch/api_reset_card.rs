//! Durable, explicitly confirmed reset-card use. Unknown sends are never replayed.

mod provider;
#[cfg(test)] mod tests;

use super::{
	ResetCardFailureCode, ResetCardOperationStatus, ResetCardPreparation, ResetCardServiceError,
};
use crate::account_api::{AccountApiInventory, AccountApiRuntime};
use decodex_codex::{ExactResetCreditId, ResetCardIdempotencyKey};
use decodex_core::{AccountId, ResetCardConsumeOutcome, ResetCardDescriptor};
use decodex_database::{ResetCardOperation, SqliteStore, StoreError};
use provider::ResetCardProvider;
use std::{
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, Notify, watch};

#[derive(Clone)]
pub(crate) struct ApiResetCardRuntime(Arc<Inner>);
struct Inner {
	store: SqliteStore,
	provider: Arc<dyn ResetCardProvider>,
	stopping: AtomicBool,
	worker: Mutex<()>,
	wakeup: Notify,
	observation: Arc<Notify>,
}
impl ApiResetCardRuntime {
	pub(crate) fn new(store: SqliteStore, api: Arc<AccountApiRuntime>) -> Self {
		Self::with_provider(store, api)
	}

	fn with_provider(store: SqliteStore, provider: Arc<dyn ResetCardProvider>) -> Self {
		Self(Arc::new(Inner {
			store,
			provider,
			stopping: AtomicBool::new(false),
			worker: Mutex::new(()),
			wakeup: Notify::new(),
			observation: Arc::new(Notify::new()),
		}))
	}

	pub(crate) fn observation_wakeup(&self) -> Arc<Notify> {
		Arc::clone(&self.0.observation)
	}

	pub(crate) fn begin_shutdown(&self) {
		self.0.stopping.store(true, Ordering::Release);
		self.0.wakeup.notify_one();
	}

	pub(crate) async fn wait_for_shutdown(&self) {
		let _guard = self.0.worker.lock().await;
	}

	pub(crate) async fn daemon_service(self, mut stop: watch::Receiver<bool>) {
		loop {
			if *stop.borrow() || self.0.stopping.load(Ordering::Acquire) {
				return;
			}
			self.process_pending().await;
			tokio::select! {
				_ = stop.changed() => { if *stop.borrow() || stop.has_changed().is_err() { return; } },
				_ = self.0.wakeup.notified() => {},
				_ = tokio::time::sleep(Duration::from_secs(5)) => {},
			}
		}
	}

	pub(crate) async fn prepare(
		&self,
		key: &str,
		account_id: &AccountId,
		revision: i64,
		descriptor: ResetCardDescriptor,
	) -> Result<ResetCardPreparation, ResetCardServiceError> {
		ResetCardIdempotencyKey::new(key.to_owned())
			.map_err(|_| ResetCardServiceError::InvalidRequest)?;
		if revision <= 0 {
			return Err(ResetCardServiceError::InvalidRequest);
		}
		// Durable replay precedes current account/provider checks, including after logout.
		if let Some(old) =
			self.0.store.reset_card_operation(key.to_owned()).await.map_err(map_store)?
		{
			if old.account_id != account_id.as_str()
				|| old.account_revision != revision
				|| old.granted_at != descriptor.granted_at().unix_seconds()
				|| old.expires_at != descriptor.expires_at().unix_seconds()
			{
				return Err(ResetCardServiceError::IdempotencyConflict);
			}
			return Ok(ResetCardPreparation {
				account_id: account_id.clone(),
				account_revision: revision,
				descriptor,
			});
		}
		if self.0.stopping.load(Ordering::Acquire) {
			return Err(ResetCardServiceError::ProductStateUnavailable);
		}
		let mut session = self.0.provider.session(account_id, revision).await?;
		let inventory = session.inventory().await?;
		let credit = select_credit(&inventory, revision, descriptor)?;
		self.0
			.store
			.prepare_reset_card(ResetCardOperation {
				key: key.to_owned(),
				account_id: account_id.as_str().to_owned(),
				account_revision: revision,
				granted_at: descriptor.granted_at().unix_seconds(),
				expires_at: descriptor.expires_at().unix_seconds(),
				exact_credit_id: Some(credit.as_str().to_owned()),
				state: "prepared".into(),
				outcome: None,
				failure: None,
			})
			.await
			.map_err(map_store)?;
		self.0.wakeup.notify_one();
		Ok(ResetCardPreparation {
			account_id: account_id.clone(),
			account_revision: revision,
			descriptor,
		})
	}

	pub(crate) async fn latest_operation(
		&self,
		account: &AccountId,
	) -> Result<Option<ResetCardOperation>, ResetCardServiceError> {
		self.0.store.latest_reset_card_operation(account.as_str().into()).await.map_err(map_store)
	}

	pub(crate) async fn operation_status(
		&self,
		key: &str,
	) -> Result<ResetCardOperationStatus, ResetCardServiceError> {
		ResetCardIdempotencyKey::new(key.to_owned())
			.map_err(|_| ResetCardServiceError::InvalidRequest)?;
		let Some(operation) =
			self.0.store.reset_card_operation(key.to_owned()).await.map_err(map_store)?
		else {
			return Ok(ResetCardOperationStatus::NotFound);
		};
		Ok(match operation.state.as_str() {
			"prepared" => ResetCardOperationStatus::Prepared,
			"sending" => ResetCardOperationStatus::EffectAmbiguous,
			"completed" =>
				ResetCardOperationStatus::Completed(parse_outcome(operation.outcome.as_deref())?),
			"failed" =>
				ResetCardOperationStatus::FailedBeforeEffect(match operation.failure.as_deref() {
					Some("account_changed") => ResetCardFailureCode::AccountChanged,
					Some("inventory_changed") => ResetCardFailureCode::InventoryChanged,
					_ => ResetCardFailureCode::ProviderUnavailable,
				}),
			_ => return Err(ResetCardServiceError::ProductStateUnavailable),
		})
	}

	async fn process_pending(&self) {
		// Shutdown drains this owner instead of cancelling a possibly transmitted POST.
		let _guard = self.0.worker.lock().await;
		let Ok(operations) = self.0.store.pending_reset_cards().await else {
			return;
		};
		for operation in operations {
			if self.0.stopping.load(Ordering::Acquire) {
				break;
			}
			self.process(operation).await;
		}
	}

	async fn process(&self, operation: ResetCardOperation) {
		if operation.outcome.is_some() {
			// A committed receipt needs only local completion, never another provider write.
			let _ = self.0.store.complete_reset_card(operation.key).await;
			self.0.observation.notify_one();
			return;
		}
		let session = self.session_for_operation(&operation).await;
		let mut session = match session {
			Ok(session) => session,
			Err(error) => {
				let failure = match error {
					ResetCardServiceError::AccountChanged
					| ResetCardServiceError::AccountNotFound
					| ResetCardServiceError::AccountStateRejected => "account_changed",
					ResetCardServiceError::InventoryChanged
					| ResetCardServiceError::InventoryIncomplete => "inventory_changed",
					_ => "provider_unavailable",
				};
				let _ =
					self.0.store.fail_reset_card_before_send(operation.key, failure.into()).await;
				return;
			},
		};
		let (Ok(key), Some(Ok(credit))) = (
			ResetCardIdempotencyKey::new(operation.key.clone()),
			operation.exact_credit_id.map(ExactResetCreditId::new),
		) else {
			return;
		};
		// Commit the no-retry barrier BEFORE the request can leave this process.
		if !matches!(self.0.store.begin_reset_card_send(operation.key.clone()).await, Ok(true)) {
			return;
		}
		let Ok(outcome) = session.consume(&key, &credit).await else {
			return;
		};
		if self
			.0
			.store
			.record_reset_card_outcome(operation.key.clone(), outcome_text(outcome).into())
			.await
			.is_ok()
		{
			let _ = self.0.store.complete_reset_card(operation.key).await;
			self.0.observation.notify_one();
		}
	}

	async fn session_for_operation(
		&self,
		operation: &ResetCardOperation,
	) -> Result<Box<dyn provider::ResetCardSession>, ResetCardServiceError> {
		let account = AccountId::new(operation.account_id.clone())
			.map_err(|_| ResetCardServiceError::AccountChanged)?;
		let mut session = self.0.provider.session(&account, operation.account_revision).await?;
		let inventory = session.inventory().await?;
		use decodex_core::ResetCardTimestamp;
		let descriptor = ResetCardTimestamp::from_unix_seconds(operation.granted_at)
			.and_then(|grant| {
				ResetCardTimestamp::from_unix_seconds(operation.expires_at)
					.and_then(|expiry| ResetCardDescriptor::new(grant, expiry))
			})
			.map_err(|_| ResetCardServiceError::InventoryChanged)?;
		let credit = select_credit(&inventory, operation.account_revision, descriptor)?;
		if Some(credit.as_str()) != operation.exact_credit_id.as_deref() {
			return Err(ResetCardServiceError::InventoryChanged);
		}
		Ok(session)
	}
}
fn select_credit(
	inventory: &AccountApiInventory,
	revision: i64,
	descriptor: ResetCardDescriptor,
) -> Result<ExactResetCreditId, ResetCardServiceError> {
	if inventory.account_revision != revision {
		return Err(ResetCardServiceError::AccountChanged);
	}
	if !inventory.details_complete
		|| inventory.reported_available_count != u64::try_from(inventory.credits.len()).ok()
	{
		return Err(ResetCardServiceError::InventoryIncomplete);
	}
	let now = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_err(|_| ResetCardServiceError::InvalidRequest)?
		.as_secs();
	if now >= u64::try_from(descriptor.expires_at().unix_seconds()).unwrap_or(0) {
		return Err(ResetCardServiceError::InventoryChanged);
	}
	let mut matches = inventory.credits.iter().filter(|card| card.descriptor() == descriptor);
	let credit = matches.next().ok_or(ResetCardServiceError::InventoryChanged)?;
	if matches.next().is_some() {
		return Err(ResetCardServiceError::InventoryChanged);
	}
	Ok(credit.exact_id().clone())
}
fn map_store(error: StoreError) -> ResetCardServiceError {
	match error {
		StoreError::IdempotencyConflict => ResetCardServiceError::IdempotencyConflict,
		StoreError::CapacityExhausted(_) => ResetCardServiceError::AcceptanceUnknown,
		StoreError::InvalidInput(_) => ResetCardServiceError::InvalidRequest,
		_ => ResetCardServiceError::ProductStateUnavailable,
	}
}
fn outcome_text(outcome: ResetCardConsumeOutcome) -> &'static str {
	match outcome {
		ResetCardConsumeOutcome::Reset => "reset",
		ResetCardConsumeOutcome::NothingToReset => "nothing_to_reset",
		ResetCardConsumeOutcome::NoCredit => "no_credit",
		ResetCardConsumeOutcome::AlreadyRedeemed => "already_redeemed",
	}
}
fn parse_outcome(outcome: Option<&str>) -> Result<ResetCardConsumeOutcome, ResetCardServiceError> {
	match outcome {
		Some("reset") => Ok(ResetCardConsumeOutcome::Reset),
		Some("nothing_to_reset") => Ok(ResetCardConsumeOutcome::NothingToReset),
		Some("no_credit") => Ok(ResetCardConsumeOutcome::NoCredit),
		Some("already_redeemed") => Ok(ResetCardConsumeOutcome::AlreadyRedeemed),
		_ => Err(ResetCardServiceError::ProductStateUnavailable),
	}
}
