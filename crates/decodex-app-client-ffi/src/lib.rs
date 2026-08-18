//! In-process C ABI for the native Decodex app.
//!
//! The bridge is a credential-negative client of the one local `decodexd`
//! service. It reuses the typed Rust protocol clients directly. Its only child
//! process is one bounded official Codex device-login session; Swift never
//! receives credential bytes or a credential-file path.

mod account_reauthentication;
mod fast_mode;

use std::{
	collections::HashMap,
	ffi::c_void,
	panic::{AssertUnwindSafe, catch_unwind},
	path::PathBuf,
	ptr, slice,
	sync::{
		Arc, Mutex, OnceLock,
		atomic::{AtomicUsize, Ordering},
	},
};

use decodex_protocol::{
	AccountClient, AccountCommandResponse, ClientFailure, ClientProfile, CommandPayload, EntityId,
	EntityRevision, IdempotencyKey, ResetCardClient, ResetCardConsumeResponse,
	ResetCardDescriptorDto, ResetCardOperationResult, ServerId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::runtime::{Builder, Runtime};

const ABI_VERSION: u32 = 1;
const CONFIG_SCHEMA: &str = "decodex/app-native-client-config/1";
const RESPONSE_SCHEMA: &str = "decodex/app-native-client/1";

static RUNTIME: OnceLock<Result<Runtime, ()>> = OnceLock::new();
static CLIENTS: OnceLock<Mutex<HashMap<usize, Arc<NativeClient>>>> = OnceLock::new();
static NEXT_CLIENT_ID: AtomicUsize = AtomicUsize::new(1);

struct NativeClient {
	profile: ClientProfile,
	account_reauthentication: account_reauthentication::Manager,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientConfig {
	schema: String,
	#[serde(default)]
	profile_name: Option<String>,
	#[serde(default)]
	expected_server_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
	ListAccounts {
		schema: String,
	},
	GetResetCards {
		schema: String,
		account_id: String,
	},
	GetAccountProfile {
		schema: String,
		account_id: String,
		include_email: bool,
	},
	GetCodexAuthProjection {
		schema: String,
	},
	WaitForAccountObservation {
		schema: String,
		after_generation: u64,
		#[serde(default)]
		request_refresh: bool,
	},
	ResetCardStatus {
		schema: String,
		idempotency_key: String,
	},
	UseResetCard {
		schema: String,
		account_id: String,
		granted_at_unix_seconds: i64,
		expires_at_unix_seconds: i64,
		expected_revision: u64,
		idempotency_key: String,
	},
	EnrollAccount {
		schema: String,
		operation_id: String,
		account_id: String,
		enabled: bool,
		idempotency_key: String,
	},
	EnableAccount {
		schema: String,
		account_id: String,
		expected_revision: u64,
		idempotency_key: String,
	},
	DisableAccount {
		schema: String,
		account_id: String,
		expected_revision: u64,
		idempotency_key: String,
	},
	LogoutAccount {
		schema: String,
		operation_id: String,
		account_id: String,
		expected_revision: u64,
		idempotency_key: String,
	},
	SetFixedSelection {
		schema: String,
		account_id: String,
		expected_account_revision: u64,
		expected_routing_revision: u64,
		idempotency_key: String,
	},
	SetBalancedSelection {
		schema: String,
		expected_routing_revision: u64,
		idempotency_key: String,
	},
	SetAccountOrder {
		schema: String,
		order: Vec<String>,
		expected_routing_revision: u64,
		idempotency_key: String,
	},
	StartAccountEnrollment {
		schema: String,
		session_id: String,
		operation_id: String,
		account_id: String,
		enabled: bool,
		idempotency_key: String,
		codex_bin: String,
		login_method: account_reauthentication::LoginMethod,
	},
	StartAccountReauthentication {
		schema: String,
		session_id: String,
		operation_id: String,
		account_id: String,
		expected_revision: u64,
		recovery_operation_id: Option<String>,
		idempotency_key: String,
		codex_bin: String,
		login_method: account_reauthentication::LoginMethod,
	},
	PollAccountReauthentication {
		schema: String,
		session_id: String,
	},
	CancelAccountReauthentication {
		schema: String,
		session_id: String,
	},
	UseAccountInCodex {
		schema: String,
		account_id: String,
		expected_revision: u64,
		idempotency_key: String,
	},
	FastModeStatus {
		schema: String,
	},
	SetFastMode {
		schema: String,
		enabled: bool,
	},
}

impl Request {
	fn operation(&self) -> &'static str {
		match self {
			Self::ListAccounts { .. } => "list_accounts",
			Self::GetResetCards { .. } => "get_reset_cards",
			Self::GetAccountProfile { .. } => "get_account_profile",
			Self::GetCodexAuthProjection { .. } => "get_codex_auth_projection",
			Self::WaitForAccountObservation { .. } => "wait_for_account_observation",
			Self::ResetCardStatus { .. } => "reset_card_status",
			Self::UseResetCard { .. } => "use_reset_card",
			Self::EnrollAccount { .. } => "enroll_account",
			Self::EnableAccount { .. } => "enable_account",
			Self::DisableAccount { .. } => "disable_account",
			Self::LogoutAccount { .. } => "logout_account",
			Self::SetFixedSelection { .. } => "set_fixed_selection",
			Self::SetBalancedSelection { .. } => "set_balanced_selection",
			Self::SetAccountOrder { .. } => "set_account_order",
			Self::StartAccountEnrollment { .. } => "start_account_enrollment",
			Self::StartAccountReauthentication { .. } => "start_account_reauthentication",
			Self::PollAccountReauthentication { .. } => "poll_account_reauthentication",
			Self::CancelAccountReauthentication { .. } => "cancel_account_reauthentication",
			Self::UseAccountInCodex { .. } => "use_account_in_codex",
			Self::FastModeStatus { .. } => "fast_mode_status",
			Self::SetFastMode { .. } => "set_fast_mode",
		}
	}

	fn schema(&self) -> &str {
		match self {
			Self::ListAccounts { schema }
			| Self::GetResetCards { schema, .. }
			| Self::GetAccountProfile { schema, .. }
			| Self::GetCodexAuthProjection { schema }
			| Self::WaitForAccountObservation { schema, .. }
			| Self::ResetCardStatus { schema, .. }
			| Self::UseResetCard { schema, .. }
			| Self::EnrollAccount { schema, .. }
			| Self::EnableAccount { schema, .. }
			| Self::DisableAccount { schema, .. }
			| Self::LogoutAccount { schema, .. }
			| Self::SetFixedSelection { schema, .. }
			| Self::SetBalancedSelection { schema, .. }
			| Self::SetAccountOrder { schema, .. }
			| Self::StartAccountEnrollment { schema, .. }
			| Self::StartAccountReauthentication { schema, .. }
			| Self::PollAccountReauthentication { schema, .. }
			| Self::CancelAccountReauthentication { schema, .. }
			| Self::UseAccountInCodex { schema, .. }
			| Self::FastModeStatus { schema }
			| Self::SetFastMode { schema, .. } => schema,
		}
	}
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum BridgeFailure {
	InvalidConfiguration,
	InvalidRequest,
	InvalidInput,
	InvalidHandle,
	RuntimeUnavailable,
	InternalFailure,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ResponseFailure {
	Client(ClientFailure),
	Bridge(BridgeFailure),
	FastMode(fast_mode::FastModeFailure),
}

#[derive(Serialize)]
struct AuthorityResponse {
	profile_name: String,
	server_id: String,
}

#[derive(Serialize)]
struct SuccessResponse {
	schema: &'static str,
	outcome: &'static str,
	operation: &'static str,
	authority: AuthorityResponse,
	data: Value,
}

#[derive(Serialize)]
struct FailureResponse {
	schema: &'static str,
	outcome: &'static str,
	operation: &'static str,
	failure: ResponseFailure,
}

#[derive(Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
enum ResetCardConsumeDto {
	Accepted {
		account_id: EntityId,
		descriptor: ResetCardDescriptorDto,
		state: ResetCardOperationResult,
		entity_revision: EntityRevision,
	},
	Rejected {
		error: decodex_protocol::CommandError,
	},
	PotentiallyDispatched {
		failure: ClientFailure,
	},
}

impl From<ResetCardConsumeResponse> for ResetCardConsumeDto {
	fn from(response: ResetCardConsumeResponse) -> Self {
		match response {
			ResetCardConsumeResponse::Accepted {
				account_id,
				descriptor,
				state,
				entity_revision,
			} => Self::Accepted { account_id, descriptor, state, entity_revision },
			ResetCardConsumeResponse::Rejected { error } => Self::Rejected { error },
			ResetCardConsumeResponse::PotentiallyDispatched { failure } =>
				Self::PotentiallyDispatched { failure },
		}
	}
}

enum RequestFailure {
	Client(ClientFailure),
	Bridge(BridgeFailure),
	FastMode(fast_mode::FastModeFailure),
}

impl From<ClientFailure> for RequestFailure {
	fn from(failure: ClientFailure) -> Self {
		Self::Client(failure)
	}
}

/// Return the C ABI generation implemented by this library.
#[unsafe(no_mangle)]
pub extern "C" fn decodex_app_native_client_abi_version() -> u32 {
	ABI_VERSION
}

/// Return the exact daemon/client artifact cohort required by this library.
#[unsafe(no_mangle)]
pub extern "C" fn decodex_app_native_client_artifact_cohort() -> u32 {
	decodex_protocol::CURRENT_ARTIFACT_COHORT
}

/// Create one thread-safe native client handle.
///
/// On success, this returns a non-null opaque handle and leaves the error output
/// empty. On failure, this returns null and writes one owned JSON failure
/// envelope. The caller must release that envelope with
/// [`decodex_app_native_client_free`].
///
/// # Safety
///
/// `config_json` must identify `config_len` readable bytes. `out_error_json`
/// and `out_error_len` must be valid writable pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn decodex_app_native_client_create(
	config_json: *const u8,
	config_len: usize,
	out_error_json: *mut *mut u8,
	out_error_len: *mut usize,
) -> *mut c_void {
	if out_error_json.is_null() || out_error_len.is_null() {
		return ptr::null_mut();
	}
	// SAFETY: Both output pointers were checked above and the caller contract
	// requires them to be writable.
	unsafe {
		*out_error_json = ptr::null_mut();
		*out_error_len = 0;
	}

	let result = catch_unwind(AssertUnwindSafe(|| {
		create_client(config_json, config_len, out_error_json, out_error_len)
	}));
	match result {
		Ok(handle) => handle,
		Err(_) => {
			let _ = write_failure(
				out_error_json,
				out_error_len,
				"create",
				ResponseFailure::Bridge(BridgeFailure::InternalFailure),
			);
			ptr::null_mut()
		},
	}
}

/// Release one opaque native client handle.
///
/// In-flight requests retain their own reference and finish safely. A stale
/// handle cannot start another request after this function returns.
#[unsafe(no_mangle)]
pub extern "C" fn decodex_app_native_client_destroy(client: *mut c_void) {
	if client.is_null() {
		return;
	}
	let client_id = client as usize;
	let clients = clients();
	let client = match clients.lock() {
		Ok(mut clients) => clients.remove(&client_id),
		Err(poisoned) => poisoned.into_inner().remove(&client_id),
	};
	if let Some(client) = client {
		client.account_reauthentication.shutdown();
	}
}

/// Execute one strict JSON request synchronously on the shared Rust runtime.
///
/// The function returns zero whenever it wrote a JSON response. It returns one
/// only when output pointers are invalid and no response can be returned.
///
/// # Safety
///
/// `request_json` must identify `request_len` readable bytes.
/// `out_response_json` and `out_response_len` must be valid writable pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn decodex_app_native_client_request(
	client: *mut c_void,
	request_json: *const u8,
	request_len: usize,
	out_response_json: *mut *mut u8,
	out_response_len: *mut usize,
) -> i32 {
	if out_response_json.is_null() || out_response_len.is_null() {
		return 1;
	}
	// SAFETY: Both output pointers were checked above and the caller contract
	// requires them to be writable.
	unsafe {
		*out_response_json = ptr::null_mut();
		*out_response_len = 0;
	}

	let result = catch_unwind(AssertUnwindSafe(|| {
		request(client, request_json, request_len, out_response_json, out_response_len)
	}));
	match result {
		Ok(status) => status,
		Err(_) => write_failure(
			out_response_json,
			out_response_len,
			"request",
			ResponseFailure::Bridge(BridgeFailure::InternalFailure),
		),
	}
}

/// Release one exact response buffer allocated by this library.
///
/// # Safety
///
/// `buffer` and `len` must be the unchanged pair returned by this library and
/// must be freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn decodex_app_native_client_free(buffer: *mut u8, len: usize) {
	if buffer.is_null() || len == 0 {
		return;
	}
	// SAFETY: The caller contract requires the exact pointer/length pair from
	// `write_bytes`, which leaked one boxed slice with this layout.
	unsafe {
		drop(Box::from_raw(ptr::slice_from_raw_parts_mut(buffer, len)));
	}
}

