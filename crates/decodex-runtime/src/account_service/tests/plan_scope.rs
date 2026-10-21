use super::{
	AccountCommandKind, AccountCommandReceiptClaim, AccountId, AccountLifecycleError,
	AccountOperationId, AccountOperationKind, AccountOperationPhase, AccountOperationPreparation,
	AccountProvider, AccountService, AccountUsageObservation, Arc, CommandIdentity,
	CredentialBinding, CredentialSecretBundle, CredentialVersion, DecodexRoot, ProviderIdentity,
	SqliteCredentialStore, SqliteStore, TempDir, UnusedCredentialRefresher, account_service, fs,
	identity_token,
};

const SUPPORTED: &[&str] = &["plus", "pro", "prolite", "promax", "team", "PLUS"];
const EXCLUDED: &[Option<&str>] = &[
	None,
	Some(""),
	Some("free"),
	Some("go"),
	Some("business"),
	Some("self_serve_business_prolite"),
	Some("self_serve_business_usage_based"),
	Some("ent26"),
	Some("enterprise"),
	Some("enterprise_cbp_automation"),
	Some("enterprise_cbp_usage_based"),
	Some("hc"),
	Some("edu"),
	Some("education"),
	Some("edu_plus"),
	Some("edu_pro"),
	Some("sci"),
	Some("sci_plus"),
	Some("future-plan"),
];

fn bundle(provider: &str, plan: Option<&str>, pat: bool) -> CredentialSecretBundle {
	if pat {
		CredentialSecretBundle::personal_access_token(
			"synthetic-pat".into(),
			"synthetic-user".into(),
			plan.map(str::to_owned),
			None,
		)
		.unwrap()
	} else {
		CredentialSecretBundle::chatgpt(
			"synthetic-access".into(),
			"synthetic-refresh".into(),
			plan.map(|plan| identity_token(provider, "fixture@example.invalid", plan)),
			plan.map(str::to_owned),
			None,
			"bearer".into(),
			i64::MAX,
		)
		.unwrap()
	}
}

async fn fixture() -> (TempDir, SqliteStore, AccountService) {
	let directory = tempfile::tempdir().unwrap();
	let root = DecodexRoot::new(fs::canonicalize(directory.path()).unwrap()).unwrap();
	let store = SqliteStore::open(&root.paths()).unwrap();
	let service = AccountService::new(
		store.clone(),
		Arc::new(SqliteCredentialStore::new(store.clone())),
		Arc::new(UnusedCredentialRefresher),
	)
	.with_shared_auth_coordinator(super::test_coordinator(
		Arc::new(super::UnmanagedSharedAuthFile),
		super::CodexLiveness::Quiescent,
	));

	service
		.attest_callback_capability(account_service::CodexAccountCapabilityAttestation {
			build_identity: "plan-scope-fixture".into(),
			executable_sha256: "1".repeat(64),
			schema_sha256: "2".repeat(64),
			callback_profile_sha256: "3".repeat(64),
			login_chatgpt_auth_tokens: true,
			refresh_callback: true,
		})
		.await
		.unwrap();

	(directory, store, service)
}

fn account_id(index: usize) -> AccountId {
	AccountId::new(format!("21000000-0000-4000-8000-{index:012}")).unwrap()
}

fn operation_id(index: usize) -> AccountOperationId {
	AccountOperationId::new(format!("22000000-0000-4000-8000-{index:012}")).unwrap()
}

#[tokio::test]
async fn only_selected_plans_can_be_installed_with_oauth_or_pat() {
	for pat in [false, true] {
		let (_directory, store, service) = fixture().await;

		for (index, plan) in
			SUPPORTED.iter().copied().map(Some).chain(EXCLUDED.iter().copied()).enumerate()
		{
			let allowed = index < SUPPORTED.len();
			let account = account_id(index + 1);
			let operation = operation_id(index + 1);
			let provider =
				ProviderIdentity::new(AccountProvider::Chatgpt, format!("provider-{index}"))
					.unwrap();
			let command =
				CommandIdentity::new(format!("plan-import-{index}"), b"plan admission").unwrap();
			let AccountCommandReceiptClaim::Owned(lease) = store
				.reserve_account_command(
					&command,
					AccountCommandKind::Import,
					account.as_str(),
					None,
				)
				.await
				.unwrap()
			else {
				panic!("fresh command")
			};
			let response = service
				.install_credentials_command(
					lease,
					operation.clone(),
					account.clone(),
					AccountOperationKind::Import,
					account_service::stable_account_alias(&provider),
					true,
					provider.clone(),
					bundle(provider.account_id(), plan, pat),
					|result| Ok(serde_json::json!({"accepted":result.is_ok()})),
				)
				.await
				.unwrap();

			assert_eq!(response["accepted"], allowed, "plan {plan:?}, PAT {pat}");

			let recorded = store.read_account_operation(&operation).await.unwrap();
			assert_eq!(recorded.is_some(), allowed);
			let replay = store
				.reserve_account_command(
					&command,
					AccountCommandKind::Import,
					account.as_str(),
					None,
				)
				.await
				.unwrap();
			assert!(
				matches!(replay, AccountCommandReceiptClaim::Replayed(value) if value == response)
			);
		}
	}
}

