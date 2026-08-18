use std::{
	fs,
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
	},
	thread::{self, JoinHandle},
	time::Duration,
};

use decodex_protocol::{
	AccountClient, AccountCommandRejectionDto, AccountCommandResponse, ClientFailure,
	ClientProfile, CommandError, CommandPayload, EntityId, EntityRevision, IdempotencyKey,
	ResultPayload, WireText,
};
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

use crate::source_login_adapter::{self, Cancellation, Config, LoginEvent};

const INSTALL_DISPATCH_ATTEMPTS: usize = 3;
const INSTALL_REPLAY_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Failure {
	LoginFailed,
	LoginTimedOut,
	AccountMismatch,
	AccountChanged,
	AccountUnavailable,
	ProviderAlreadyEnrolled,
	RecoveryChanged,
	CredentialStoreUnavailable,
	ServiceUnavailable,
	OutcomeUnknown,
	SessionNotFound,
	Busy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
	OpeningBrowser,
	RequestingCode,
	WaitingForBrowser,
	Installing,
	Completed,
	Failed,
	Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LoginMethod {
	BrowserRedirect,
	DeviceCode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Prompt {
	verification_url: String,
	user_code: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Status {
	session_id: String,
	state: State,
	#[serde(skip_serializing_if = "Option::is_none")]
	prompt: Option<Prompt>,
	#[serde(skip_serializing_if = "Option::is_none")]
	authorization_url: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	failure: Option<Failure>,
}

impl Status {
	fn opening_browser(session_id: String) -> Self {
		Self {
			session_id,
			state: State::OpeningBrowser,
			prompt: None,
			authorization_url: None,
			failure: None,
		}
	}

	fn requesting_code(session_id: String) -> Self {
		Self {
			session_id,
			state: State::RequestingCode,
			prompt: None,
			authorization_url: None,
			failure: None,
		}
	}

	fn browser_authorization(session_id: String, authorization_url: String) -> Self {
		Self {
			session_id,
			state: State::WaitingForBrowser,
			prompt: None,
			authorization_url: Some(authorization_url),
			failure: None,
		}
	}

	fn device_authorization(session_id: String, prompt: Prompt) -> Self {
		Self {
			session_id,
			state: State::WaitingForBrowser,
			prompt: Some(prompt),
			authorization_url: None,
			failure: None,
		}
	}

	fn installing(session_id: String) -> Self {
		Self {
			session_id,
			state: State::Installing,
			prompt: None,
			authorization_url: None,
			failure: None,
		}
	}

	fn completed(session_id: String) -> Self {
		Self {
			session_id,
			state: State::Completed,
			prompt: None,
			authorization_url: None,
			failure: None,
		}
	}

	fn failed(session_id: String, failure: Failure) -> Self {
		Self {
			session_id,
			state: State::Failed,
			prompt: None,
			authorization_url: None,
			failure: Some(failure),
		}
	}

	fn cancelled(session_id: String) -> Self {
		Self {
			session_id,
			state: State::Cancelled,
			prompt: None,
			authorization_url: None,
			failure: None,
		}
	}

	fn is_terminal(&self) -> bool {
		matches!(self.state, State::Completed | State::Failed | State::Cancelled)
	}
}

pub(crate) enum InstallMode {
	Reauthenticate {
		expected_revision: EntityRevision,
		recovery_operation_id: Option<EntityId>,
	},
	Enroll {
		enabled: bool,
	},
}

pub(crate) struct Start {
	pub session_id: String,
	pub operation_id: EntityId,
	pub account_id: EntityId,
	pub idempotency_key: IdempotencyKey,
	pub login_method: LoginMethod,
	pub install_mode: InstallMode,
}

struct Shared {
	status: Mutex<Status>,
	cancellation: Cancellation,
}

impl Shared {
	fn new(session_id: String, login_method: LoginMethod) -> Self {
		let status = match login_method {
			LoginMethod::BrowserRedirect => Status::opening_browser(session_id),
			LoginMethod::DeviceCode => Status::requesting_code(session_id),
		};
		Self { status: Mutex::new(status), cancellation: Cancellation::default() }
	}

	fn status(&self) -> Status {
		match self.status.lock() {
			Ok(status) => status.clone(),
			Err(poisoned) => poisoned.into_inner().clone(),
		}
	}

	fn set_status(&self, status: Status) {
		match self.status.lock() {
			Ok(mut current) => *current = status,
			Err(poisoned) => *poisoned.into_inner() = status,
		}
	}
}

struct Session {
	session_id: String,
	shared: Arc<Shared>,
	worker: Option<JoinHandle<()>>,
}

pub(crate) struct Manager {
	session: Mutex<Option<Session>>,
	closed: AtomicBool,
}

impl Default for Manager {
	fn default() -> Self {
		Self { session: Mutex::new(None), closed: AtomicBool::new(false) }
	}
}

impl Manager {
	pub(crate) fn start(&self, start: Start, profile: ClientProfile, runtime: Handle) -> Status {
		let mut slot = self.lock_session();
		if self.closed.load(Ordering::Acquire) {
			return Status::failed(start.session_id, Failure::ServiceUnavailable);
		}
		if let Some(session) = slot.as_ref() {
			if session.session_id == start.session_id {
				return session.shared.status();
			}
			if !session.shared.status().is_terminal() {
				return Status::failed(start.session_id, Failure::Busy);
			}
		}
		let shared = Arc::new(Shared::new(start.session_id.clone(), start.login_method));
		let worker_shared = Arc::clone(&shared);
		let worker = match thread::Builder::new()
			.name("decodex-account-login".to_owned())
			.spawn(move || run_login(worker_shared, start, profile, runtime))
		{
			Ok(worker) => Some(worker),
			Err(_) => {
				shared.set_status(Status::failed(
					shared.status().session_id,
					Failure::ServiceUnavailable,
				));
				None
			},
		};
		let status = shared.status();
		*slot = Some(Session { session_id: status.session_id.clone(), shared, worker });
		status
	}

	pub(crate) fn poll(&self, session_id: &str) -> Status {
		let slot = self.lock_session();
		slot.as_ref().filter(|session| session.session_id == session_id).map_or_else(
			|| Status::failed(session_id.to_owned(), Failure::SessionNotFound),
			|session| session.shared.status(),
		)
	}

	pub(crate) fn cancel(&self, session_id: &str) -> Status {
		let (shared, worker) = {
			let mut slot = self.lock_session();
			let Some(session) = slot.as_ref().filter(|session| session.session_id == session_id)
			else {
				return Status::failed(session_id.to_owned(), Failure::SessionNotFound);
			};
			let shared = Arc::clone(&session.shared);
			shared.cancellation.cancel();
			let worker = slot.as_mut().and_then(|session| session.worker.take());
			(shared, worker)
		};
		if let Some(worker) = worker
			&& worker.join().is_err()
		{
			shared.set_status(Status::failed(session_id.to_owned(), Failure::ServiceUnavailable));
		}
		wait_for_terminal(&shared)
	}

	pub(crate) fn shutdown(&self) {
		self.closed.store(true, Ordering::Release);
		let session = {
			let mut slot = self.lock_session();
			slot.as_mut().map(|session| {
				session.shared.cancellation.cancel();
				(Arc::clone(&session.shared), session.worker.take())
			})
		};
		if let Some((shared, worker)) = session {
			if let Some(worker) = worker
				&& worker.join().is_err()
			{
				shared.set_status(Status::failed(
					shared.status().session_id,
					Failure::ServiceUnavailable,
				));
			}
			wait_for_terminal(&shared);
		}
	}

	fn lock_session(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
		match self.session.lock() {
			Ok(session) => session,
			Err(poisoned) => poisoned.into_inner(),
		}
	}
}

fn wait_for_terminal(shared: &Shared) -> Status {
	loop {
		let status = shared.status();
		if status.is_terminal() {
			return status;
		}
		thread::sleep(Duration::from_millis(10));
	}
}

impl Drop for Manager {
	fn drop(&mut self) {
		self.shutdown();
	}
}

fn run_login(shared: Arc<Shared>, start: Start, profile: ClientProfile, runtime: Handle) {
	let status = run_login_session(&shared, start, profile, runtime);
	shared.set_status(status);
}

fn run_login_session(
	shared: &Shared,
	start: Start,
	profile: ClientProfile,
	runtime: Handle,
) -> Status {
	let session_id = start.session_id.clone();
	let config = match Config::production() {
		Ok(config) => config,
		Err(error) => return Status::failed(session_id, map_adapter_error(error)),
	};
	let mut login_home = match LoginHome::create(&start.session_id) {
		Ok(home) => home,
		Err(_) => return Status::failed(session_id, Failure::ServiceUnavailable),
	};
	let status = run_login_in_home(shared, start, profile, runtime, &config, login_home.path());
	finalize_login_status(&mut login_home, session_id, status)
}

fn finalize_login_status(login_home: &mut LoginHome, session_id: String, status: Status) -> Status {
	if login_home.cleanup().is_ok() {
		return status;
	}
	let failure =
		if status.state == State::Completed || status.failure == Some(Failure::OutcomeUnknown) {
			Failure::OutcomeUnknown
		} else {
			Failure::ServiceUnavailable
		};
	Status::failed(session_id, failure)
}

fn run_login_in_home(
	shared: &Shared,
	start: Start,
	profile: ClientProfile,
	runtime: Handle,
	config: &Config,
	login_home: &Path,
) -> Status {
	let session_id = start.session_id.clone();
	let event_session_id = session_id.clone();
	let result = source_login_adapter::run(
		config,
		start.login_method,
		login_home,
		&runtime,
		&shared.cancellation,
		|event| match event {
			LoginEvent::BrowserAuthorization { authorization_url } => shared.set_status(
				Status::browser_authorization(event_session_id.clone(), authorization_url),
			),
			LoginEvent::DeviceAuthorization { verification_url, user_code } => shared.set_status(
				Status::device_authorization(
					event_session_id.clone(),
					Prompt { verification_url, user_code },
				),
			),
		},
	);
	if let Err(error) = result {
		return match error {
			source_login_adapter::Error::Cancelled => Status::cancelled(session_id),
			_ => Status::failed(session_id, map_adapter_error(error)),
		};
	}
	let auth_path = match canonical_auth_file(login_home) {
		Ok(path) => path,
		Err(_) => return Status::failed(session_id, Failure::LoginFailed),
	};
	shared.set_status(Status::installing(session_id.clone()));
	let outcome = runtime.block_on(install_credential(profile, start, auth_path));
	match outcome {
		Ok(()) => Status::completed(session_id),
		Err(failure) => Status::failed(session_id, failure),
	}
}

fn map_adapter_error(error: source_login_adapter::Error) -> Failure {
	match error {
		source_login_adapter::Error::Cancelled => Failure::LoginFailed,
		source_login_adapter::Error::TimedOut => Failure::LoginTimedOut,
		source_login_adapter::Error::Unavailable
		| source_login_adapter::Error::Rejected
		| source_login_adapter::Error::InvalidResponse => Failure::LoginFailed,
		source_login_adapter::Error::Persistence => Failure::ServiceUnavailable,
	}
}

async fn install_credential(
	profile: ClientProfile,
	start: Start,
	auth_path: PathBuf,
) -> Result<(), Failure> {
	let source_descriptor = WireText::new(auth_path.to_string_lossy().into_owned())
		.map_err(|_| Failure::LoginFailed)?;
	let (payload, expected_revision) = install_command(&start, source_descriptor);
	for attempt in 0..INSTALL_DISPATCH_ATTEMPTS {
		let response = AccountClient::new(profile.clone())
			.execute(payload.clone(), expected_revision, start.idempotency_key.clone())
			.await
			.map_err(map_client_failure)?;
		match response {
			AccountCommandResponse::Applied { result, .. } => match result.as_ref() {
				ResultPayload::AccountChanged { account }
					if account.account_id == start.account_id =>
					return Ok(()),
				_ => return Err(Failure::ServiceUnavailable),
			},
			AccountCommandResponse::Rejected { error } => return Err(map_command_error(&error)),
			AccountCommandResponse::PotentiallyDispatched { .. }
				if attempt + 1 < INSTALL_DISPATCH_ATTEMPTS =>
			{
				tokio::time::sleep(INSTALL_REPLAY_DELAY).await;
			},
			AccountCommandResponse::PotentiallyDispatched { .. } =>
				return Err(Failure::OutcomeUnknown),
		}
	}
	Err(Failure::OutcomeUnknown)
}

fn install_command(
	start: &Start,
	source_descriptor: WireText,
) -> (CommandPayload, Option<EntityRevision>) {
	match &start.install_mode {
		InstallMode::Reauthenticate { expected_revision, recovery_operation_id } => (
			CommandPayload::ReauthenticateAccountFromCredentialFile {
				operation_id: start.operation_id.clone(),
				account_id: start.account_id.clone(),
				recovery_operation_id: recovery_operation_id.clone(),
				source_descriptor,
			},
			Some(*expected_revision),
		),
		InstallMode::Enroll { enabled } => (
			CommandPayload::EnrollAccountFromCredentialFile {
				operation_id: start.operation_id.clone(),
				account_id: start.account_id.clone(),
				enabled: *enabled,
				source_descriptor,
			},
			None,
		),
	}
}

fn map_client_failure(_failure: ClientFailure) -> Failure {
	Failure::ServiceUnavailable
}

fn map_command_error(error: &CommandError) -> Failure {
	match error {
		CommandError::ExpectedRevisionMismatch { .. } => Failure::AccountChanged,
		CommandError::AccountCommandRejected { rejection, .. } => match rejection {
			AccountCommandRejectionDto::ProviderMismatch => Failure::AccountMismatch,
			AccountCommandRejectionDto::ProviderAlreadyEnrolled => Failure::ProviderAlreadyEnrolled,
			AccountCommandRejectionDto::StaleAccount => Failure::AccountChanged,
			AccountCommandRejectionDto::CredentialStoreUnavailable =>
				Failure::CredentialStoreUnavailable,
			AccountCommandRejectionDto::AccountNotFound
			| AccountCommandRejectionDto::CredentialAbsent
			| AccountCommandRejectionDto::LifecycleUnready
			| AccountCommandRejectionDto::OperationUnsettled
			| AccountCommandRejectionDto::InvalidRequest
			| AccountCommandRejectionDto::AccountInUse
			| AccountCommandRejectionDto::RoutingOrderInvalid => Failure::AccountUnavailable,
			AccountCommandRejectionDto::OperationNotFound
			| AccountCommandRejectionDto::ManualRecoveryRequired => Failure::RecoveryChanged,
			AccountCommandRejectionDto::StaleRoutingControl => Failure::AccountChanged,
		},
		CommandError::IdempotencyConflict
		| CommandError::IdempotencyCapacityExceeded { .. }
		| CommandError::ApplicationUnavailable { .. }
		| CommandError::QuickTaskUnavailable { .. }
		| CommandError::QuickTaskRecoveryRequired { .. }
		| CommandError::AcceptanceUnknown => Failure::ServiceUnavailable,
	}
}

fn canonical_auth_file(login_home: &Path) -> Result<PathBuf, ()> {
	let expected = login_home.join("auth.json");
	let canonical = fs::canonicalize(&expected).map_err(|_| ())?;
	if canonical != expected {
		return Err(());
	}
	let metadata = fs::symlink_metadata(&expected).map_err(|_| ())?;
	if !metadata.file_type().is_file()
		|| metadata.uid() != unsafe { libc::geteuid() }
		|| metadata.permissions().mode() & 0o077 != 0
	{
		return Err(());
	}
	Ok(canonical)
}

struct LoginHome {
	path: PathBuf,
	device: u64,
	inode: u64,
	cleaned: bool,
}

impl LoginHome {
	fn create(session_id: &str) -> Result<Self, ()> {
		let base = fs::canonicalize(std::env::temp_dir()).map_err(|_| ())?;
		let path = base.join(format!("decodex-codex-device-login-{session_id}"));
		fs::create_dir(&path).map_err(|_| ())?;
		if fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).is_err() {
			let _ = fs::remove_dir(&path);
			return Err(());
		}
		let metadata = match fs::symlink_metadata(&path) {
			Ok(metadata) => metadata,
			Err(_) => {
				let _ = fs::remove_dir(&path);
				return Err(());
			},
		};
		if !metadata.file_type().is_dir()
			|| metadata.uid() != unsafe { libc::geteuid() }
			|| metadata.permissions().mode() & 0o077 != 0
		{
			let _ = fs::remove_dir(&path);
			return Err(());
		}
		Ok(Self { path, device: metadata.dev(), inode: metadata.ino(), cleaned: false })
	}

	fn path(&self) -> &Path {
		&self.path
	}

	fn cleanup(&mut self) -> Result<(), ()> {
		if self.cleaned {
			return Ok(());
		}
		let metadata = match fs::symlink_metadata(&self.path) {
			Ok(metadata) => metadata,
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
				self.cleaned = true;
				return Ok(());
			},
			Err(_) => return Err(()),
		};
		if !metadata.file_type().is_dir()
			|| metadata.dev() != self.device
			|| metadata.ino() != self.inode
		{
			return Err(());
		}
		fs::remove_dir_all(&self.path).map_err(|_| ())?;
		match fs::symlink_metadata(&self.path) {
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
				self.cleaned = true;
				Ok(())
			},
			Ok(_) | Err(_) => Err(()),
		}
	}
}