fn create_client(
	config_json: *const u8,
	config_len: usize,
	out_error_json: *mut *mut u8,
	out_error_len: *mut usize,
) -> *mut c_void {
	let Some(config_bytes) = input_bytes(config_json, config_len) else {
		let _ = write_failure(
			out_error_json,
			out_error_len,
			"create",
			ResponseFailure::Bridge(BridgeFailure::InvalidConfiguration),
		);
		return ptr::null_mut();
	};
	let Ok(config) = serde_json::from_slice::<ClientConfig>(config_bytes) else {
		let _ = write_failure(
			out_error_json,
			out_error_len,
			"create",
			ResponseFailure::Bridge(BridgeFailure::InvalidConfiguration),
		);
		return ptr::null_mut();
	};
	if config.schema != CONFIG_SCHEMA
		|| config.profile_name.as_ref().is_some_and(|value| value.is_empty())
	{
		let _ = write_failure(
			out_error_json,
			out_error_len,
			"create",
			ResponseFailure::Bridge(BridgeFailure::InvalidConfiguration),
		);
		return ptr::null_mut();
	}
	if runtime().is_err() {
		let _ = write_failure(
			out_error_json,
			out_error_len,
			"create",
			ResponseFailure::Bridge(BridgeFailure::RuntimeUnavailable),
		);
		return ptr::null_mut();
	}

	let profile = match ClientProfile::load_default(config.profile_name.as_deref()) {
		Ok(profile) => profile,
		Err(failure) => {
			let _ = write_failure(
				out_error_json,
				out_error_len,
				"create",
				ResponseFailure::Client(failure),
			);
			return ptr::null_mut();
		},
	};
	let profile = match config.expected_server_id {
		Some(expected_server_id) => {
			if !is_canonical_uuid(&expected_server_id) {
				let _ = write_failure(
					out_error_json,
					out_error_len,
					"create",
					ResponseFailure::Bridge(BridgeFailure::InvalidConfiguration),
				);
				return ptr::null_mut();
			}
			let Ok(server_id) = ServerId::new(expected_server_id) else {
				let _ = write_failure(
					out_error_json,
					out_error_len,
					"create",
					ResponseFailure::Bridge(BridgeFailure::InvalidConfiguration),
				);
				return ptr::null_mut();
			};
			profile.with_expected_server_id(server_id)
		},
		None => profile,
	};

	let client_id = next_client_id();
	let client = Arc::new(NativeClient {
		profile,
		account_reauthentication: account_reauthentication::Manager::default(),
	});
	match clients().lock() {
		Ok(mut clients) => {
			clients.insert(client_id, client);
		},
		Err(_) => {
			let _ = write_failure(
				out_error_json,
				out_error_len,
				"create",
				ResponseFailure::Bridge(BridgeFailure::InternalFailure),
			);
			return ptr::null_mut();
		},
	}

	client_id as *mut c_void
}

