//! Singleton daemon owner for provider authorization and AccountService installation.

use std::{
	path::Path,
	sync::{
		Arc, Mutex, MutexGuard,
		atomic::{AtomicBool, Ordering},
	},
	thread::{self, JoinHandle},
	time::Duration,
};

use decodex_account_login::{
	Cancellation, Config, Error as ProviderError, LoginEvent, LoginHome,
	LoginMethod as ProviderLoginMethod, cleanup_stale_login_homes, run as run_provider_login,
};
use decodex_core::{AccountId, AccountOperationId};
use decodex_database::{
	AccountCommandKind, AccountCommandReceiptClaim, CommandIdentity, SqliteStore,
};
use decodex_protocol::{
	AccountCommandRejectionDto, AccountLoginFailure, AccountLoginInstallMode, AccountLoginMethod,
	AccountLoginPrompt, AccountLoginRequest, AccountLoginStart, AccountLoginState,
	AccountLoginStatus, AccountLoginUrl, CommandError, EntityId, ResultPayload, WireText,
};
use serde::Serialize;
use tokio::runtime::Handle;

use crate::{
	account_observation::AccountObservationService,
	account_service::{AccountLifecycleError, AccountService},
	application::{
		account_changed_publication, account_enrollment_publication,
		account_lifecycle_command_error, decode_account_command_receipt,
		encode_account_command_receipt, map_account_store_command_error as map_store_error,
	},
};

const INSTALL_DISPATCH_ATTEMPTS: usize = 3;
const INSTALL_REPLAY_DELAY: Duration = Duration::from_millis(100);
const LOGIN_INSTALL_IDENTITY_SCHEMA: &str = "decodex/account-login-install/1";

struct Shared {
	status: Mutex<AccountLoginStatus>,
	cancellation: Cancellation,
}

impl Shared {
	fn new(session_id: EntityId, method: AccountLoginMethod) -> Self {
		let state = match method {
			AccountLoginMethod::BrowserRedirect => AccountLoginState::OpeningBrowser,
			AccountLoginMethod::DeviceCode => AccountLoginState::RequestingCode,
		};
		Self {
			status: Mutex::new(status(session_id, state, None, None, None, None)),
			cancellation: Cancellation::default(),
		}
	}

	fn status(&self) -> AccountLoginStatus {
		self.lock_status().clone()
	}

	fn set_status(&self, status: AccountLoginStatus) {
		*self.lock_status() = status;
	}

	fn lock_status(&self) -> MutexGuard<'_, AccountLoginStatus> {
		self.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
	}
}

struct Session {
	start: AccountLoginStart,
	shared: Arc<Shared>,
	worker: Option<JoinHandle<()>>,
}

/// One daemon-lifetime owner that enforces a single global login session.
pub(crate) struct AccountLoginManager {
	operation: tokio::sync::Mutex<()>,
	session: Mutex<Option<Session>>,
	closed: AtomicBool,
	authority: Option<AccountLoginInstallAuthority>,
	provider: Option<Config>,
}

impl AccountLoginManager {
	pub(crate) fn new(
		store: SqliteStore,
		accounts: Arc<AccountService>,
		observations: Option<AccountObservationService>,
	) -> Self {
		let provider = cleanup_stale_login_homes().and_then(|()| Config::production()).ok();
		Self {
			operation: tokio::sync::Mutex::new(()),
			session: Mutex::new(None),
			closed: AtomicBool::new(false),
			authority: Some(AccountLoginInstallAuthority { store, accounts, observations }),
			provider,
		}
	}

	pub(crate) async fn handle(
		&self,
		request: &AccountLoginRequest,
		runtime: Handle,
	) -> AccountLoginStatus {
		let _operation = self.operation.lock().await;
		match request {
			AccountLoginRequest::Start { start } => self.start((**start).clone(), runtime).await,
			AccountLoginRequest::Status { session_id } => self.poll(session_id),
			AccountLoginRequest::Cancel { session_id } => self.cancel(session_id).await,
		}
	}

