//! Durable reset-card worker backed by the direct account backend API.

use std::{
	sync::Arc,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use decodex_codex::{ExactResetCreditId, ResetCardIdempotencyKey};
use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountRecord, AccountState, ResetCardConsumeOutcome,
	ResetCardDescriptor, ServerIdentity,
};
use decodex_postgres::{
	CommandIdentity, OutboxReconciliation, PostgresStore, ReconciliationOutcome, ResetCardClaim,
	ResetCardFailureCode, ResetCardOperationStatus, ResetCardPreparation, StoreError,
};
use serde_json::json;
use tokio::{
	sync::{Mutex, Notify, oneshot, watch},
	time,
};

use super::{ResetCardServiceError, ResetCardVaultStatus};
use crate::{
	account_api::{AccountApiInventory, AccountApiRuntime, AccountApiRuntimeError},
	account_service::{AccountLifecycleError, AccountService},
};

const CLAIM_LEASE: Duration = Duration::from_secs(360);
const CLAIM_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const WORKER_IDLE_POLL: Duration = Duration::from_secs(5);
const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const RECONCILIATION_DELAY: Duration = Duration::from_secs(1);
const OBSERVATION_CREDENTIAL_VALIDITY: Duration = Duration::from_secs(20);

/// Direct backend API reset-card runtime.
#[derive(Clone)]
pub(crate) struct ApiResetCardRuntime {
	inner: Arc<ApiResetCardRuntimeInner>,
}

struct ApiResetCardRuntimeInner {
	store: PostgresStore,
	accounts: Arc<AccountService>,
	api: Arc<AccountApiRuntime>,
	worker_id: String,
	worker_lock: Mutex<()>,
	worker_wakeup: Arc<Notify>,
	observation_wakeup: Arc<Notify>,
}

impl ApiResetCardRuntime {
	/// Compose the direct provider runtime without a Codex executable or schema probe.
	pub(crate) fn start(
		store: PostgresStore,
		accounts: Arc<AccountService>,
		api: Arc<AccountApiRuntime>,
	) -> Result<Self, ResetCardServiceError> {
		let worker_id = ServerIdentity::generate()
			.map_err(|_| ResetCardServiceError::ProductStateUnavailable)?
			.as_str()
			.to_owned();
		let runtime = Self {
			inner: Arc::new(ApiResetCardRuntimeInner {
				store,
				accounts,
				api,
				worker_id,
				worker_lock: Mutex::new(()),
				worker_wakeup: Arc::new(Notify::new()),
				observation_wakeup: Arc::new(Notify::new()),
			}),
		};
		runtime.inner.worker_wakeup.notify_one();
		Ok(runtime)
	}

	pub(crate) async fn daemon_service(self, mut stop: watch::Receiver<bool>) {
		run_worker(Arc::clone(&self.inner), &mut stop).await;
	}

	pub(crate) fn begin_shutdown(&self) {
		self.inner.worker_wakeup.notify_one();
	}

	pub(crate) async fn wait_for_shutdown(&self) {}

	pub(crate) fn vault_status(&self) -> ResetCardVaultStatus {
		ResetCardVaultStatus::Ready
	}

	pub(crate) fn observation_wakeup(&self) -> Arc<Notify> {
		Arc::clone(&self.inner.observation_wakeup)
	}

	/// Prepare one exact durable reset-card operation under the current credential binding.
	pub(crate) async fn prepare(
		&self,
		idempotency_key: &str,
		account_id: &AccountId,
		expected_revision: i64,
		descriptor: ResetCardDescriptor,
	) -> Result<ResetCardPreparation, ResetCardServiceError> {
		let command = CommandIdentity::new(
			idempotency_key,
			format!(
				"decodex/reset-card-operation/1\n{}\n{}\n{}\n{}",
				account_id.as_str(),
				expected_revision,
				descriptor.granted_at().unix_seconds(),
				descriptor.expires_at().unix_seconds(),
			)
			.as_bytes(),
		)
		.map_err(map_store_error)?;
		if let Some(preparation) = self
			.inner
			.store
			.replay_reset_card_api_preparation(&command, account_id, expected_revision, descriptor)
			.await
			.map_err(map_store_error)?
		{
			self.inner.worker_wakeup.notify_one();
			return Ok(preparation);
		}
		let account = self
			.inner
			.accounts
			.inspect(account_id)
			.await
			.map_err(map_account_service_error)?
			.account;
		if !api_account_admitted(&account) {
			return Err(ResetCardServiceError::AccountStateRejected);
		}
		if account.revision != expected_revision {
			return Err(ResetCardServiceError::ExpectedRevisionMismatch {
				actual: account.revision,
			});
		}
		let credential = self
			.inner
			.accounts
			.api_credential_for_observation(account_id, OBSERVATION_CREDENTIAL_VALIDITY)
			.await
			.map_err(map_account_service_error)?;
		if credential.account_revision != expected_revision {
			return Err(ResetCardServiceError::AccountChanged);
		}
		let preparation = self
			.inner
			.store
			.prepare_reset_card_api_operation(
				&command,
				account_id,
				expected_revision,
				&credential.binding,
				descriptor,
			)
			.await
			.map_err(map_store_error)?;
		self.inner.worker_wakeup.notify_one();
		Ok(preparation)
	}