fn request(
	client: *mut c_void,
	request_json: *const u8,
	request_len: usize,
	out_response_json: *mut *mut u8,
	out_response_len: *mut usize,
) -> i32 {
	let Some(request_bytes) = input_bytes(request_json, request_len) else {
		return write_failure(
			out_response_json,
			out_response_len,
			"request",
			ResponseFailure::Bridge(BridgeFailure::InvalidRequest),
		);
	};
	let request = match serde_json::from_slice::<Request>(request_bytes) {
		Ok(request) => request,
		Err(_) => {
			return write_failure(
				out_response_json,
				out_response_len,
				"request",
				ResponseFailure::Bridge(BridgeFailure::InvalidRequest),
			);
		},
	};
	let operation = request.operation();
	if request.schema() != RESPONSE_SCHEMA {
		return write_failure(
			out_response_json,
			out_response_len,
			operation,
			ResponseFailure::Bridge(BridgeFailure::InvalidRequest),
		);
	}
	let client_id = client as usize;
	let native_client = match clients().lock() {
		Ok(clients) => clients.get(&client_id).cloned(),
		Err(_) => None,
	};
	let Some(native_client) = native_client else {
		return write_failure(
			out_response_json,
			out_response_len,
			operation,
			ResponseFailure::Bridge(BridgeFailure::InvalidHandle),
		);
	};
	let Ok(runtime) = runtime() else {
		return write_failure(
			out_response_json,
			out_response_len,
			operation,
			ResponseFailure::Bridge(BridgeFailure::RuntimeUnavailable),
		);
	};
	let authority = AuthorityResponse {
		profile_name: native_client.profile.name().to_owned(),
		server_id: native_client.profile.expected_server_id().as_str().to_owned(),
	};

	match runtime.block_on(execute_request(native_client, request)) {
		Ok(data) => write_serialized(
			out_response_json,
			out_response_len,
			&SuccessResponse {
				schema: RESPONSE_SCHEMA,
				outcome: "success",
				operation,
				authority,
				data,
			},
		),
		Err(RequestFailure::Client(failure)) => write_failure(
			out_response_json,
			out_response_len,
			operation,
			ResponseFailure::Client(failure),
		),
		Err(RequestFailure::Bridge(failure)) => write_failure(
			out_response_json,
			out_response_len,
			operation,
			ResponseFailure::Bridge(failure),
		),
		Err(RequestFailure::FastMode(failure)) => write_failure(
			out_response_json,
			out_response_len,
			operation,
			ResponseFailure::FastMode(failure),
		),
	}
}