	async fn start(&self, start: AccountLoginStart, runtime: Handle) -> AccountLoginStatus {
		if self.closed.load(Ordering::Acquire) {
			return failed(start.session_id, AccountLoginFailure::ServiceUnavailable);
		}
		let previous_worker = {
			let mut slot = self.lock_session();
			if let Some(session) = slot.as_mut() {
				if session.start.session_id == start.session_id {
					return if session.start == start {
						session.shared.status()
					} else {
						failed(start.session_id, AccountLoginFailure::Busy)
					};
				}
				if !is_terminal(&session.shared.status()) {
					return failed(start.session_id, AccountLoginFailure::Busy);
				}
				session.worker.take()
			} else {
				None
			}
		};
		if let Some(worker) = previous_worker
			&& !join_worker(worker).await
		{
			return failed(start.session_id, AccountLoginFailure::ServiceUnavailable);
		}
		if self.closed.load(Ordering::Acquire) {
			return failed(start.session_id, AccountLoginFailure::ServiceUnavailable);
		}

		let shared = Arc::new(Shared::new(start.session_id.clone(), start.method));
		let worker_shared = Arc::clone(&shared);
		let worker_start = start.clone();
		let authority = self.authority.clone();
		let provider = self.provider.clone();
		let worker =
			thread::Builder::new().name("decodexd-account-login".to_owned()).spawn(move || {
				let result = run_login_session(
					&worker_shared,
					worker_start,
					runtime,
					authority.as_ref(),
					provider.as_ref(),
				);
				worker_shared.set_status(result);
			});
		let worker = match worker {
			Ok(worker) => Some(worker),
			Err(_) => {
				shared.set_status(failed(
					start.session_id.clone(),
					AccountLoginFailure::ServiceUnavailable,
				));
				None
			},
		};
		let current = shared.status();
		let mut slot = self.lock_session();
		*slot = Some(Session { start, shared, worker });
		if self.closed.load(Ordering::Acquire)
			&& let Some(session) = slot.as_ref()
		{
			session.shared.cancellation.cancel();
		}
		current
	}

	fn poll(&self, session_id: &EntityId) -> AccountLoginStatus {
		self.lock_session()
			.as_ref()
			.filter(|session| &session.start.session_id == session_id)
			.map_or_else(
				|| failed(session_id.clone(), AccountLoginFailure::SessionNotFound),
				|session| session.shared.status(),
			)
	}

	async fn cancel(&self, session_id: &EntityId) -> AccountLoginStatus {
		let (shared, worker) = {
			let mut slot = self.lock_session();
			let Some(session) =
				slot.as_mut().filter(|session| &session.start.session_id == session_id)
			else {
				return failed(session_id.clone(), AccountLoginFailure::SessionNotFound);
			};
			session.shared.cancellation.cancel();
			(Arc::clone(&session.shared), session.worker.take())
		};
		if let Some(worker) = worker
			&& !join_worker(worker).await
		{
			shared.set_status(failed(session_id.clone(), AccountLoginFailure::ServiceUnavailable));
		}
		let current = shared.status();
		if is_terminal(&current) {
			current
		} else {
			let terminal = failed(session_id.clone(), AccountLoginFailure::ServiceUnavailable);
			shared.set_status(terminal.clone());
			terminal
		}
	}

	pub(crate) fn begin_shutdown(&self) {
		self.closed.store(true, Ordering::Release);
		if let Some(session) = self.lock_session().as_ref() {
			session.shared.cancellation.cancel();
		}
	}

	pub(crate) async fn wait_for_shutdown(&self) {
		self.begin_shutdown();
		let _operation = self.operation.lock().await;
		let (shared, session_id, worker) = {
			let mut slot = self.lock_session();
			let Some(session) = slot.as_mut() else {
				return;
			};
			(Arc::clone(&session.shared), session.start.session_id.clone(), session.worker.take())
		};
		if let Some(worker) = worker
			&& !join_worker(worker).await
		{
			shared.set_status(failed(session_id, AccountLoginFailure::ServiceUnavailable));
		}
	}