impl Drop for LoginHome {
	fn drop(&mut self) {
		let _ = self.cleanup();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entity_id(value: &str) -> EntityId {
		EntityId::new(value).expect("fixture entity ID")
	}

	fn start(install_mode: InstallMode) -> Start {
		start_with_login_method(install_mode, LoginMethod::BrowserRedirect)
	}

	fn start_with_login_method(install_mode: InstallMode, login_method: LoginMethod) -> Start {
		Start {
			session_id: "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162".to_owned(),
			operation_id: entity_id("028f0f9e-7b6e-4a31-8f4c-1d2e3f405163"),
			account_id: entity_id("038f0f9e-7b6e-4a31-8f4c-1d2e3f405164"),
			idempotency_key: IdempotencyKey::new("account-login-fixture")
				.expect("fixture idempotency key"),
			login_method,
			install_mode,
		}
	}

	#[test]
	fn login_methods_start_in_distinct_closed_states() {
		let session_id = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let browser = Shared::new(session_id.to_owned(), LoginMethod::BrowserRedirect).status();
		let device_code = Shared::new(session_id.to_owned(), LoginMethod::DeviceCode).status();

		assert_eq!(browser.state, State::OpeningBrowser);
		assert_eq!(browser.prompt, None);
		assert_eq!(browser.authorization_url, None);
		assert_eq!(device_code.state, State::RequestingCode);
		assert_eq!(device_code.prompt, None);
		assert_eq!(device_code.authorization_url, None);
	}

	#[test]
	fn structured_login_events_publish_closed_browser_and_device_status() {
		let session_id = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162".to_owned();
		let browser = Status::browser_authorization(
			session_id.clone(),
			"https://auth.openai.com/oauth/authorize?fixture=true".to_owned(),
		);
		let device = Status::device_authorization(
			session_id,
			Prompt {
				verification_url: "https://auth.openai.com/codex/device".to_owned(),
				user_code: "FIXT-URE1".to_owned(),
			},
		);

		assert_eq!(browser.state, State::WaitingForBrowser);
		assert!(browser.authorization_url.is_some());
		assert!(browser.prompt.is_none());
		assert_eq!(device.state, State::WaitingForBrowser);
		assert!(device.authorization_url.is_none());
		assert!(device.prompt.is_some());
	}

	#[test]
	fn enrollment_installs_with_the_typed_unrevisioned_command() {
		for login_method in [LoginMethod::BrowserRedirect, LoginMethod::DeviceCode] {
			let start =
				start_with_login_method(InstallMode::Enroll { enabled: true }, login_method);
			let source_descriptor =
				WireText::new("/private/tmp/device-login/auth.json").expect("fixture source");

			let (command, expected_revision) = install_command(&start, source_descriptor.clone());

			assert_eq!(expected_revision, None);
			assert_eq!(
				command,
				CommandPayload::EnrollAccountFromCredentialFile {
					operation_id: start.operation_id,
					account_id: start.account_id,
					enabled: true,
					source_descriptor,
				}
			);
		}
	}

	#[test]
	fn reauthentication_retains_its_revision_and_recovery_fences() {
		let recovery_operation_id = entity_id("048f0f9e-7b6e-4a31-8f4c-1d2e3f405165");
		let start = start(InstallMode::Reauthenticate {
			expected_revision: EntityRevision(7),
			recovery_operation_id: Some(recovery_operation_id.clone()),
		});
		let source_descriptor =
			WireText::new("/private/tmp/device-login/auth.json").expect("fixture source");

		let (command, expected_revision) = install_command(&start, source_descriptor.clone());

		assert_eq!(expected_revision, Some(EntityRevision(7)));
		assert_eq!(
			command,
			CommandPayload::ReauthenticateAccountFromCredentialFile {
				operation_id: start.operation_id,
				account_id: start.account_id,
				recovery_operation_id: Some(recovery_operation_id),
				source_descriptor,
			}
		);
	}

	#[test]
	fn duplicate_provider_is_a_closed_enrollment_failure() {
		let error = CommandError::AccountCommandRejected {
			rejection: AccountCommandRejectionDto::ProviderAlreadyEnrolled,
			actual_revision: None,
		};

		assert_eq!(map_command_error(&error), Failure::ProviderAlreadyEnrolled);
		assert_eq!(
			serde_json::to_value(Status::failed(
				"058f0f9e-7b6e-4a31-8f4c-1d2e3f405166".to_owned(),
				Failure::ProviderAlreadyEnrolled,
			))
			.expect("status must encode"),
			serde_json::json!({
				"session_id": "058f0f9e-7b6e-4a31-8f4c-1d2e3f405166",
				"state": "failed",
				"failure": "provider_already_enrolled",
			}),
		);
	}

	#[test]
	fn login_home_is_private_and_removed_on_drop() {
		let session_id = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		let metadata = fs::metadata(&path).expect("login home metadata");

		assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
		drop(home);
		assert!(!path.exists());
	}

	#[test]
	fn login_home_cleanup_proves_absence() {
		let session_id = "028f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let mut home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		fs::write(path.join("auth.json"), b"secret").expect("temporary credential");

		home.cleanup().expect("explicit cleanup");

		assert!(!path.exists());
		assert!(home.cleaned);
	}

	#[test]
	fn login_home_cleanup_rejects_identity_drift() {
		let session_id = "038f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let mut home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		let moved = path.with_extension("moved");
		fs::rename(&path, &moved).expect("move original home");
		fs::create_dir(&path).expect("replacement home");

		assert!(home.cleanup().is_err());
		assert!(path.exists());

		fs::remove_dir(&path).expect("remove replacement");
		fs::rename(&moved, &path).expect("restore original");
		home.cleanup().expect("cleanup restored home");
	}

	#[test]
	fn completed_install_with_unproven_cleanup_is_outcome_unknown() {
		let session_id = "048f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let mut home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		let moved = path.with_extension("moved");
		fs::rename(&path, &moved).expect("move original home");
		fs::create_dir(&path).expect("replacement home");

		let status = finalize_login_status(
			&mut home,
			session_id.to_owned(),
			Status::completed(session_id.to_owned()),
		);

		assert_eq!(status.state, State::Failed);
		assert_eq!(status.failure, Some(Failure::OutcomeUnknown));

		fs::remove_dir(&path).expect("remove replacement");
		fs::rename(&moved, &path).expect("restore original");
		home.cleanup().expect("cleanup restored home");
	}

	#[test]
	fn uncertain_install_with_unproven_cleanup_remains_outcome_unknown() {
		let session_id = "048f0f9e-7b6e-4a31-8f4c-1d2e3f405163";
		let mut home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		let moved = path.with_extension("moved");
		fs::rename(&path, &moved).expect("move original home");
		fs::create_dir(&path).expect("replacement home");

		let status = finalize_login_status(
			&mut home,
			session_id.to_owned(),
			Status::failed(session_id.to_owned(), Failure::OutcomeUnknown),
		);

		assert_eq!(status.state, State::Failed);
		assert_eq!(status.failure, Some(Failure::OutcomeUnknown));

		fs::remove_dir(&path).expect("remove replacement");
		fs::rename(&moved, &path).expect("restore original");
		home.cleanup().expect("cleanup restored home");
	}

	#[test]
	fn cancel_waits_for_worker_terminal_cleanup() {
		let manager = Manager::default();
		let session_id = "078f0f9e-7b6e-4a31-8f4c-1d2e3f405162".to_owned();
		let shared =
			Arc::new(Shared::new(session_id.clone(), LoginMethod::BrowserRedirect));
		let worker_shared = Arc::clone(&shared);
		let worker_session_id = session_id.clone();
		let worker = thread::spawn(move || {
			while !worker_shared.cancellation.is_cancelled() {
				thread::yield_now();
			}
			worker_shared.set_status(Status::cancelled(worker_session_id));
		});
		*manager.lock_session() =
			Some(Session { session_id: session_id.clone(), shared, worker: Some(worker) });

		let status = manager.cancel(&session_id);

		assert_eq!(status.state, State::Cancelled);
		assert!(manager.lock_session().as_ref().is_some_and(|session| session.worker.is_none()));
	}

	#[test]
	fn shutdown_closes_start_fence_and_joins_installing_worker() {
		let manager = Manager::default();
		let session_id = "088f0f9e-7b6e-4a31-8f4c-1d2e3f405162".to_owned();
		let shared =
			Arc::new(Shared::new(session_id.clone(), LoginMethod::BrowserRedirect));
		shared.set_status(Status::installing(session_id.clone()));
		let worker_shared = Arc::clone(&shared);
		let worker_session_id = session_id.clone();
		let worker = thread::spawn(move || {
			thread::sleep(Duration::from_millis(5));
			worker_shared.set_status(Status::completed(worker_session_id));
		});
		*manager.lock_session() = Some(Session {
			session_id: session_id.clone(),
			shared: Arc::clone(&shared),
			worker: Some(worker),
		});

		manager.shutdown();

		assert!(manager.closed.load(Ordering::Acquire));
		assert_eq!(shared.status().state, State::Completed);
		assert!(manager.lock_session().as_ref().is_some_and(|session| session.worker.is_none()));
	}

}