async fn execute_request(
	native_client: Arc<NativeClient>,
	request: Request,
) -> Result<Value, RequestFailure> {
	let profile = native_client.profile.clone();
	match request {
		Request::ListAccounts { .. } => list_accounts(profile).await,
		Request::GetResetCards { account_id, .. } => get_reset_cards(profile, account_id).await,
		Request::GetAccountProfile { account_id, include_email, .. } =>
			get_account_profile(profile, account_id, include_email).await,
		Request::GetCodexAuthProjection { .. } => get_codex_auth_projection(profile).await,
		Request::WaitForAccountObservation { after_generation, request_refresh, .. } =>
			wait_for_account_observation(profile, after_generation, request_refresh).await,
		Request::ResetCardStatus { idempotency_key, .. } =>
			get_reset_card_status(profile, idempotency_key).await,
		Request::UseResetCard {
			account_id,
			granted_at_unix_seconds,
			expires_at_unix_seconds,
			expected_revision,
			idempotency_key,
			..
		} =>
			consume_reset_card(
				profile,
				account_id,
				granted_at_unix_seconds,
				expires_at_unix_seconds,
				expected_revision,
				idempotency_key,
			)
			.await,
		Request::EnrollAccount { operation_id, account_id, enabled, idempotency_key, .. } =>
			enroll_account(profile, operation_id, account_id, enabled, idempotency_key).await,
		Request::EnableAccount { account_id, expected_revision, idempotency_key, .. } =>
			set_account_enabled(profile, account_id, true, expected_revision, idempotency_key).await,
		Request::DisableAccount { account_id, expected_revision, idempotency_key, .. } =>
			set_account_enabled(profile, account_id, false, expected_revision, idempotency_key)
				.await,
		Request::LogoutAccount {
			operation_id,
			account_id,
			expected_revision,
			idempotency_key,
			..
		} =>
			logout_account(profile, operation_id, account_id, expected_revision, idempotency_key)
				.await,
		Request::SetFixedSelection {
			account_id,
			expected_account_revision,
			expected_routing_revision,
			idempotency_key,
			..
		} =>
			set_fixed_selection(
				profile,
				account_id,
				expected_account_revision,
				expected_routing_revision,
				idempotency_key,
			)
			.await,
		Request::SetBalancedSelection { expected_routing_revision, idempotency_key, .. } =>
			set_balanced_selection(profile, expected_routing_revision, idempotency_key).await,
		Request::SetAccountOrder { order, expected_routing_revision, idempotency_key, .. } =>
			set_account_order(profile, order, expected_routing_revision, idempotency_key).await,
		Request::StartAccountEnrollment {
			session_id,
			operation_id,
			account_id,
			enabled,
			idempotency_key,
			codex_bin,
			login_method,
			..
		} => start_account_enrollment(
			&native_client,
			profile,
			AccountEnrollmentInput {
				session_id,
				operation_id,
				account_id,
				enabled,
				idempotency_key,
				codex_bin,
				login_method,
			},
		),
		Request::StartAccountReauthentication {
			session_id,
			operation_id,
			account_id,
			expected_revision,
			recovery_operation_id,
			idempotency_key,
			codex_bin,
			login_method,
			..
		} => start_account_reauthentication(
			&native_client,
			profile,
			AccountReauthenticationInput {
				session_id,
				operation_id,
				account_id,
				expected_revision,
				recovery_operation_id,
				idempotency_key,
				codex_bin,
				login_method,
			},
		),
		Request::PollAccountReauthentication { session_id, .. } =>
			poll_account_reauthentication(&native_client, session_id),
		Request::CancelAccountReauthentication { session_id, .. } =>
			cancel_account_reauthentication(&native_client, session_id),
		Request::UseAccountInCodex { account_id, expected_revision, idempotency_key, .. } =>
			use_account_in_codex(profile, account_id, expected_revision, idempotency_key).await,
		Request::FastModeStatus { .. } => fast_mode_status(),
		Request::SetFastMode { enabled, .. } => set_fast_mode(enabled),
	}
}

#[derive(Serialize)]
struct FastModeData {
	enabled: bool,
}

struct AccountReauthenticationInput {
	session_id: String,
	operation_id: String,
	account_id: String,
	expected_revision: u64,
	recovery_operation_id: Option<String>,
	idempotency_key: String,
	codex_bin: String,
	login_method: account_reauthentication::LoginMethod,
}

struct AccountEnrollmentInput {
	session_id: String,
	operation_id: String,
	account_id: String,
	enabled: bool,
	idempotency_key: String,
	codex_bin: String,
	login_method: account_reauthentication::LoginMethod,
}

async fn list_accounts(profile: ClientProfile) -> Result<Value, RequestFailure> {
	to_value(AccountClient::new(profile).list().await.map_err(RequestFailure::Client)?)
}

async fn get_codex_auth_projection(profile: ClientProfile) -> Result<Value, RequestFailure> {
	to_value(
		AccountClient::new(profile)
			.codex_auth_projection()
			.await
			.map_err(RequestFailure::Client)?,
	)
}