	fn lock_session(&self) -> MutexGuard<'_, Option<Session>> {
		self.session.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	#[cfg(test)]
	fn unavailable_for_test() -> Self {
		Self {
			operation: tokio::sync::Mutex::new(()),
			session: Mutex::new(None),
			closed: AtomicBool::new(false),
			authority: None,
			provider: None,
		}
	}
}

impl Drop for AccountLoginManager {
	fn drop(&mut self) {
		self.closed.store(true, Ordering::Release);
		let worker = self
			.session
			.get_mut()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.as_mut()
			.and_then(|session| {
				session.shared.cancellation.cancel();
				session.worker.take()
			});
		if let Some(worker) = worker {
			let _ = thread::Builder::new().name("decodexd-account-login-drop".to_owned()).spawn(
				move || {
					let _ = worker.join();
				},
			);
		}
	}
}

async fn join_worker(worker: JoinHandle<()>) -> bool {
	tokio::task::spawn_blocking(move || worker.join().is_ok()).await.unwrap_or(false)
}

fn run_login_session(
	shared: &Shared,
	start: AccountLoginStart,
	runtime: Handle,
	authority: Option<&AccountLoginInstallAuthority>,
	provider: Option<&Config>,
) -> AccountLoginStatus {
	let session_id = start.session_id.clone();
	let (Some(authority), Some(provider)) = (authority, provider) else {
		return failed(session_id, AccountLoginFailure::ServiceUnavailable);
	};
	let mut home = match LoginHome::create(start.session_id.as_str()) {
		Ok(home) => home,
		Err(_) => return failed(session_id, AccountLoginFailure::ServiceUnavailable),
	};
	let event_session_id = start.session_id.clone();
	let provider_method = match start.method {
		AccountLoginMethod::BrowserRedirect => ProviderLoginMethod::BrowserRedirect,
		AccountLoginMethod::DeviceCode => ProviderLoginMethod::DeviceCode,
	};
	let provider_result = run_provider_login(
		provider,
		provider_method,
		home.path(),
		&runtime,
		&shared.cancellation,
		|event| publish_provider_event(shared, &event_session_id, event),
	);
	if let Err(error) = provider_result {
		let current = shared.status();
		let status = if is_terminal(&current) {
			current
		} else if error == ProviderError::Cancelled {
			status(session_id.clone(), AccountLoginState::Cancelled, None, None, None, None)
		} else {
			failed(session_id.clone(), map_provider_error(error))
		};
		return finalize_login_status(&mut home, session_id, status);
	}
	let credential_path = match home.credential_path() {
		Ok(path) => path,
		Err(_) => {
			let status = failed(session_id.clone(), AccountLoginFailure::LoginFailed);
			return finalize_login_status(&mut home, session_id, status);
		},
	};
	if shared.cancellation.is_cancelled() {
		let status =
			status(session_id.clone(), AccountLoginState::Cancelled, None, None, None, None);
		return finalize_login_status(&mut home, session_id, status);
	}
	shared.set_status(status(
		session_id.clone(),
		AccountLoginState::Installing,
		None,
		None,
		None,
		None,
	));
	let installed = runtime.block_on(authority.install(&start, &credential_path));
	let terminal = match installed {
		Ok(resolved_account_id) => status(
			session_id.clone(),
			AccountLoginState::Completed,
			None,
			None,
			None,
			Some(resolved_account_id),
		),
		Err(failure) => failed(session_id.clone(), failure),
	};
	finalize_login_status(&mut home, session_id, terminal)
}

fn publish_provider_event(shared: &Shared, session_id: &EntityId, event: LoginEvent) {
	let converted = match event {
		LoginEvent::BrowserAuthorization { authorization_url } =>
			AccountLoginUrl::new(authorization_url).map(|authorization_url| {
				status(
					session_id.clone(),
					AccountLoginState::WaitingForBrowser,
					None,
					Some(authorization_url),
					None,
					None,
				)
			}),
		LoginEvent::DeviceAuthorization { verification_url, user_code } =>
			AccountLoginUrl::new(verification_url).and_then(|verification_url| {
				WireText::new(user_code).map(|user_code| {
					status(
						session_id.clone(),
						AccountLoginState::WaitingForBrowser,
						Some(AccountLoginPrompt { verification_url, user_code }),
						None,
						None,
						None,
					)
				})
			}),
	};
	match converted {
		Ok(status) => shared.set_status(status),
		Err(_) => {
			shared.set_status(failed(session_id.clone(), AccountLoginFailure::LoginFailed));
			shared.cancellation.cancel();
		},
	}
}

