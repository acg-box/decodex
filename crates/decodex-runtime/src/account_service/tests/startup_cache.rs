use super::{
	AccountCommandKind, AccountCommandReceiptClaim, AccountId, AccountService,
	AccountUsageObservation, Arc, CommandIdentity, CredentialRefreshError, DecodexRoot,
	SqliteCredentialStore, SqliteStore, TempDir, UnusedCredentialRefresher, account_service,
	observation_service_with_expiry,
};
use std::{sync::atomic::Ordering, time::Duration};

const VALIDITY: Duration = Duration::from_secs(30);

async fn fixture() -> (TempDir, SqliteStore, AccountService, AccountId, i64) {
	let (directory, store, service, account, _) =
		observation_service_with_expiry(Err(CredentialRefreshError::Unavailable), i64::MAX).await;
	service
		.attest_callback_capability(account_service::CodexAccountCapabilityAttestation {
			build_identity: "startup-cache-fixture".into(),
			executable_sha256: "1".repeat(64),
			schema_sha256: "2".repeat(64),
			callback_profile_sha256: "3".repeat(64),
			login_chatgpt_auth_tokens: true,
			refresh_callback: true,
		})
		.await
		.unwrap();
	let now = account_service::current_unix_micros().unwrap();
	observe(&service, &account, now, true).await;
	(directory, store, service, account, now)
}

async fn observe(service: &AccountService, account: &AccountId, now: i64, allowed: bool) {
	let revision = service.inspect(account).await.unwrap().account.revision;
	assert!(
		service
			.observe_usage(
				account,
				AccountUsageObservation {
					account_revision: revision,
					observed_at_unix_micros: now,
					ordinary_usage_allowed: Some(allowed),
					conditions: Default::default(),
				},
				[None, None],
			)
			.await
			.unwrap()
	);
}

#[tokio::test]
async fn current_observation_survives_restart_without_reusing_native_authority() {
	let (directory, store, service, account, now) = fixture().await;
	assert!(service.reusable_agent_observation(&account, now, VALIDITY).await);
	drop(service);
	drop(store);
	let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
	let store = SqliteStore::open(&root.paths()).unwrap();
	let restarted = AccountService::new(
		store.clone(),
		Arc::new(SqliteCredentialStore::new(store)),
		Arc::new(UnusedCredentialRefresher),
	);
	assert!(!restarted.callback_ready.load(Ordering::Acquire));
	assert!(restarted.reusable_agent_observation(&account, now, VALIDITY).await);
	assert!(restarted.select_agent_route(Some(&account), now).await.is_err());
	let observations = super::AccountObservationService::new(Arc::new(restarted), None, None, None);
	assert!(observations.can_reuse_initial_observation(Some(&account)).await);
	assert!(!observations.can_reuse_initial_observation(None).await);
}

#[tokio::test]
async fn cached_observation_rejects_expiry_clock_reversal_and_provider_denial() {
	let (_directory, _store, service, account, now) = fixture().await;
	assert!(service.reusable_agent_observation(&account, now + 270_000_000, VALIDITY).await);
	assert!(!service.reusable_agent_observation(&account, now + 270_000_001, VALIDITY).await);
	assert!(!service.reusable_agent_observation(&account, now - 1, VALIDITY).await);
	observe(&service, &account, now + 1, false).await;
	assert!(!service.reusable_agent_observation(&account, now + 1, VALIDITY).await);
}

#[tokio::test]
async fn cached_observation_tracks_account_revision_and_exact_credential() {
	let (_directory, store, service, account, now) = fixture().await;
	assert!(service.reusable_agent_observation(&account, now, VALIDITY).await);
	for enabled in [false, true] {
		let revision = service.inspect(&account).await.unwrap().account.revision;
		let identity = CommandIdentity::new(
			format!("startup-cache-enabled-{enabled}"),
			b"startup cache invalidation fixture",
		)
		.unwrap();
		let AccountCommandReceiptClaim::Owned(lease) = store
			.reserve_account_command(
				&identity,
				AccountCommandKind::SetEnabled,
				account.as_str(),
				Some(revision),
			)
			.await
			.unwrap()
		else {
			panic!("fresh administration owner")
		};
		service
			.set_account_enabled_command(lease, &account, revision, enabled, move |_, _| {
				Ok(serde_json::json!({"enabled": enabled}))
			})
			.await
			.unwrap();
		assert!(!service.reusable_agent_observation(&account, now, VALIDITY).await);
	}
	let refreshed_at = account_service::current_unix_micros().unwrap();
	observe(&service, &account, refreshed_at, true).await;
	assert!(service.reusable_agent_observation(&account, refreshed_at, VALIDITY).await);
	let binding = service.inspect(&account).await.unwrap().account.credential.unwrap();
	service.credentials.delete(&account, &binding).unwrap();
	assert!(!service.reusable_agent_observation(&account, now, VALIDITY).await);
}