async fn wait_for_account_observation(
	profile: ClientProfile,
	after_generation: u64,
	request_refresh: bool,
) -> Result<Value, RequestFailure> {
	let client = AccountClient::new(profile);
	let signal = if request_refresh {
		client.request_observation_refresh(after_generation).await
	} else {
		client.wait_for_observation(after_generation).await
	}
	.map_err(RequestFailure::Client)?;
	to_value(signal)
}

async fn get_reset_card_status(
	profile: ClientProfile,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let idempotency_key = parse_idempotency_key(idempotency_key)?;
	to_value(
		ResetCardClient::new(profile)
			.status(idempotency_key)
			.await
			.map_err(RequestFailure::Client)?,
	)
}

async fn enroll_account(
	profile: ClientProfile,
	operation_id: String,
	account_id: String,
	enabled: bool,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let payload = CommandPayload::EnrollAccountFromSharedCodex {
		operation_id: entity_id(&operation_id)?,
		account_id: entity_id(&account_id)?,
		enabled,
	};
	execute_account_command(profile, payload, None, idempotency_key).await
}

async fn logout_account(
	profile: ClientProfile,
	operation_id: String,
	account_id: String,
	expected_revision: u64,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let payload = CommandPayload::LogoutAccount {
		operation_id: entity_id(&operation_id)?,
		account_id: entity_id(&account_id)?,
	};
	execute_account_command(profile, payload, Some(revision(expected_revision)?), idempotency_key)
		.await
}

fn start_account_reauthentication(
	native_client: &NativeClient,
	profile: ClientProfile,
	input: AccountReauthenticationInput,
) -> Result<Value, RequestFailure> {
	validate_login_start(&input.session_id, &input.codex_bin)?;
	let operation_id = entity_id(&input.operation_id)?;
	let recovery_operation_id =
		input.recovery_operation_id.map(|operation_id| entity_id(&operation_id)).transpose()?;
	if recovery_operation_id.as_ref() == Some(&operation_id) {
		return Err(RequestFailure::Bridge(BridgeFailure::InvalidInput));
	}
	let start = account_reauthentication::Start {
		session_id: input.session_id,
		operation_id,
		account_id: entity_id(&input.account_id)?,
		idempotency_key: parse_idempotency_key(input.idempotency_key)?,
		codex_bin: PathBuf::from(input.codex_bin),
		login_method: input.login_method,
		install_mode: account_reauthentication::InstallMode::Reauthenticate {
			expected_revision: revision(input.expected_revision)?,
			recovery_operation_id,
		},
	};
	to_value(native_client.account_reauthentication.start(
		start,
		profile,
		tokio::runtime::Handle::current(),
	))
}

fn start_account_enrollment(
	native_client: &NativeClient,
	profile: ClientProfile,
	input: AccountEnrollmentInput,
) -> Result<Value, RequestFailure> {
	validate_login_start(&input.session_id, &input.codex_bin)?;
	let start = account_reauthentication::Start {
		session_id: input.session_id,
		operation_id: entity_id(&input.operation_id)?,
		account_id: entity_id(&input.account_id)?,
		idempotency_key: parse_idempotency_key(input.idempotency_key)?,
		codex_bin: PathBuf::from(input.codex_bin),
		login_method: input.login_method,
		install_mode: account_reauthentication::InstallMode::Enroll { enabled: input.enabled },
	};
	to_value(native_client.account_reauthentication.start(
		start,
		profile,
		tokio::runtime::Handle::current(),
	))
}

fn validate_login_start(session_id: &str, codex_bin: &str) -> Result<(), RequestFailure> {
	if !is_canonical_uuid(session_id)
		|| codex_bin.is_empty()
		|| codex_bin.len() > 4_096
		|| codex_bin.chars().any(char::is_control)
	{
		return Err(RequestFailure::Bridge(BridgeFailure::InvalidInput));
	}
	Ok(())
}

fn poll_account_reauthentication(
	native_client: &NativeClient,
	session_id: String,
) -> Result<Value, RequestFailure> {
	validate_session_id(&session_id)?;
	to_value(native_client.account_reauthentication.poll(&session_id))
}

fn cancel_account_reauthentication(
	native_client: &NativeClient,
	session_id: String,
) -> Result<Value, RequestFailure> {
	validate_session_id(&session_id)?;
	to_value(native_client.account_reauthentication.cancel(&session_id))
}

fn validate_session_id(session_id: &str) -> Result<(), RequestFailure> {
	if is_canonical_uuid(session_id) {
		Ok(())
	} else {
		Err(RequestFailure::Bridge(BridgeFailure::InvalidInput))
	}
}

fn fast_mode_status() -> Result<Value, RequestFailure> {
	let enabled = fast_mode::status().map_err(RequestFailure::FastMode)?;
	to_value(FastModeData { enabled })
}

fn set_fast_mode(enabled: bool) -> Result<Value, RequestFailure> {
	let enabled = fast_mode::set_enabled(enabled).map_err(RequestFailure::FastMode)?;
	to_value(FastModeData { enabled })
}

async fn get_reset_cards(
	profile: ClientProfile,
	account_id: String,
) -> Result<Value, RequestFailure> {
	to_value(
		ResetCardClient::new(profile)
			.list(entity_id(&account_id)?)
			.await
			.map_err(RequestFailure::Client)?,
	)
}

async fn get_account_profile(
	profile: ClientProfile,
	account_id: String,
	include_email: bool,
) -> Result<Value, RequestFailure> {
	to_value(
		AccountClient::new(profile)
			.profile(entity_id(&account_id)?, include_email)
			.await
			.map_err(RequestFailure::Client)?,
	)
}

async fn consume_reset_card(
	profile: ClientProfile,
	account_id: String,
	granted_at_unix_seconds: i64,
	expires_at_unix_seconds: i64,
	expected_revision: u64,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let descriptor = ResetCardDescriptorDto::new(granted_at_unix_seconds, expires_at_unix_seconds)
		.map_err(|_| RequestFailure::Bridge(BridgeFailure::InvalidInput))?;
	let response = ResetCardClient::new(profile)
		.consume(
			entity_id(&account_id)?,
			descriptor,
			revision(expected_revision)?,
			parse_idempotency_key(idempotency_key)?,
		)
		.await
		.map_err(RequestFailure::Client)?;
	to_value(ResetCardConsumeDto::from(response))
}