fn finalize_login_status(
	home: &mut LoginHome,
	session_id: EntityId,
	status: AccountLoginStatus,
) -> AccountLoginStatus {
	if home.cleanup().is_ok() {
		return status;
	}
	let failure = if status.state == AccountLoginState::Completed
		|| status.failure == Some(AccountLoginFailure::OutcomeUnknown)
	{
		AccountLoginFailure::OutcomeUnknown
	} else {
		AccountLoginFailure::ServiceUnavailable
	};
	failed(session_id, failure)
}

fn map_provider_error(error: ProviderError) -> AccountLoginFailure {
	match error {
		ProviderError::TimedOut => AccountLoginFailure::LoginTimedOut,
		ProviderError::DeviceAuthorizationRejected =>
			AccountLoginFailure::DeviceAuthorizationRejected,
		ProviderError::Persistence => AccountLoginFailure::ServiceUnavailable,
		ProviderError::Cancelled
		| ProviderError::Unavailable
		| ProviderError::Rejected
		| ProviderError::InvalidResponse => AccountLoginFailure::LoginFailed,
	}
}

#[derive(Clone)]
struct AccountLoginInstallAuthority {
	store: SqliteStore,
	accounts: Arc<AccountService>,
	observations: Option<AccountObservationService>,
}

impl AccountLoginInstallAuthority {
	async fn install(
		&self,
		start: &AccountLoginStart,
		credential_path: &Path,
	) -> Result<EntityId, AccountLoginFailure> {
		for attempt in 0..INSTALL_DISPATCH_ATTEMPTS {
			match self.install_once(start, credential_path).await {
				Ok(publication) => {
					let resolved = resolved_account_id(start, &publication.result)?;
					if let Ok(account_id) = AccountId::new(resolved.as_str())
						&& let Some(observations) = &self.observations
					{
						observations.invalidate_account(&account_id).await;
						observations.request_refresh();
					}
					return Ok(resolved);
				},
				Err(CommandError::AcceptanceUnknown) if attempt + 1 < INSTALL_DISPATCH_ATTEMPTS => {
					tokio::time::sleep(INSTALL_REPLAY_DELAY).await;
				},
				Err(CommandError::AcceptanceUnknown) => {
					return Err(AccountLoginFailure::OutcomeUnknown);
				},
				Err(error) => return Err(map_install_error(&error)),
			}
		}
		Err(AccountLoginFailure::OutcomeUnknown)
	}

