//! Entirely in-memory provider; no network client, real account, or real credit is created.
use super::{
	provider::{ResetCardProvider, ResetCardSession},
	*,
};
use decodex_core::DecodexRoot;
use std::{
	future::Future,
	pin::Pin,
	sync::{Mutex as StdMutex, atomic::AtomicUsize},
};

type TestFuture<'a, T> =
	Pin<Box<dyn Future<Output = Result<T, ResetCardServiceError>> + Send + 'a>>;
struct Fake {
	inventory: StdMutex<AccountApiInventory>,
	outcome: StdMutex<Result<ResetCardConsumeOutcome, ResetCardServiceError>>,
	sends: AtomicUsize,
	identities: StdMutex<Vec<(String, String)>>,
	unavailable: AtomicBool,
	pause_send: AtomicBool,
	entered: Notify,
	release: Notify,
}
struct Session(Arc<Fake>);
impl ResetCardProvider for Arc<Fake> {
	fn session(&self, _: &AccountId, _: i64) -> TestFuture<'_, Box<dyn ResetCardSession>> {
		Box::pin(async {
			if self.unavailable.load(Ordering::Acquire) {
				return Err(ResetCardServiceError::AccountChanged);
			}
			Ok(Box::new(Session(Self::clone(self))) as Box<dyn ResetCardSession>)
		})
	}
}
impl ResetCardSession for Session {
	fn inventory(&mut self) -> TestFuture<'_, AccountApiInventory> {
		Box::pin(async { Ok(self.0.inventory.lock().expect("isolated reset fixture").clone()) })
	}

	fn consume<'a>(
		&'a mut self,
		key: &'a ResetCardIdempotencyKey,
		credit: &'a ExactResetCreditId,
	) -> TestFuture<'a, ResetCardConsumeOutcome> {
		Box::pin(async move {
			self.0.sends.fetch_add(1, Ordering::SeqCst);
			self.0
				.identities
				.lock()
				.expect("isolated reset fixture")
				.push((key.as_str().into(), credit.as_str().into()));
			self.0.entered.notify_one();
			if self.0.pause_send.load(Ordering::Acquire) {
				self.0.release.notified().await;
			}
			*self.0.outcome.lock().expect("isolated reset fixture")
		})
	}
}
fn fixture()
-> (tempfile::TempDir, SqliteStore, Arc<Fake>, ApiResetCardRuntime, AccountId, ResetCardDescriptor)
{
	let dir = tempfile::tempdir().expect("isolated reset fixture");
	let paths = DecodexRoot::new(dir.path().canonicalize().expect("isolated reset fixture"))
		.expect("isolated reset fixture")
		.paths();
	let store = SqliteStore::open(&paths).expect("isolated reset fixture");
	let credits = decodex_codex::decode_account_api_reset_credits(br#"{"available_count":1,"credits":[{"id":"FAKE-ONLY-credit-1","reset_type":"codexRateLimits","status":"available","granted_at":1800000000,"expires_at":4102444800}]}"#).expect("isolated reset fixture");
	let descriptor = credits.credits[0].descriptor();
	let usage = decodex_codex::decode_account_api_usage(br#"{"rate_limit":{"primary_window":{"used_percent":100,"limit_window_seconds":18000,"reset_at":4102444800},"secondary_window":{"used_percent":100,"limit_window_seconds":604800,"reset_at":4102444800}}}"#).expect("isolated reset fixture");
	let fake = Arc::new(Fake {
		inventory: StdMutex::new(AccountApiInventory {
			banner: Default::default(),
			recovery_context: None,
			account_revision: 1,
			ordinary_usage_allowed: None,
			conditions: Default::default(),
			quota_windows: usage.quota_windows,
			reported_available_count: Some(1),
			details_complete: true,
			credits: credits.credits,
		}),
		outcome: StdMutex::new(Ok(ResetCardConsumeOutcome::Reset)),
		sends: AtomicUsize::new(0),
		identities: StdMutex::new(Vec::new()),
		unavailable: AtomicBool::new(false),
		pause_send: AtomicBool::new(false),
		entered: Notify::new(),
		release: Notify::new(),
	});
	let runtime = ApiResetCardRuntime::with_provider(store.clone(), Arc::new(Arc::clone(&fake)));
	(
		dir,
		store,
		fake,
		runtime,
		AccountId::new("21000000-0000-4000-8000-000000000099").expect("isolated reset fixture"),
		descriptor,
	)
}
#[tokio::test]
async fn exact_card_is_sent_once_and_completed_replay_needs_no_credentials() {
	let (_dir, store, fake, runtime, account, descriptor) = fixture();
	runtime.prepare("manual-1", &account, 1, descriptor).await.expect("isolated reset fixture");
	assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
	runtime.process_pending().await;
	fake.unavailable.store(true, Ordering::Release);
	runtime.prepare("manual-1", &account, 1, descriptor).await.expect("isolated reset fixture");
	runtime.process_pending().await;
	assert_eq!(
		runtime.operation_status("manual-1").await.expect("isolated reset fixture"),
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::Reset)
	);
	assert_eq!(
		*fake.identities.lock().expect("isolated reset fixture"),
		vec![("manual-1".into(), "FAKE-ONLY-credit-1".into())]
	);
	assert!(
		store
			.reset_card_operation("manual-1".into())
			.await
			.expect("isolated reset fixture")
			.expect("isolated reset fixture")
			.exact_credit_id
			.is_none()
	);
	assert_eq!(
		runtime
			.prepare("manual-1", &account, 2, descriptor)
			.await
			.expect_err("expected safe refusal"),
		ResetCardServiceError::IdempotencyConflict
	);
}
#[tokio::test]
async fn response_loss_and_restart_never_resend_or_admit_another_key() {
	let (dir, store, fake, runtime, account, descriptor) = fixture();
	*fake.outcome.lock().expect("isolated reset fixture") =
		Err(ResetCardServiceError::ProviderUnavailable);
	runtime.prepare("manual-1", &account, 1, descriptor).await.expect("isolated reset fixture");
	runtime.process_pending().await;
	drop(runtime);
	drop(store);
	let paths = DecodexRoot::new(dir.path().canonicalize().expect("isolated reset fixture"))
		.expect("isolated reset fixture")
		.paths();
	let restarted = ApiResetCardRuntime::with_provider(
		SqliteStore::open(&paths).expect("isolated reset fixture"),
		Arc::new(Arc::clone(&fake)),
	);
	restarted.prepare("manual-1", &account, 1, descriptor).await.expect("isolated reset fixture");
	restarted.process_pending().await;
	assert_eq!(
		restarted.operation_status("manual-1").await.expect("isolated reset fixture"),
		ResetCardOperationStatus::EffectAmbiguous
	);
	assert_eq!(
		restarted
			.prepare("manual-2", &account, 1, descriptor)
			.await
			.expect_err("expected safe refusal"),
		ResetCardServiceError::AcceptanceUnknown
	);
	assert_eq!(fake.sends.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn prepared_rechecks_exact_identity_and_rejects_replacement_card() {
	let (_dir, _store, fake, runtime, account, descriptor) = fixture();
	runtime.prepare("manual-1", &account, 1, descriptor).await.expect("isolated reset fixture");
	let replacement = decodex_codex::decode_account_api_reset_credits(br#"{"available_count":1,"credits":[{"id":"FAKE-ONLY-replacement","reset_type":"codexRateLimits","status":"available","granted_at":1800000000,"expires_at":4102444800}]}"#).expect("isolated reset fixture");
	fake.inventory.lock().expect("isolated reset fixture").credits = replacement.credits;
	runtime.process_pending().await;
	assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
	assert_eq!(
		runtime.operation_status("manual-1").await.expect("isolated reset fixture"),
		ResetCardOperationStatus::FailedBeforeEffect(ResetCardFailureCode::InventoryChanged)
	);
}
#[tokio::test]
async fn incomplete_duplicate_and_stale_inventory_never_send() {
	let (_dir, _store, fake, runtime, account, descriptor) = fixture();
	fake.inventory.lock().expect("isolated reset fixture").details_complete = false;
	assert_eq!(
		runtime
			.prepare("manual-1", &account, 1, descriptor)
			.await
			.expect_err("expected safe refusal"),
		ResetCardServiceError::InventoryIncomplete
	);
	{
		let mut inventory = fake.inventory.lock().expect("isolated reset fixture");
		inventory.details_complete = true;
		let credit = inventory.credits[0].clone();
		inventory.credits.push(credit);
		inventory.reported_available_count = Some(2);
	}
	assert_eq!(
		runtime
			.prepare("manual-1", &account, 1, descriptor)
			.await
			.expect_err("expected safe refusal"),
		ResetCardServiceError::InventoryChanged
	);
	fake.inventory.lock().expect("isolated reset fixture").account_revision = 2;
	assert_eq!(
		runtime
			.prepare("manual-1", &account, 1, descriptor)
			.await
			.expect_err("expected safe refusal"),
		ResetCardServiceError::AccountChanged
	);
	assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn durable_receipt_finishes_after_restart_without_a_provider_session() {
	let (dir, store, fake, runtime, account, descriptor) = fixture();
	runtime.prepare("manual-1", &account, 1, descriptor).await.expect("isolated reset fixture");
	assert!(store.begin_reset_card_send("manual-1".into()).await.expect("isolated reset fixture"));
	store
		.record_reset_card_outcome("manual-1".into(), "already_redeemed".into())
		.await
		.expect("isolated reset fixture");
	drop(runtime);
	drop(store);
	let paths = DecodexRoot::new(dir.path().canonicalize().expect("canonical temporary root"))
		.expect("temporary root")
		.paths();
	let runtime = ApiResetCardRuntime::with_provider(
		SqliteStore::open(&paths).expect("reopened ledger"),
		Arc::new(Arc::clone(&fake)),
	);
	fake.unavailable.store(true, Ordering::Release);
	runtime.process_pending().await;
	assert_eq!(
		runtime.operation_status("manual-1").await.expect("isolated reset fixture"),
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::AlreadyRedeemed)
	);
	assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn shutdown_preserves_prepared_work_without_sending() {
	let (_dir, _store, fake, runtime, account, descriptor) = fixture();
	runtime.prepare("manual-1", &account, 1, descriptor).await.expect("isolated reset fixture");
	runtime.begin_shutdown();
	runtime.process_pending().await;
	runtime.wait_for_shutdown().await;
	assert_eq!(
		runtime.operation_status("manual-1").await.expect("isolated reset fixture"),
		ResetCardOperationStatus::Prepared
	);
	assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn competing_workers_and_double_clicks_send_only_once() {
	let (_dir, store, fake, runtime, account, descriptor) = fixture();
	let (first, second) = tokio::join!(
		runtime.prepare("manual-1", &account, 1, descriptor),
		runtime.prepare("manual-1", &account, 1, descriptor),
	);
	first.expect("accepted first confirmation");
	second.expect("same confirmation replay");
	let other_owner = ApiResetCardRuntime::with_provider(store, Arc::new(Arc::clone(&fake)));
	tokio::join!(runtime.process_pending(), other_owner.process_pending());
	assert_eq!(fake.sends.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn shutdown_drains_a_send_without_cancelling_or_replaying_it() {
	let (_dir, _store, fake, runtime, account, descriptor) = fixture();
	fake.pause_send.store(true, Ordering::Release);
	runtime.prepare("manual-1", &account, 1, descriptor).await.expect("prepare fake credit");
	let worker = runtime.clone();
	let task = tokio::spawn(async move {
		worker.process_pending().await;
	});
	tokio::time::timeout(Duration::from_secs(2), fake.entered.notified())
		.await
		.expect("fake send started");
	runtime.begin_shutdown();
	assert!(
		tokio::time::timeout(Duration::from_millis(20), runtime.wait_for_shutdown()).await.is_err()
	);
	fake.release.notify_one();
	task.await.expect("drained fake send");
	runtime.wait_for_shutdown().await;
	assert_eq!(fake.sends.load(Ordering::SeqCst), 1);
	assert_eq!(
		runtime.operation_status("manual-1").await.expect("receipt"),
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::Reset)
	);
}
#[tokio::test]
async fn account_recovery_discovers_the_original_key_without_provider_access() {
	let (_dir, _store, fake, runtime, account, descriptor) = fixture();
	runtime.prepare("manual-1", &account, 1, descriptor).await.expect("prepare fake credit");
	fake.unavailable.store(true, Ordering::Release);
	let operation =
		runtime.latest_operation(&account).await.expect("ledger read").expect("saved operation");
	assert_eq!(operation.key, "manual-1");
	assert_eq!(operation.account_revision, 1);
	runtime.process_pending().await;
	assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
	assert_eq!(
		runtime.operation_status("manual-1").await.expect("receipt"),
		ResetCardOperationStatus::FailedBeforeEffect(ResetCardFailureCode::AccountChanged)
	);
}
#[tokio::test]
async fn expired_cards_and_known_no_effect_outcomes_remain_distinct() {
	let (_dir, _store, fake, runtime, account, _) = fixture();
	let expired = decodex_core::ResetCardDescriptor::new(
		decodex_core::ResetCardTimestamp::from_unix_seconds(1).expect("grant"),
		decodex_core::ResetCardTimestamp::from_unix_seconds(2).expect("expiry"),
	)
	.expect("descriptor");
	assert_eq!(
		runtime.prepare("expired", &account, 1, expired).await.expect_err("expired"),
		ResetCardServiceError::InventoryChanged
	);
	assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
	for outcome in [ResetCardConsumeOutcome::NothingToReset, ResetCardConsumeOutcome::NoCredit] {
		let (_dir, _store, fake, runtime, account, descriptor) = fixture();
		*fake.outcome.lock().expect("fake result") = Ok(outcome);
		runtime.prepare("manual-1", &account, 1, descriptor).await.expect("prepare");
		runtime.process_pending().await;
		assert_eq!(
			runtime.operation_status("manual-1").await.expect("receipt"),
			ResetCardOperationStatus::Completed(outcome)
		);
		assert_eq!(fake.sends.load(Ordering::SeqCst), 1);
	}
}