async fn set_fixed_selection(
	profile: ClientProfile,
	account_id: String,
	expected_account_revision: u64,
	expected_routing_revision: u64,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let response = AccountClient::new(profile)
		.set_fixed_account_selection(
			entity_id(&account_id)?,
			revision(expected_account_revision)?,
			revision(expected_routing_revision)?,
			parse_idempotency_key(idempotency_key)?,
		)
		.await
		.map_err(RequestFailure::Client)?;
	to_value(response)
}

async fn set_balanced_selection(
	profile: ClientProfile,
	expected_routing_revision: u64,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let response = AccountClient::new(profile)
		.set_balanced_account_selection(
			revision(expected_routing_revision)?,
			parse_idempotency_key(idempotency_key)?,
		)
		.await
		.map_err(RequestFailure::Client)?;
	to_value(response)
}

async fn set_account_order(
	profile: ClientProfile,
	order: Vec<String>,
	expected_routing_revision: u64,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let order = order
		.into_iter()
		.map(|account_id| entity_id(&account_id))
		.collect::<Result<Vec<_>, _>>()?;
	let response = AccountClient::new(profile)
		.set_account_order(
			order,
			revision(expected_routing_revision)?,
			parse_idempotency_key(idempotency_key)?,
		)
		.await
		.map_err(RequestFailure::Client)?;
	to_value(response)
}

async fn use_account_in_codex(
	profile: ClientProfile,
	account_id: String,
	expected_revision: u64,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let response = AccountClient::new(profile)
		.use_account_in_codex(
			entity_id(&account_id)?,
			revision(expected_revision)?,
			parse_idempotency_key(idempotency_key)?,
		)
		.await
		.map_err(RequestFailure::Client)?;
	to_value(response)
}

async fn set_account_enabled(
	profile: ClientProfile,
	account_id: String,
	enabled: bool,
	expected_revision: u64,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let payload =
		CommandPayload::SetAccountEnabled { account_id: entity_id(&account_id)?, enabled };
	execute_account_command(profile, payload, Some(revision(expected_revision)?), idempotency_key)
		.await
}

async fn execute_account_command(
	profile: ClientProfile,
	payload: CommandPayload,
	expected_revision: Option<EntityRevision>,
	idempotency_key: String,
) -> Result<Value, RequestFailure> {
	let response: AccountCommandResponse = AccountClient::new(profile)
		.execute(payload, expected_revision, parse_idempotency_key(idempotency_key)?)
		.await
		.map_err(RequestFailure::Client)?;
	to_value(response)
}

fn entity_id(value: &str) -> Result<EntityId, RequestFailure> {
	if !is_canonical_uuid(value) {
		return Err(RequestFailure::Bridge(BridgeFailure::InvalidInput));
	}
	EntityId::new(value.to_owned()).map_err(|_| RequestFailure::Bridge(BridgeFailure::InvalidInput))
}

fn revision(value: u64) -> Result<EntityRevision, RequestFailure> {
	if value == 0 {
		Err(RequestFailure::Bridge(BridgeFailure::InvalidInput))
	} else {
		Ok(EntityRevision(value))
	}
}

fn parse_idempotency_key(value: String) -> Result<IdempotencyKey, RequestFailure> {
	IdempotencyKey::new(value).map_err(|_| RequestFailure::Bridge(BridgeFailure::InvalidInput))
}

fn to_value<T: Serialize>(value: T) -> Result<Value, RequestFailure> {
	serde_json::to_value(value).map_err(|_| RequestFailure::Bridge(BridgeFailure::InternalFailure))
}

fn runtime() -> Result<&'static Runtime, ()> {
	RUNTIME
		.get_or_init(|| {
			Builder::new_multi_thread()
				.enable_all()
				.thread_name("decodex-app-client")
				.build()
				.map_err(|_| ())
		})
		.as_ref()
		.map_err(|_| ())
}

fn clients() -> &'static Mutex<HashMap<usize, Arc<NativeClient>>> {
	CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_client_id() -> usize {
	loop {
		let id = NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed);
		if id != 0 {
			return id;
		}
	}
}

fn input_bytes<'a>(input: *const u8, len: usize) -> Option<&'a [u8]> {
	if input.is_null() || len == 0 {
		return None;
	}
	// SAFETY: Callers of the public ABI functions promise that `input` points
	// to `len` readable bytes for the duration of the call.
	Some(unsafe { slice::from_raw_parts(input, len) })
}

fn write_failure(
	out_json: *mut *mut u8,
	out_len: *mut usize,
	operation: &'static str,
	failure: ResponseFailure,
) -> i32 {
	write_serialized(
		out_json,
		out_len,
		&FailureResponse { schema: RESPONSE_SCHEMA, outcome: "failure", operation, failure },
	)
}

fn write_serialized<T: Serialize>(out_json: *mut *mut u8, out_len: *mut usize, value: &T) -> i32 {
	let bytes = match serde_json::to_vec(value) {
		Ok(bytes) => bytes,
		Err(_) => return 2,
	};
	write_bytes(out_json, out_len, bytes)
}

fn write_bytes(out_json: *mut *mut u8, out_len: *mut usize, bytes: Vec<u8>) -> i32 {
	let mut bytes = bytes.into_boxed_slice();
	let len = bytes.len();
	let buffer = bytes.as_mut_ptr();
	std::mem::forget(bytes);
	// SAFETY: Public entry points checked the output pointers before calling
	// this helper. `buffer` remains owned by the caller until `free`.
	unsafe {
		*out_json = buffer;
		*out_len = len;
	}
	0
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
		})
}

#[cfg(test)]
mod tests {
	use super::*;