	async fn install_once(
		&self,
		start: &AccountLoginStart,
		credential_path: &Path,
	) -> Result<crate::ApplicationPublication, CommandError> {
		let (identity, kind, entity_id, expected_revision) = install_identity(start)?;
		let request = serde_json::to_vec(&identity).map_err(|_| invalid_request())?;
		let idempotency_key = match &start.install_mode {
			AccountLoginInstallMode::Enroll { idempotency_key, .. }
			| AccountLoginInstallMode::Reauthenticate { idempotency_key, .. } => idempotency_key,
		};
		let identity =
			CommandIdentity::new(idempotency_key.as_str(), &request).map_err(map_store_error)?;
		let claim = self
			.store
			.reserve_account_command(&identity, kind, &entity_id, expected_revision)
			.await
			.map_err(map_store_error)?;
		let lease = match claim {
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(value)
			| AccountCommandReceiptClaim::Replayed(value) => {
				return decode_account_command_receipt(value)
					.map_err(|_| CommandError::AcceptanceUnknown)?;
			},
		};
		let source = credential_path.to_str().ok_or_else(invalid_request)?;
		let value = match &start.install_mode {
			AccountLoginInstallMode::Enroll { operation_id, account_id, enabled, .. } => {
				let operation_id = AccountOperationId::new(operation_id.as_str())
					.map_err(|_| invalid_request())?;
				let account_id =
					AccountId::new(account_id.as_str()).map_err(|_| invalid_request())?;
				let requested_account_id = account_id.clone();
				self.accounts
					.enroll_from_credential_file_command(
						lease,
						operation_id,
						account_id,
						*enabled,
						source,
						move |result| {
							encode_account_command_receipt(
								&result.map_err(account_lifecycle_command_error).and_then(
									|account| {
										account_enrollment_publication(
											&requested_account_id,
											account.clone(),
										)
									},
								),
							)
						},
					)
					.await
			},
			AccountLoginInstallMode::Reauthenticate {
				operation_id,
				account_id,
				expected_revision,
				recovery_operation_id,
				..
			} => {
				let operation_id = AccountOperationId::new(operation_id.as_str())
					.map_err(|_| invalid_request())?;
				let account_id =
					AccountId::new(account_id.as_str()).map_err(|_| invalid_request())?;
				let expected_revision = i64::try_from(expected_revision.0)
					.ok()
					.filter(|value| *value > 0)
					.ok_or_else(invalid_request)?;
				let recovery_operation_id = recovery_operation_id
					.as_ref()
					.map(|value| AccountOperationId::new(value.as_str()))
					.transpose()
					.map_err(|_| invalid_request())?;
				self.accounts
					.reauthenticate_from_credential_file_command(
						lease,
						operation_id,
						&account_id,
						expected_revision,
						recovery_operation_id.as_ref(),
						source,
						|result| {
							encode_account_command_receipt(
								&result.map_err(account_lifecycle_command_error).and_then(
									|account| account_changed_publication(account.clone()),
								),
							)
						},
					)
					.await
			},
		}
		.map_err(|_error: AccountLifecycleError| CommandError::AcceptanceUnknown)?;
		decode_account_command_receipt(value).map_err(|_| CommandError::AcceptanceUnknown)?
	}
}

#[derive(Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum DurableInstallIdentity<'a> {
	Enroll {
		schema: &'static str,
		operation_id: &'a str,
		account_id: &'a str,
		enabled: bool,
	},
	Reauthenticate {
		schema: &'static str,
		operation_id: &'a str,
		account_id: &'a str,
		expected_revision: u64,
		recovery_operation_id: Option<&'a str>,
	},
}

fn install_identity(
	start: &AccountLoginStart,
) -> Result<(DurableInstallIdentity<'_>, AccountCommandKind, String, Option<i64>), CommandError> {
	match &start.install_mode {
		AccountLoginInstallMode::Enroll { operation_id, account_id, enabled, .. } => Ok((
			DurableInstallIdentity::Enroll {
				schema: LOGIN_INSTALL_IDENTITY_SCHEMA,
				operation_id: operation_id.as_str(),
				account_id: account_id.as_str(),
				enabled: *enabled,
			},
			AccountCommandKind::Enroll,
			account_id.as_str().to_owned(),
			None,
		)),
		AccountLoginInstallMode::Reauthenticate {
			operation_id,
			account_id,
			expected_revision,
			recovery_operation_id,
			..
		} => {
			let expected = i64::try_from(expected_revision.0)
				.ok()
				.filter(|value| *value > 0)
				.ok_or_else(invalid_request)?;
			Ok((
				DurableInstallIdentity::Reauthenticate {
					schema: LOGIN_INSTALL_IDENTITY_SCHEMA,
					operation_id: operation_id.as_str(),
					account_id: account_id.as_str(),
					expected_revision: expected_revision.0,
					recovery_operation_id: recovery_operation_id.as_ref().map(EntityId::as_str),
				},
				AccountCommandKind::Refresh,
				account_id.as_str().to_owned(),
				Some(expected),
			))
		},
	}
}