// Seed the old persisted format directly: admission policy must not make an old
// credential unreadable, delete it, or allow it to escape through another caller.
pub(super) async fn legacy_account(
	store: &SqliteStore,
	service: &AccountService,
	index: usize,
	plan: Option<&str>,
	pat: bool,
) -> (AccountId, CredentialBinding) {
	let account = account_id(index);
	let operation = operation_id(index);
	let provider =
		ProviderIdentity::new(AccountProvider::Chatgpt, format!("legacy-provider-{index}"))
			.unwrap();
	let secret = bundle(provider.account_id(), plan, pat);
	let binding = secret
		.binding_for(&account, &operation, CredentialVersion::new(1).unwrap(), &provider)
		.unwrap();
	account_service::accepted_phase(
		store
			.prepare_account_operation(&AccountOperationPreparation {
				operation_id: operation.clone(),
				account_id: account.clone(),
				kind: AccountOperationKind::Enroll,
				display_label: Some(account_service::stable_account_alias(&provider)),
				enabled: Some(true),
				expected_account_revision: None,
				expected: None,
				target: Some(binding.clone()),
				provider,
			})
			.await
			.unwrap(),
	)
	.unwrap();
	service.credentials.create(&account, &binding, secret).unwrap();
	for (before, after) in [
		(AccountOperationPhase::Prepared, AccountOperationPhase::StoreApplied),
		(AccountOperationPhase::StoreApplied, AccountOperationPhase::Committed),
	] {
		account_service::accepted_phase(
			store.advance_account_operation(&operation, before, after, None).await.unwrap(),
		)
		.unwrap();
	}
	let current = service.inspect(&account).await.unwrap().account;
	let now = account_service::current_unix_micros().unwrap();
	service
		.observe_usage(
			&account,
			AccountUsageObservation {
				account_revision: current.revision,
				observed_at_unix_micros: now,
				ordinary_usage_allowed: Some(true),
				conditions: Default::default(),
			},
			[None, None],
		)
		.await
		.unwrap();
	(account, binding)
}

#[tokio::test]
async fn old_excluded_accounts_remain_readable_but_cannot_supply_work() {
	for (plan, pat) in [(Some("free"), false), (Some("sci_plus"), true), (None, false)] {
		let (_directory, store, service) = fixture().await;
		let (excluded, binding) = legacy_account(&store, &service, 1, plan, pat).await;
		let (supported, _) = legacy_account(&store, &service, 2, Some("team"), false).await;
		let before = service.inspect(&excluded).await.unwrap().account;

		assert!(
			service
				.api_credential_for_observation(&excluded, std::time::Duration::ZERO)
				.await
				.is_err(),
			"excluded observation {plan:?}"
		);

		let command = CommandIdentity::new("unsupported-route", b"route old account").unwrap();
		let AccountCommandReceiptClaim::Owned(lease) = store
			.reserve_account_command(&command, AccountCommandKind::Route, excluded.as_str(), None)
			.await
			.unwrap()
		else {
			panic!("fresh route command")
		};
		let routing_before = service.routing_control().await.unwrap();
		let response = service.route_account_command_sync(lease, &excluded, |result| {
			Ok(serde_json::json!({"unsupported":matches!(result, Err(account_service::AccountRouteFailure::Lifecycle(AccountLifecycleError::UnsupportedPlan)))}))
		}).await.unwrap();

		assert_eq!(response["unsupported"], true);
		assert_eq!(service.routing_control().await.unwrap(), routing_before);
		assert!(
			service.process_credential(&excluded, before.revision).await.is_err(),
			"excluded process {plan:?}"
		);
		assert!(service.credential_for_launch(&excluded, before.revision, &binding).await.is_err());

		let now = account_service::current_unix_micros().unwrap();
		assert_eq!(service.select_initial(now).await.unwrap().account.account_id, supported);
		assert_eq!(
			service.select_agent_route(Some(&excluded), now).await.unwrap().account.account_id,
			supported
		);
		assert!(
			service
				.process_credential(
					&supported,
					service.inspect(&supported).await.unwrap().account.revision
				)
				.await
				.is_ok()
		);

		let retained = service.credentials.read_exact(&excluded, &binding).unwrap();
		assert_eq!(retained.bundle().plan_type(), plan);
		assert_eq!(service.inspect(&excluded).await.unwrap().account, before);
	}
}