	const ACCOUNT_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
	const SECOND_ACCOUNT_ID: &str = "028f0f9e-7b6e-4a31-8f4c-1d2e3f405163";

	#[test]
	fn exported_abi_and_artifact_cohort_are_exact() {
		assert_eq!(decodex_app_native_client_abi_version(), ABI_VERSION);
		assert_eq!(
			decodex_app_native_client_artifact_cohort(),
			decodex_protocol::CURRENT_ARTIFACT_COHORT,
		);
	}

	#[test]
	fn strict_request_accepts_the_versioned_operations() {
		let request = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"get_reset_cards","account_id":"{ACCOUNT_ID}"}}"#
		))
		.expect("request must decode");

		assert_eq!(request.operation(), "get_reset_cards");
		assert_eq!(request.schema(), RESPONSE_SCHEMA);
	}

	#[test]
	fn observation_wait_is_exact_and_priority_refresh_is_not_a_separate_app_operation() {
		let request = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"wait_for_account_observation","after_generation":17}}"#
		))
		.expect("observation wait must decode");
		assert_eq!(request.operation(), "wait_for_account_observation");
		assert!(matches!(
			request,
			Request::WaitForAccountObservation { request_refresh: false, .. }
		));
		let priority_request = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"wait_for_account_observation","after_generation":17,"request_refresh":true}}"#
		))
		.expect("priority observation wait must decode");
		assert!(matches!(
			priority_request,
			Request::WaitForAccountObservation { request_refresh: true, .. }
		));
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"wait_for_account_observation"}}"#
			))
			.is_err()
		);
		assert!(serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"refresh_account","operation_id":"028f0f9e-7b6e-4a31-8f4c-1d2e3f405163","account_id":"{ACCOUNT_ID}","expected_revision":7,"idempotency_key":"refresh-test"}}"#
		))
		.is_err());
	}

	#[test]
	fn use_in_codex_request_is_account_revision_fenced() {
		let request = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"use_account_in_codex","account_id":"{ACCOUNT_ID}","expected_revision":7,"idempotency_key":"use-account-test"}}"#
		))
		.expect("use-in-Codex request must decode");

		assert_eq!(request.operation(), "use_account_in_codex");
		assert!(serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"use_account_in_codex","account_id":"{ACCOUNT_ID}","idempotency_key":"use-account-test"}}"#
		))
		.is_err());
	}

	#[test]
	fn account_order_request_is_exact_and_routing_revision_fenced() {
		let request = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"set_account_order","order":["{SECOND_ACCOUNT_ID}","{ACCOUNT_ID}"],"expected_routing_revision":9,"idempotency_key":"account-order-test"}}"#
		))
		.expect("account-order request must decode");

		assert_eq!(request.operation(), "set_account_order");
		assert!(serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"set_account_order","order":["{SECOND_ACCOUNT_ID}","{ACCOUNT_ID}"],"idempotency_key":"account-order-test"}}"#
		))
		.is_err());
		assert!(serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"set_account_order","order":["{SECOND_ACCOUNT_ID}","{ACCOUNT_ID}"],"expected_routing_revision":9,"idempotency_key":"account-order-test","extra":true}}"#
		))
		.is_err());
	}

	#[test]
	fn account_reauthentication_requests_are_exact_and_revision_fenced() {
		let session_id = "028f0f9e-7b6e-4a31-8f4c-1d2e3f405163";
		let operation_id = "038f0f9e-7b6e-4a31-8f4c-1d2e3f405164";
		let recovery_operation_id = "048f0f9e-7b6e-4a31-8f4c-1d2e3f405165";
		let start = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"start_account_reauthentication","session_id":"{session_id}","operation_id":"{operation_id}","account_id":"{ACCOUNT_ID}","expected_revision":7,"recovery_operation_id":"{recovery_operation_id}","idempotency_key":"{operation_id}","codex_bin":"/Applications/Codex.app/Contents/Resources/codex","login_method":"browser_redirect"}}"#
		))
		.expect("start request must decode");
		let poll = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"poll_account_reauthentication","session_id":"{session_id}"}}"#
		))
		.expect("poll request must decode");
		let cancel = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"cancel_account_reauthentication","session_id":"{session_id}"}}"#
		))
		.expect("cancel request must decode");

		assert_eq!(start.operation(), "start_account_reauthentication");
		assert!(matches!(
			start,
			Request::StartAccountReauthentication {
				recovery_operation_id: Some(ref actual),
				login_method: account_reauthentication::LoginMethod::BrowserRedirect,
				..
			} if actual == recovery_operation_id
		));
		assert_eq!(poll.operation(), "poll_account_reauthentication");
		assert_eq!(cancel.operation(), "cancel_account_reauthentication");
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"start_account_reauthentication","session_id":"{session_id}","operation_id":"{operation_id}","account_id":"{ACCOUNT_ID}","idempotency_key":"{operation_id}","codex_bin":"/Applications/Codex.app/Contents/Resources/codex","login_method":"browser_redirect"}}"#
			))
			.is_err()
		);
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"start_account_reauthentication","session_id":"{session_id}","operation_id":"{operation_id}","account_id":"{ACCOUNT_ID}","expected_revision":7,"idempotency_key":"{operation_id}","codex_bin":"/Applications/Codex.app/Contents/Resources/codex"}}"#
			))
			.is_err()
		);
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"poll_account_reauthentication","session_id":"{session_id}","extra":true}}"#
			))
			.is_err()
		);
	}

	#[test]
	fn account_enrollment_start_is_exact_and_unrevisioned() {
		let session_id = "058f0f9e-7b6e-4a31-8f4c-1d2e3f405166";
		let operation_id = "068f0f9e-7b6e-4a31-8f4c-1d2e3f405167";
		let start = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"start_account_enrollment","session_id":"{session_id}","operation_id":"{operation_id}","account_id":"{ACCOUNT_ID}","enabled":true,"idempotency_key":"{operation_id}","codex_bin":"/Applications/Codex.app/Contents/Resources/codex","login_method":"device_code"}}"#
		))
		.expect("account enrollment start must decode");

		assert_eq!(start.operation(), "start_account_enrollment");
		assert_eq!(start.schema(), RESPONSE_SCHEMA);
		assert!(matches!(
			start,
			Request::StartAccountEnrollment {
				enabled: true,
				login_method: account_reauthentication::LoginMethod::DeviceCode,
				..
			}
		));
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"start_account_enrollment","session_id":"{session_id}","operation_id":"{operation_id}","account_id":"{ACCOUNT_ID}","idempotency_key":"{operation_id}","codex_bin":"/Applications/Codex.app/Contents/Resources/codex"}}"#
			))
			.is_err()
		);
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"start_account_enrollment","session_id":"{session_id}","operation_id":"{operation_id}","account_id":"{ACCOUNT_ID}","enabled":true,"idempotency_key":"{operation_id}","codex_bin":"/Applications/Codex.app/Contents/Resources/codex"}}"#
			))
			.is_err()
		);
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"start_account_enrollment","session_id":"{session_id}","operation_id":"{operation_id}","account_id":"{ACCOUNT_ID}","enabled":true,"expected_revision":7,"idempotency_key":"{operation_id}","codex_bin":"/Applications/Codex.app/Contents/Resources/codex","login_method":"device_code"}}"#
			))
			.is_err()
		);
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"start_account_enrollment","session_id":"{session_id}","operation_id":"{operation_id}","account_id":"{ACCOUNT_ID}","enabled":true,"idempotency_key":"{operation_id}","codex_bin":"/Applications/Codex.app/Contents/Resources/codex","login_method":"future_method"}}"#
			))
			.is_err()
		);
	}

	#[test]
	fn projection_query_and_label_free_enrollment_are_exact() {
		let projection = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"get_codex_auth_projection"}}"#
		))
		.expect("projection request must decode");
		let enrollment = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"enroll_account","operation_id":"{ACCOUNT_ID}","account_id":"{ACCOUNT_ID}","enabled":true,"idempotency_key":"enroll-test"}}"#
		))
		.expect("label-free enrollment must decode");

		assert_eq!(projection.operation(), "get_codex_auth_projection");
		assert_eq!(enrollment.operation(), "enroll_account");
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"enroll_account","operation_id":"{ACCOUNT_ID}","account_id":"{ACCOUNT_ID}","display_label":"legacy","enabled":true,"idempotency_key":"enroll-test"}}"#
			))
			.is_err()
		);
		assert!(
			serde_json::from_str::<Request>(&format!(
				r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"rename_account","account_id":"{ACCOUNT_ID}","display_label":"legacy","expected_revision":7,"idempotency_key":"rename-test"}}"#
			))
			.is_err()
		);
	}

	#[test]
	fn strict_request_rejects_unknown_fields() {
		let request = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"list_accounts","extra":true}}"#
		));

		assert!(request.is_err());
	}

	#[test]
	fn fast_mode_requests_and_success_authority_are_exact() {
		let request = serde_json::from_str::<Request>(&format!(
			r#"{{"schema":"{RESPONSE_SCHEMA}","operation":"set_fast_mode","enabled":true}}"#
		))
		.expect("Fast mode request must decode");

		assert_eq!(request.operation(), "set_fast_mode");
		let response = SuccessResponse {
			schema: RESPONSE_SCHEMA,
			outcome: "success",
			operation: "set_fast_mode",
			authority: AuthorityResponse {
				profile_name: "local".into(),
				server_id: ACCOUNT_ID.into(),
			},
			data: serde_json::to_value(FastModeData { enabled: true })
				.expect("Fast mode data must serialize"),
		};
		let value = serde_json::to_value(response).expect("success response must serialize");

		assert_eq!(value["authority"]["profile_name"], "local");
		assert_eq!(value["authority"]["server_id"], ACCOUNT_ID);
		assert_eq!(value["data"]["enabled"], true);
		assert_eq!(value.as_object().expect("response must be an object").len(), 5);
	}

	#[test]
	fn response_failure_is_one_closed_string() {
		let response = FailureResponse {
			schema: RESPONSE_SCHEMA,
			outcome: "failure",
			operation: "list_accounts",
			failure: ResponseFailure::Client(ClientFailure::ProtocolTimeout),
		};
		let value = serde_json::to_value(response).expect("response must serialize");

		assert_eq!(value["failure"], "protocol_timeout");
		assert_eq!(value["outcome"], "failure");
	}

	#[test]
	fn fast_mode_failure_is_one_closed_string() {
		let response = FailureResponse {
			schema: RESPONSE_SCHEMA,
			outcome: "failure",
			operation: "fast_mode_status",
			failure: ResponseFailure::FastMode(fast_mode::FastModeFailure::ConfigInvalid),
		};
		let value = serde_json::to_value(response).expect("response must serialize");

		assert_eq!(value["failure"], "config_invalid");
		assert_eq!(value.as_object().expect("response must be an object").len(), 4);
	}

	#[test]
	fn output_buffer_round_trips_through_the_public_free_function() {
		let mut pointer = ptr::null_mut();
		let mut len = 0;
		let status = write_failure(
			&mut pointer,
			&mut len,
			"request",
			ResponseFailure::Bridge(BridgeFailure::InvalidRequest),
		);

		assert_eq!(status, 0);
		assert!(!pointer.is_null());
		assert!(len > 0);
		// SAFETY: This test passes the exact pair returned by `write_failure`.
		unsafe {
			decodex_app_native_client_free(pointer, len);
		}
	}

	#[test]
	fn account_ids_are_canonical_lowercase_uuids() {
		assert!(is_canonical_uuid(ACCOUNT_ID));
		assert!(!is_canonical_uuid("018F0F9E-7B6E-4A31-8F4C-1D2E3F405162"));
		assert!(!is_canonical_uuid("not-an-account"));
	}
}