fn resolved_account_id(
	start: &AccountLoginStart,
	result: &ResultPayload,
) -> Result<EntityId, AccountLoginFailure> {
	let requested = match &start.install_mode {
		AccountLoginInstallMode::Enroll { account_id, .. }
		| AccountLoginInstallMode::Reauthenticate { account_id, .. } => account_id,
	};
	match (&start.install_mode, result) {
		(AccountLoginInstallMode::Enroll { .. }, ResultPayload::AccountChanged { account })
			if &account.account_id == requested =>
			Ok(account.account_id.clone()),
		(
			AccountLoginInstallMode::Enroll { .. },
			ResultPayload::AccountRestored { requested_account_id, account },
		) if requested_account_id == requested && account.account_id != *requested =>
			Ok(account.account_id.clone()),
		(
			AccountLoginInstallMode::Reauthenticate { .. },
			ResultPayload::AccountChanged { account },
		) if &account.account_id == requested => Ok(account.account_id.clone()),
		_ => Err(AccountLoginFailure::ServiceUnavailable),
	}
}

fn map_install_error(error: &CommandError) -> AccountLoginFailure {
	match error {
		CommandError::ExpectedRevisionMismatch { .. } => AccountLoginFailure::AccountChanged,
		CommandError::AccountCommandRejected { rejection, .. } => match rejection {
			AccountCommandRejectionDto::ProviderMismatch => AccountLoginFailure::AccountMismatch,
			AccountCommandRejectionDto::ProviderAlreadyEnrolled =>
				AccountLoginFailure::ProviderAlreadyEnrolled,
			AccountCommandRejectionDto::StaleAccount => AccountLoginFailure::AccountChanged,
			AccountCommandRejectionDto::CredentialStoreUnavailable =>
				AccountLoginFailure::CredentialStoreUnavailable,
			AccountCommandRejectionDto::OperationNotFound
			| AccountCommandRejectionDto::ManualRecoveryRequired => AccountLoginFailure::RecoveryChanged,
			AccountCommandRejectionDto::AccountNotFound
			| AccountCommandRejectionDto::CredentialAbsent
			| AccountCommandRejectionDto::LifecycleUnready
			| AccountCommandRejectionDto::SharedAuthOwnerBusy
			| AccountCommandRejectionDto::OperationUnsettled
			| AccountCommandRejectionDto::InvalidRequest
			| AccountCommandRejectionDto::AccountInUse
			| AccountCommandRejectionDto::RoutingOrderInvalid => AccountLoginFailure::AccountUnavailable,
			AccountCommandRejectionDto::StaleRoutingControl
			| AccountCommandRejectionDto::RouteSuperseded => AccountLoginFailure::AccountChanged,
		},
		CommandError::AcceptanceUnknown => AccountLoginFailure::OutcomeUnknown,
		CommandError::IdempotencyConflict
		| CommandError::IdempotencyCapacityExceeded { .. }
		| CommandError::ApplicationUnavailable { .. }
		| CommandError::ConversationUnavailable { .. }
		| CommandError::ConversationRecoveryRequired { .. } => AccountLoginFailure::ServiceUnavailable,
	}
}

fn invalid_request() -> CommandError {
	CommandError::AccountCommandRejected {
		rejection: AccountCommandRejectionDto::InvalidRequest,
		actual_revision: None,
	}
}

fn status(
	session_id: EntityId,
	state: AccountLoginState,
	prompt: Option<AccountLoginPrompt>,
	authorization_url: Option<AccountLoginUrl>,
	failure: Option<AccountLoginFailure>,
	resolved_account_id: Option<EntityId>,
) -> AccountLoginStatus {
	let status = AccountLoginStatus {
		session_id,
		state,
		prompt,
		authorization_url,
		failure,
		resolved_account_id,
	};
	debug_assert!(status.validate().is_ok());
	status
}

fn failed(session_id: EntityId, failure: AccountLoginFailure) -> AccountLoginStatus {
	status(session_id, AccountLoginState::Failed, None, None, Some(failure), None)
}

pub(crate) fn unavailable_status(request: &AccountLoginRequest) -> AccountLoginStatus {
	failed(request.session_id().clone(), AccountLoginFailure::ServiceUnavailable)
}