#[tokio::test]
async fn changed_plan_keeps_rotated_credentials_and_finishes_the_refusal_receipt() {
	for pat in [false, true] {
		let (_directory, store, service) = fixture().await;
		let (account, binding) = legacy_account(&store, &service, 1, Some("pro"), pat).await;
		let before = service.inspect(&account).await.unwrap().account;
		let operation = operation_id(2);
		let command = CommandIdentity::new("plan-refresh", b"refresh account").unwrap();
		let AccountCommandReceiptClaim::Owned(lease) = store
			.reserve_account_command(
				&command,
				AccountCommandKind::Refresh,
				account.as_str(),
				Some(before.revision),
			)
			.await
			.unwrap()
		else {
			panic!("fresh refresh command")
		};
		account_service::accepted_phase(
			store
				.prepare_account_operation(&AccountOperationPreparation {
					operation_id: operation.clone(),
					account_id: account.clone(),
					kind: AccountOperationKind::Refresh,
					display_label: None,
					enabled: None,
					expected_account_revision: Some(before.revision),
					expected: Some(binding.clone()),
					target: None,
					provider: binding.provider.clone(),
				})
				.await
				.unwrap(),
		)
		.unwrap();
		account_service::accepted_phase(
			store
				.advance_account_operation(
					&operation,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::ProviderEffectPending,
					None,
				)
				.await
				.unwrap(),
		)
		.unwrap();
		let refreshed = account_service::CredentialRefreshResult {
			returned_provider: binding.provider.clone(),
			bundle: bundle(binding.provider.account_id(), Some("sci_plus"), pat),
		};
		let response = service.commit_refresh_command(
			lease, &operation, &account, &binding,
			account_service::RefreshResolution::Rotate { refreshed, projected_source: None },
			|result| Ok(serde_json::json!({"unsupported":matches!(result, Err(AccountLifecycleError::UnsupportedPlan))})),
		).await.unwrap();

		assert_eq!(response["unsupported"], true);
		let after = service.inspect(&account).await.unwrap().account;
		let successor = after.credential.unwrap();
		assert_eq!(successor.version.get(), 2);
		assert_eq!(
			service.credentials.read_exact(&account, &successor).unwrap().bundle().plan_type(),
			Some("sci_plus")
		);
		assert!(after.unsettled_operation.is_none());
		assert_eq!(
			store.read_account_operation(&operation).await.unwrap().unwrap().phase,
			AccountOperationPhase::Committed
		);
		assert!(
			matches!(store.reserve_account_command(&command, AccountCommandKind::Refresh, account.as_str(), Some(before.revision)).await.unwrap(), AccountCommandReceiptClaim::Replayed(saved) if saved == response)
		);
		assert!(matches!(
			service.api_credential_for_observation(&account, std::time::Duration::ZERO).await,
			Err(AccountLifecycleError::UnsupportedPlan)
		));
	}
}

#[tokio::test]
async fn excluded_reauthentication_keeps_the_prior_supported_credential() {
	let (_directory, store, service) = fixture().await;
	let (account, binding) = legacy_account(&store, &service, 1, Some("pro"), false).await;
	let before = service.inspect(&account).await.unwrap().account;
	let (_source, path) = super::owner_private_shared_codex_auth(
		binding.provider.account_id(),
		"fixture@example.invalid",
	);
	let mut source: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
	source["tokens"]["id_token"] =
		identity_token(binding.provider.account_id(), "fixture@example.invalid", "edu").into();
	fs::write(&path, serde_json::to_vec(&source).unwrap()).unwrap();
	let operation = operation_id(2);
	let command = CommandIdentity::new("unsupported-reauth", b"reauthenticate account").unwrap();
	let AccountCommandReceiptClaim::Owned(lease) = store
		.reserve_account_command(
			&command,
			AccountCommandKind::Refresh,
			account.as_str(),
			Some(before.revision),
		)
		.await
		.unwrap()
	else {
		panic!("fresh reauthentication command")
	};
	let response = service.reauthenticate_from_credential_file_command(
		lease, operation.clone(), &account, before.revision, None, &path,
		|result| Ok(serde_json::json!({"unsupported":matches!(result, Err(AccountLifecycleError::UnsupportedPlan))})),
	).await.unwrap();

	assert_eq!(response["unsupported"], true);
	assert_eq!(service.inspect(&account).await.unwrap().account, before);
	assert!(store.read_account_operation(&operation).await.unwrap().is_none());
	assert_eq!(
		service.credentials.read_exact(&account, &binding).unwrap().bundle().plan_type(),
		Some("pro")
	);
}