	pub(crate) async fn operation_status(
		&self,
		idempotency_key: &str,
	) -> Result<ResetCardOperationStatus, ResetCardServiceError> {
		self.inner.store.reset_card_operation_status(idempotency_key).await.map_err(map_store_error)
	}
}

async fn run_worker(inner: Arc<ApiResetCardRuntimeInner>, stop: &mut watch::Receiver<bool>) {
	loop {
		if *stop.borrow() {
			return;
		}
		let Ok(_worker) = inner.worker_lock.try_lock() else {
			return;
		};
		while !*stop.borrow() {
			let claim =
				match inner.store.claim_reset_card_operation(&inner.worker_id, CLAIM_LEASE).await {
					Ok(Some(claim)) => claim,
					Ok(None) | Err(_) => break,
				};
			process_claim(Arc::clone(&inner), claim).await;
			inner.observation_wakeup.notify_one();
		}
		drop(_worker);
		tokio::select! {
			biased;
			_ = wait_for_shutdown(stop) => return,
			_ = inner.worker_wakeup.notified() => {},
			_ = time::sleep(WORKER_IDLE_POLL) => {},
		}
	}
}

async fn wait_for_shutdown(stop: &mut watch::Receiver<bool>) {
	while !*stop.borrow() {
		if stop.changed().await.is_err() {
			return;
		}
	}
}

async fn process_claim(inner: Arc<ApiResetCardRuntimeInner>, claim: ResetCardClaim) {
	if !claim_binding_admitted(&inner, &claim).await {
		fail_before_effect(&inner, &claim, ResetCardFailureCode::AccountChanged).await;
		return;
	}
	if ResetCardIdempotencyKey::new(claim.provider_idempotency_key().to_owned()).is_err() {
		fail_before_effect(&inner, &claim, ResetCardFailureCode::ProviderUnavailable).await;
		return;
	}
	let exact_credit_id = match claim.exact_credit_id() {
		Some(value) => match ExactResetCreditId::new(value.to_owned()) {
			Ok(value) => value,
			Err(_) => {
				fail_before_effect(&inner, &claim, ResetCardFailureCode::InventoryChanged).await;
				return;
			},
		},
		None => match resolve_credit_id(&inner, &claim).await {
			Some(value) => value,
			None => return,
		},
	};
	if let Some(outcome) = claim.recorded_outcome {
		reconcile_recorded_claim(&inner, &claim, &exact_credit_id, outcome).await;
		return;
	}
	let Ok(()) = inner.store.begin_reset_card_effect(&claim, &inner.worker_id).await else {
		return;
	};
	let outcome = match run_with_claim_heartbeat(&inner, &claim, || {
		inner.api.consume_reset_credit(
			&claim.account_id,
			claim.account_revision,
			claim.provider_idempotency_key(),
			&exact_credit_id,
		)
	})
	.await
	{
		Ok(outcome) => outcome,
		Err(_) => return,
	};
	let receipt = json!({"outcome": outcome_text(outcome)});
	if inner
		.store
		.record_outbox_receipt(claim.id, &inner.worker_id, claim.claim_token(), &receipt)
		.await
		.is_err()
	{
		return;
	}
	let Some(inventory) = read_inventory_for_claim(&inner, &claim).await else {
		return;
	};
	complete_reconciliation(&inner, &claim, &exact_credit_id, outcome, &inventory).await;
}

async fn resolve_credit_id(
	inner: &Arc<ApiResetCardRuntimeInner>,
	claim: &ResetCardClaim,
) -> Option<ExactResetCreditId> {
	let observation = inner.api.observe_account(&claim.account_id).await;
	let inventory = match observation.inventory {
		Ok(inventory) if inventory.details_complete => inventory,
		Ok(_) | Err(_) => {
			fail_before_effect(inner, claim, ResetCardFailureCode::InventoryIncomplete).await;
			return None;
		},
	};
	let exact = match inventory.resolve_exact_credit_id(claim.descriptor) {
		Ok(exact) => exact,
		Err(_) => {
			fail_before_effect(inner, claim, ResetCardFailureCode::InventoryChanged).await;
			return None;
		},
	};
	if !claim_binding_admitted(inner, claim).await {
		fail_before_effect(inner, claim, ResetCardFailureCode::AccountChanged).await;
		return None;
	}
	if inner.store.bind_reset_card_credit(claim, &inner.worker_id, exact.as_str()).await.is_err() {
		return None;
	}
	Some(exact)
}