fn is_terminal(status: &AccountLoginStatus) -> bool {
	matches!(
		status.state,
		AccountLoginState::Completed | AccountLoginState::Failed | AccountLoginState::Cancelled
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::{EntityRevision, IdempotencyKey};
	use std::sync::atomic::AtomicBool;

	fn entity(value: &str) -> EntityId {
		EntityId::new(value).expect("fixture identity")
	}

	fn start(session_id: &str, method: AccountLoginMethod) -> AccountLoginStart {
		AccountLoginStart {
			session_id: entity(session_id),
			method,
			install_mode: AccountLoginInstallMode::Reauthenticate {
				operation_id: entity("028f0f9e-7b6e-4a31-8f4c-1d2e3f405163"),
				account_id: entity("038f0f9e-7b6e-4a31-8f4c-1d2e3f405164"),
				expected_revision: EntityRevision(7),
				recovery_operation_id: Some(entity("048f0f9e-7b6e-4a31-8f4c-1d2e3f405165")),
				idempotency_key: IdempotencyKey::new("account-login-fixture")
					.expect("fixture idempotency key"),
			},
		}
	}

	#[test]
	fn both_methods_start_in_distinct_closed_states() {
		let browser = Shared::new(
			entity("018f0f9e-7b6e-4a31-8f4c-1d2e3f405162"),
			AccountLoginMethod::BrowserRedirect,
		)
		.status();
		let device = Shared::new(
			entity("028f0f9e-7b6e-4a31-8f4c-1d2e3f405163"),
			AccountLoginMethod::DeviceCode,
		)
		.status();

		assert_eq!(browser.state, AccountLoginState::OpeningBrowser);
		assert_eq!(device.state, AccountLoginState::RequestingCode);
	}

	#[tokio::test(flavor = "current_thread")]
	async fn singleton_returns_same_session_and_rejects_another_active_session() {
		let manager = AccountLoginManager::unavailable_for_test();
		let first =
			start("018f0f9e-7b6e-4a31-8f4c-1d2e3f405162", AccountLoginMethod::BrowserRedirect);
		let shared = Arc::new(Shared::new(first.session_id.clone(), first.method));
		*manager.lock_session() =
			Some(Session { start: first.clone(), shared: Arc::clone(&shared), worker: None });
		let runtime = Handle::current();

		assert_eq!(manager.start(first.clone(), runtime.clone()).await, shared.status());
		let second = start("058f0f9e-7b6e-4a31-8f4c-1d2e3f405166", AccountLoginMethod::DeviceCode);
		assert_eq!(
			manager.start(second.clone(), runtime).await.failure,
			Some(AccountLoginFailure::Busy)
		);
	}

	#[tokio::test(flavor = "current_thread")]
	async fn cancel_joins_off_runtime_after_terminal_cleanup() {
		let manager = AccountLoginManager::unavailable_for_test();
		let start =
			start("068f0f9e-7b6e-4a31-8f4c-1d2e3f405167", AccountLoginMethod::BrowserRedirect);
		let shared = Arc::new(Shared::new(start.session_id.clone(), start.method));
		let worker_shared = Arc::clone(&shared);
		let worker_session_id = start.session_id.clone();
		let runtime = Handle::current();
		let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
		let worker = thread::spawn(move || {
			while !worker_shared.cancellation.is_cancelled() {
				thread::yield_now();
			}
			runtime.block_on(async {
				let _ = release_receiver.await;
			});
			worker_shared.set_status(status(
				worker_session_id,
				AccountLoginState::Cancelled,
				None,
				None,
				None,
				None,
			));
		});
		*manager.lock_session() =
			Some(Session { start: start.clone(), shared, worker: Some(worker) });

		tokio::spawn(async move {
			tokio::task::yield_now().await;
			let _ = release_sender.send(());
		});
		let result =
			tokio::time::timeout(Duration::from_secs(1), manager.cancel(&start.session_id))
				.await
				.expect("current-thread cancellation must not deadlock");

		assert_eq!(result.state, AccountLoginState::Cancelled);
		assert!(manager.lock_session().as_ref().is_some_and(|session| session.worker.is_none()));
	}

	#[tokio::test(flavor = "current_thread")]
	async fn shutdown_signals_then_joins_off_runtime() {
		let manager = AccountLoginManager::unavailable_for_test();
		let start = start("088f0f9e-7b6e-4a31-8f4c-1d2e3f405169", AccountLoginMethod::DeviceCode);
		let shared = Arc::new(Shared::new(start.session_id.clone(), start.method));
		let worker_shared = Arc::clone(&shared);
		let runtime = Handle::current();
		let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
		let worker = thread::spawn(move || {
			while !worker_shared.cancellation.is_cancelled() {
				thread::yield_now();
			}
			runtime.block_on(async {
				let _ = release_receiver.await;
			});
		});
		*manager.lock_session() = Some(Session { start, shared, worker: Some(worker) });

		manager.begin_shutdown();
		tokio::spawn(async move {
			tokio::task::yield_now().await;
			let _ = release_sender.send(());
		});
		tokio::time::timeout(Duration::from_secs(1), manager.wait_for_shutdown())
			.await
			.expect("current-thread shutdown must not deadlock");

		assert!(manager.lock_session().as_ref().is_some_and(|session| session.worker.is_none()));
	}

	#[test]
	fn manager_drop_cancels_and_reaps_its_worker_off_thread() {
		let manager = AccountLoginManager::unavailable_for_test();
		let start = start("078f0f9e-7b6e-4a31-8f4c-1d2e3f405168", AccountLoginMethod::DeviceCode);
		let shared = Arc::new(Shared::new(start.session_id.clone(), start.method));
		let worker_shared = Arc::clone(&shared);
		let settled = Arc::new(AtomicBool::new(false));
		let worker_settled = Arc::clone(&settled);
		let worker = thread::spawn(move || {
			while !worker_shared.cancellation.is_cancelled() {
				thread::yield_now();
			}
			worker_settled.store(true, Ordering::Release);
		});
		*manager.lock_session() = Some(Session { start, shared, worker: Some(worker) });

		drop(manager);

		let deadline = std::time::Instant::now() + Duration::from_secs(1);
		while !settled.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
			thread::yield_now();
		}
		assert!(settled.load(Ordering::Acquire));
	}

	#[test]
	fn restored_enrollment_resolves_the_original_account_uuid() {
		let start = AccountLoginStart {
			session_id: entity("018f0f9e-7b6e-4a31-8f4c-1d2e3f405162"),
			method: AccountLoginMethod::BrowserRedirect,
			install_mode: AccountLoginInstallMode::Enroll {
				operation_id: entity("028f0f9e-7b6e-4a31-8f4c-1d2e3f405163"),
				account_id: entity("038f0f9e-7b6e-4a31-8f4c-1d2e3f405164"),
				enabled: true,
				idempotency_key: IdempotencyKey::new("restore-fixture")
					.expect("fixture idempotency key"),
			},
		};
		let restored = entity("048f0f9e-7b6e-4a31-8f4c-1d2e3f405165");
		let result = ResultPayload::AccountRestored {
			requested_account_id: entity("038f0f9e-7b6e-4a31-8f4c-1d2e3f405164"),
			account: Box::new(decodex_protocol::AccountDto {
				account_id: restored.clone(),
				alias: WireText::new("restored").expect("fixture alias"),
				enabled: true,
				account_revision: EntityRevision(3),
				observed_state: decodex_protocol::AccountObservedStateDto::Unknown,
				lifecycle_readiness: decodex_protocol::AccountLifecycleReadinessDto::Ready,
				credential_binding: None,
				unsettled_operation: None,
				five_hour_quota: decodex_protocol::AccountQuotaWindowDto {
					duration_minutes: 300,
					observed_at_unix_micros: None,
					result: decodex_protocol::AccountQuotaStateDto::Unknown,
				},
				seven_day_quota: decodex_protocol::AccountQuotaWindowDto {
					duration_minutes: 10_080,
					observed_at_unix_micros: None,
					result: decodex_protocol::AccountQuotaStateDto::Unknown,
				},
			}),
		};

		assert_eq!(resolved_account_id(&start, &result), Ok(restored));
	}
}