async fn reconcile_recorded_claim(
	inner: &Arc<ApiResetCardRuntimeInner>,
	claim: &ResetCardClaim,
	exact_credit_id: &ExactResetCreditId,
	outcome: ResetCardConsumeOutcome,
) {
	let Some(inventory) = read_inventory_for_claim(inner, claim).await else {
		return;
	};
	complete_reconciliation(inner, claim, exact_credit_id, outcome, &inventory).await;
}

async fn read_inventory_for_claim(
	inner: &Arc<ApiResetCardRuntimeInner>,
	claim: &ResetCardClaim,
) -> Option<AccountApiInventory> {
	inner.api.observe_account(&claim.account_id).await.inventory.ok()
}

async fn run_with_claim_heartbeat<T, MakeWork, Work>(
	inner: &Arc<ApiResetCardRuntimeInner>,
	claim: &ResetCardClaim,
	make_work: MakeWork,
) -> Result<T, ResetCardServiceError>
where
	MakeWork: FnOnce() -> Work,
	Work: std::future::Future<Output = Result<T, AccountApiRuntimeError>>,
{
	inner
		.store
		.renew_reset_card_claim(claim.id, &inner.worker_id, claim.claim_token(), CLAIM_LEASE)
		.await
		.map_err(|_| ResetCardServiceError::ProductStateUnavailable)?;
	let (stop_sender, stop_receiver) = oneshot::channel();
	let heartbeat_store = inner.store.clone();
	let heartbeat_worker = inner.worker_id.clone();
	let heartbeat = maintain_claim_heartbeat(
		heartbeat_store,
		heartbeat_worker,
		claim.id,
		claim.claim_token().to_owned(),
		stop_receiver,
	);
	tokio::pin!(heartbeat);
	let work = make_work();
	tokio::pin!(work);
	tokio::select! {
		result = &mut work => {
			let _ = stop_sender.send(());
			match heartbeat.await {
				Ok(()) => result.map_err(map_api_error_to_reset),
				Err(_) => Err(ResetCardServiceError::ProductStateUnavailable),
			}
		},
		result = &mut heartbeat => {
			drop(stop_sender);
			match result {
				Ok(()) | Err(_) => Err(ResetCardServiceError::ProductStateUnavailable),
			}
		},
	}
}

async fn maintain_claim_heartbeat(
	store: PostgresStore,
	worker_id: String,
	claim_id: i64,
	claim_token: String,
	mut stop: oneshot::Receiver<()>,
) -> Result<(), StoreError> {
	loop {
		tokio::select! {
			_ = &mut stop => return Ok(()),
			_ = time::sleep(CLAIM_HEARTBEAT_INTERVAL) => {
				store.renew_reset_card_claim(claim_id, &worker_id, &claim_token, CLAIM_LEASE).await?;
			},
		}
	}
}

async fn claim_binding_admitted(
	inner: &Arc<ApiResetCardRuntimeInner>,
	claim: &ResetCardClaim,
) -> bool {
	let Ok(credential) = inner
		.accounts
		.api_credential_for_observation(&claim.account_id, OBSERVATION_CREDENTIAL_VALIDITY)
		.await
	else {
		return false;
	};
	credential.account_revision == claim.account_revision
		&& credential.binding == claim.process_binding.credential
}

async fn complete_reconciliation(
	inner: &ApiResetCardRuntimeInner,
	claim: &ResetCardClaim,
	exact_credit_id: &ExactResetCreditId,
	outcome: ResetCardConsumeOutcome,
	inventory: &AccountApiInventory,
) {
	if !inventory.details_complete {
		return;
	}
	let selected_available = inventory.contains_exact_credit_id(exact_credit_id);
	let selected_expired = current_unix_seconds()
		.is_some_and(|now| now >= claim.descriptor.expires_at().unix_seconds());
	if !readback_confirms_outcome(outcome, selected_available, selected_expired) {
		return;
	}
	let readback = json!({
		"schema": "decodex/reset-card-readback/1",
		"account_id": claim.account_id.as_str(),
		"account_revision": claim.account_revision,
		"outcome": outcome_text(outcome),
		"available_count": inventory.reported_available_count,
		"selected_exact_credit_available": selected_available,
		"selected_descriptor_expired": selected_expired,
	});
	let _ = inner
		.store
		.reconcile_outbox(
			claim.id,
			&inner.worker_id,
			claim.claim_token(),
			&OutboxReconciliation { readback, outcome: ReconciliationOutcome::EffectPresent },
			RECONCILIATION_DELAY,
			RETENTION,
		)
		.await;
}

const fn readback_confirms_outcome(
	outcome: ResetCardConsumeOutcome,
	selected_available: bool,
	selected_expired: bool,
) -> bool {
	match outcome {
		ResetCardConsumeOutcome::NothingToReset => selected_available || selected_expired,
		ResetCardConsumeOutcome::Reset
		| ResetCardConsumeOutcome::NoCredit
		| ResetCardConsumeOutcome::AlreadyRedeemed => !selected_available,
	}
}

fn api_account_admitted(account: &AccountRecord) -> bool {
	account.enabled
		&& !account.tombstoned
		&& account.unsettled_operation.is_none()
		&& account.credential.is_some()
		&& matches!(account.observed_state, AccountState::Available | AccountState::Depleted)
}

fn current_unix_seconds() -> Option<i64> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

fn outcome_text(outcome: ResetCardConsumeOutcome) -> &'static str {
	match outcome {
		ResetCardConsumeOutcome::Reset => "reset",
		ResetCardConsumeOutcome::NothingToReset => "nothing_to_reset",
		ResetCardConsumeOutcome::NoCredit => "no_credit",
		ResetCardConsumeOutcome::AlreadyRedeemed => "already_redeemed",
	}
}

fn map_api_error_to_reset(error: AccountApiRuntimeError) -> ResetCardServiceError {
	match error {
		AccountApiRuntimeError::AccountUnavailable => ResetCardServiceError::AccountNotFound,
		AccountApiRuntimeError::CredentialUnavailable => ResetCardServiceError::VaultUnavailable,
		AccountApiRuntimeError::AccountChanged => ResetCardServiceError::AccountChanged,
		AccountApiRuntimeError::ProtocolUnavailable => ResetCardServiceError::InventoryIncomplete,
		AccountApiRuntimeError::Unauthorized | AccountApiRuntimeError::ProviderUnavailable =>
			ResetCardServiceError::ProviderUnavailable,
	}
}

fn map_account_service_error(error: AccountLifecycleError) -> ResetCardServiceError {
	match error {
		AccountLifecycleError::AccountMissing => ResetCardServiceError::AccountNotFound,
		AccountLifecycleError::CredentialAbsent
		| AccountLifecycleError::CredentialStore(_)
		| AccountLifecycleError::NotReady(
			AccountLifecycleReadiness::CredentialAbsent
			| AccountLifecycleReadiness::StoreUnavailable
			| AccountLifecycleReadiness::StoreMismatch,
		) => ResetCardServiceError::VaultUnavailable,
		AccountLifecycleError::ProviderMismatch
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::ProviderMismatch)
		| AccountLifecycleError::StaleAccount => ResetCardServiceError::AccountChanged,
		AccountLifecycleError::Refresh(_) => ResetCardServiceError::ProviderUnavailable,
		AccountLifecycleError::AccountDisabled
		| AccountLifecycleError::NotReady(_)
		| AccountLifecycleError::OperationRejected(_) => ResetCardServiceError::AccountStateRejected,
		AccountLifecycleError::CredentialImport | AccountLifecycleError::InvalidOperation =>
			ResetCardServiceError::InvalidRequest,
		AccountLifecycleError::Persistence(_) | AccountLifecycleError::CoordinatorUnavailable =>
			ResetCardServiceError::ProductStateUnavailable,
	}
}

fn map_store_error(error: StoreError) -> ResetCardServiceError {
	match error {
		StoreError::InvalidInput(_) => ResetCardServiceError::InvalidRequest,
		StoreError::ResetCardSelectionConflict => ResetCardServiceError::InventoryChanged,
		StoreError::RevisionConflict { actual, .. } =>
			ResetCardServiceError::ExpectedRevisionMismatch { actual: actual.unwrap_or(0) },
		StoreError::ResetCardCommitOutcomeUnknown => ResetCardServiceError::AcceptanceUnknown,
		StoreError::Incompatible(_) => ResetCardServiceError::ProductStateUnavailable,
		_ => ResetCardServiceError::ProductStateUnavailable,
	}
}

async fn fail_before_effect(
	inner: &ApiResetCardRuntimeInner,
	claim: &ResetCardClaim,
	failure: ResetCardFailureCode,
) {
	if !claim.requires_reconciliation {
		let _ = inner.store.fail_reset_card_before_effect(claim, &inner.worker_id, failure).await;
	}
}
