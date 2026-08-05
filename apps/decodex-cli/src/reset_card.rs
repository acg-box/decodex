//! Reset-card CLI client and stable output projection.

use std::{
	fmt::{Debug, Formatter, Write as _},
	path::Path,
	time::{Duration, Instant},
};

use clap::Subcommand;
use serde::Serialize;
use tokio::time;

use decodex_protocol::{
	AccountQuotaStateDto, AccountQuotaWindowDto, ClientFailure, ClientProfile, CommandError,
	EntityId, EntityRevision, IdempotencyKey, ResetCardClient, ResetCardConsumeResponse,
	ResetCardDescriptorDto, ResetCardError, ResetCardInventoryResult, ResetCardOperationResult,
	ResetCardOutcome,
};

use crate::{CommandOutput, OutputFormat, load_client_profile};

const RESET_CARD_OUTPUT_SCHEMA: &str = "decodex/reset-card-cli/1";
const OPERATION_POLL_DEADLINE: Duration = Duration::from_secs(30);
const OPERATION_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Reset-card operations served by the common daemon authority.
#[derive(Clone, Eq, PartialEq, Subcommand)]
pub enum ResetCardCommand {
	/// Read one account's bounded current public reset-card observation.
	List {
		/// Canonical vNext account UUID.
		#[arg(long, value_name = "UUID")]
		account: String,
	},
	/// Consume one explicitly selected public reset-card descriptor.
	Use {
		/// Canonical vNext account UUID.
		#[arg(long, value_name = "UUID")]
		account: String,
		/// Public reset-card grant timestamp in Unix seconds.
		#[arg(long, value_name = "SECONDS")]
		granted_at: i64,
		/// Public reset-card expiry timestamp in Unix seconds.
		#[arg(long, value_name = "SECONDS")]
		expires_at: i64,
		/// Required optimistic account revision.
		#[arg(long, value_name = "N")]
		expected_revision: u64,
		/// Stable logical-command key. Persist this value before invoking the command.
		#[arg(long, value_name = "KEY")]
		idempotency_key: String,
		/// Confirm this manual external mutation.
		#[arg(long, action = clap::ArgAction::SetTrue)]
		yes: bool,
	},
	/// Read one durable operation without replaying its consume command.
	Status {
		/// Stable logical-command key used by `reset-card use`.
		#[arg(long, value_name = "KEY")]
		idempotency_key: String,
	},
}
impl Debug for ResetCardCommand {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::List { account } =>
				formatter.debug_struct("List").field("account", account).finish(),
			Self::Use {
				account,
				granted_at,
				expires_at,
				expected_revision,
				idempotency_key: _,
				yes,
			} => formatter
				.debug_struct("Use")
				.field("account", account)
				.field("granted_at", granted_at)
				.field("expires_at", expires_at)
				.field("expected_revision", expected_revision)
				.field("idempotency_key", &"<redacted>")
				.field("yes", yes)
				.finish(),
			Self::Status { .. } =>
				formatter.debug_struct("Status").field("idempotency_key", &"<redacted>").finish(),
		}
	}
}

#[derive(Serialize)]
struct AuthorityDocument<'a> {
	profile_name: &'a str,
	server_id: &'a str,
}

#[derive(Serialize)]
struct QueryDocument<'a, T> {
	schema: &'static str,
	command: &'static str,
	outcome: &'static str,
	authority: AuthorityDocument<'a>,
	result: &'a T,
}

#[derive(Serialize)]
struct OperationDocument<'a> {
	schema: &'static str,
	command: &'static str,
	outcome: &'static str,
	idempotency_key: &'a str,
	state: ResetCardOperationResult,
}

#[derive(Serialize)]
struct UseDocument<'a> {
	schema: &'static str,
	command: &'static str,
	outcome: &'static str,
	idempotency_key: &'a str,
	dispatch_state: UseDispatchState,
	account_id: &'a EntityId,
	descriptor: ResetCardDescriptorDto,
	account_revision: EntityRevision,
	state: ResetCardOperationResult,
}

#[derive(Serialize)]
struct RejectedDocument<'a> {
	schema: &'static str,
	command: &'static str,
	outcome: &'static str,
	idempotency_key: &'a str,
	dispatch_state: UseDispatchState,
	error: &'a CommandError,
}

#[derive(Serialize)]
struct UseFailureDocument<'a> {
	schema: &'static str,
	command: &'static str,
	outcome: &'static str,
	idempotency_key: &'a str,
	dispatch_state: UseDispatchState,
	failure: &'a str,
}

#[derive(Serialize)]
struct FailureDocument<'a> {
	schema: &'static str,
	command: &'static str,
	outcome: &'static str,
	failure: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum UseDispatchState {
	DefinitelyNotDispatched,
	PotentiallyDispatched,
	DurablyAccepted,
	RejectedBeforeAcceptance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputFailure {
	InvalidAccountId,
	InvalidDescriptor,
	InvalidIdempotencyKey,
	ConfirmationRequired,
}
impl InputFailure {
	const fn code(self) -> &'static str {
		match self {
			Self::InvalidAccountId => "invalid_account_id",
			Self::InvalidDescriptor => "invalid_descriptor",
			Self::InvalidIdempotencyKey => "invalid_idempotency_key",
			Self::ConfirmationRequired => "confirmation_required",
		}
	}
}

pub(crate) async fn execute(
	command: ResetCardCommand,
	format: OutputFormat,
	root: Option<&Path>,
	selected_profile: Option<&str>,
	expected_server_id: Option<&str>,
) -> CommandOutput {
	if matches!(&command, ResetCardCommand::Use { .. }) {
		return execute_use(command, format, root, selected_profile, expected_server_id).await;
	}
	let command_name = command.name();
	let profile = load_client_profile(root, selected_profile, expected_server_id);
	let profile = match profile {
		Ok(profile) => profile,
		Err(failure) => return render_client_failure(command_name, format, failure),
	};
	let client = ResetCardClient::new(profile.clone());

	match command {
		ResetCardCommand::List { account } => {
			let account = match parse_account_id(account) {
				Ok(account) => account,
				Err(failure) => return render_input_failure(command_name, format, failure),
			};

			match client.list(account).await {
				Ok(result) => render_inventory(format, &profile, &result),
				Err(failure) => render_client_failure(command_name, format, failure),
			}
		},
		ResetCardCommand::Status { idempotency_key } => {
			let idempotency_key = match IdempotencyKey::new(idempotency_key) {
				Ok(key) => key,
				Err(_) => {
					return render_input_failure(
						command_name,
						format,
						InputFailure::InvalidIdempotencyKey,
					);
				},
			};

			match client.status(idempotency_key.clone()).await {
				Ok(state) => render_operation(format, "status", &idempotency_key, state),
				Err(failure) => render_client_failure(command_name, format, failure),
			}
		},
		ResetCardCommand::Use { .. } => {
			unreachable!("reset-card use is handled before profile loading")
		},
	}
}

async fn execute_use(
	command: ResetCardCommand,
	format: OutputFormat,
	root: Option<&Path>,
	selected_profile: Option<&str>,
	expected_server_id: Option<&str>,
) -> CommandOutput {
	let ResetCardCommand::Use {
		account,
		granted_at,
		expires_at,
		expected_revision,
		idempotency_key,
		yes,
	} = command
	else {
		unreachable!("execute_use accepts only reset-card use");
	};
	let idempotency_key = match IdempotencyKey::new(idempotency_key) {
		Ok(key) => key,
		Err(_) => return render_input_failure("use", format, InputFailure::InvalidIdempotencyKey),
	};
	if !yes {
		return render_use_input_failure(
			format,
			&idempotency_key,
			InputFailure::ConfirmationRequired,
		);
	}
	let account = match parse_account_id(account) {
		Ok(account) => account,
		Err(failure) => return render_use_input_failure(format, &idempotency_key, failure),
	};
	let descriptor = match ResetCardDescriptorDto::new(granted_at, expires_at) {
		Ok(descriptor) => descriptor,
		Err(_) => {
			return render_use_input_failure(
				format,
				&idempotency_key,
				InputFailure::InvalidDescriptor,
			);
		},
	};
	let profile = load_client_profile(root, selected_profile, expected_server_id);
	let profile = match profile {
		Ok(profile) => profile,
		Err(failure) => {
			return render_use_client_failure(
				format,
				&idempotency_key,
				UseDispatchState::DefinitelyNotDispatched,
				failure,
			);
		},
	};
	let client = ResetCardClient::new(profile);

	match client
		.consume(account, descriptor, EntityRevision(expected_revision), idempotency_key.clone())
		.await
	{
		Ok(ResetCardConsumeResponse::Accepted {
			account_id,
			descriptor,
			state,
			entity_revision,
		}) => {
			let state = match state {
				ResetCardOperationResult::Prepared =>
					accepted_state_after_poll(poll_operation(&client, &idempotency_key).await),
				state => state,
			};

			render_use(format, &idempotency_key, &account_id, descriptor, entity_revision, state)
		},
		Ok(ResetCardConsumeResponse::Rejected { error }) =>
			render_rejected(format, &idempotency_key, &error),
		Ok(ResetCardConsumeResponse::PotentiallyDispatched { failure }) =>
			render_use_client_failure(
				format,
				&idempotency_key,
				UseDispatchState::PotentiallyDispatched,
				failure,
			),
		Err(failure) => render_use_client_failure(
			format,
			&idempotency_key,
			UseDispatchState::DefinitelyNotDispatched,
			failure,
		),
	}
}

fn accepted_state_after_poll(
	result: Result<ResetCardOperationResult, ClientFailure>,
) -> ResetCardOperationResult {
	result.unwrap_or(ResetCardOperationResult::Prepared)
}

impl ResetCardCommand {
	const fn name(&self) -> &'static str {
		match self {
			Self::List { .. } => "list",
			Self::Use { .. } => "use",
			Self::Status { .. } => "status",
		}
	}
}

async fn poll_operation(
	client: &ResetCardClient,
	idempotency_key: &IdempotencyKey,
) -> Result<ResetCardOperationResult, ClientFailure> {
	let deadline = Instant::now() + OPERATION_POLL_DEADLINE;

	loop {
		let remaining = deadline.saturating_duration_since(Instant::now());

		if remaining.is_zero() {
			return Ok(ResetCardOperationResult::Prepared);
		}

		let state = match time::timeout(remaining, client.status(idempotency_key.clone())).await {
			Ok(result) => result?,
			Err(_) => return Ok(ResetCardOperationResult::Prepared),
		};

		if state != ResetCardOperationResult::Prepared || Instant::now() >= deadline {
			return Ok(state);
		}

		time::sleep(
			OPERATION_POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
		)
		.await;
	}
}

fn parse_account_id(value: String) -> Result<EntityId, InputFailure> {
	if !is_canonical_uuid(&value) {
		return Err(InputFailure::InvalidAccountId);
	}

	EntityId::new(value).map_err(|_| InputFailure::InvalidAccountId)
}

fn render_inventory(
	format: OutputFormat,
	profile: &ClientProfile,
	result: &ResetCardInventoryResult,
) -> CommandOutput {
	let (outcome, exit_code) = match result {
		ResetCardInventoryResult::Available { .. } => ("available", 0),
		ResetCardInventoryResult::ObservationFailed { .. } => ("observation_failed", 1),
		ResetCardInventoryResult::Unavailable { .. } => ("unavailable", 1),
	};
	let text = match format {
		OutputFormat::Json => serde_json::to_string(&QueryDocument {
			schema: RESET_CARD_OUTPUT_SCHEMA,
			command: "list",
			outcome,
			authority: authority_document(profile),
			result,
		})
		.expect("bounded reset-card inventory serialization cannot fail"),
		OutputFormat::Human => match result {
			ResetCardInventoryResult::Available {
				account_id,
				account_revision,
				reported_available_count,
				details_complete,
				cards,
				five_hour_quota: _,
				seven_day_quota: _,
			} => {
				let count = reported_available_count
					.map(|value| value.to_string())
					.unwrap_or_else(|| "not reported".into());
				let detail_state =
					if *details_complete { "complete" } else { "details unavailable" };
				let mut output = format!(
					concat!(
						"reset cards for {}: {} available, {} (revision {})\n",
						"profile: {}\nserver: {}"
					),
					account_id.as_str(),
					count,
					detail_state,
					account_revision.0,
					profile.name(),
					profile.expected_server_id().as_str(),
				);

				for card in cards {
					let _ = write!(
						output,
						"\ngranted_at={} expires_at={}",
						card.descriptor.granted_at_unix_seconds(),
						card.descriptor.expires_at_unix_seconds(),
					);
				}

				output
			},
			ResetCardInventoryResult::ObservationFailed {
				account_id,
				account_revision,
				five_hour_quota,
				seven_day_quota,
				error,
			} => format!(
				concat!(
					"reset-card observation failed for {} (revision {}): {}\n",
					"five_hour={}\nseven_day={}\nprofile: {}\nserver: {}"
				),
				account_id.as_str(),
				account_revision.0,
				reset_error_name(*error),
				quota_summary(five_hour_quota),
				quota_summary(seven_day_quota),
				profile.name(),
				profile.expected_server_id().as_str(),
			),
			ResetCardInventoryResult::Unavailable { error } => {
				format!(
					"reset-card inventory unavailable: {}\nprofile: {}\nserver: {}",
					reset_error_name(*error),
					profile.name(),
					profile.expected_server_id().as_str(),
				)
			},
		},
	};

	CommandOutput { text, exit_code, error_stream: false }
}

fn quota_summary(quota: &AccountQuotaWindowDto) -> String {
	let observed =
		quota.observed_at_unix_micros.map_or_else(|| "none".to_owned(), |value| value.to_string());
	let result = match quota.result {
		AccountQuotaStateDto::Unknown => "unknown".to_owned(),
		AccountQuotaStateDto::Current { used_percent, resets_at_unix_micros } =>
			format!("current:{used_percent}:{resets_at_unix_micros}"),
		AccountQuotaStateDto::Error { error } => format!("error:{error:?}"),
	};
	format!("{}:{observed}:{result}", quota.duration_minutes)
}

fn authority_document(profile: &ClientProfile) -> AuthorityDocument<'_> {
	AuthorityDocument {
		profile_name: profile.name(),
		server_id: profile.expected_server_id().as_str(),
	}
}

fn render_operation(
	format: OutputFormat,
	command: &'static str,
	idempotency_key: &IdempotencyKey,
	state: ResetCardOperationResult,
) -> CommandOutput {
	let exit_code = operation_exit_code(state);
	let outcome = operation_state_name(state);
	let text = match format {
		OutputFormat::Json => serde_json::to_string(&OperationDocument {
			schema: RESET_CARD_OUTPUT_SCHEMA,
			command,
			outcome,
			idempotency_key: idempotency_key.as_str(),
			state,
		})
		.expect("bounded reset-card operation serialization cannot fail"),
		OutputFormat::Human => format!(
			"reset-card operation {}: {}\nidempotency_key: {}",
			outcome,
			operation_detail(state),
			idempotency_key.as_str(),
		),
	};

	CommandOutput { text, exit_code, error_stream: false }
}

fn render_use(
	format: OutputFormat,
	idempotency_key: &IdempotencyKey,
	account_id: &EntityId,
	descriptor: ResetCardDescriptorDto,
	account_revision: EntityRevision,
	state: ResetCardOperationResult,
) -> CommandOutput {
	let exit_code = operation_exit_code(state);
	let outcome = operation_state_name(state);
	let text = match format {
		OutputFormat::Json => serde_json::to_string(&UseDocument {
			schema: RESET_CARD_OUTPUT_SCHEMA,
			command: "use",
			outcome,
			idempotency_key: idempotency_key.as_str(),
			dispatch_state: UseDispatchState::DurablyAccepted,
			account_id,
			descriptor,
			account_revision,
			state,
		})
		.expect("bounded reset-card use serialization cannot fail"),
		OutputFormat::Human => format!(
			concat!(
				"reset-card use {}: {}\naccount: {}\nrevision: {}\n",
				"idempotency_key: {}\ndispatch_state: durably_accepted"
			),
			outcome,
			operation_detail(state),
			account_id.as_str(),
			account_revision.0,
			idempotency_key.as_str(),
		),
	};

	CommandOutput { text, exit_code, error_stream: false }
}

fn render_rejected(
	format: OutputFormat,
	idempotency_key: &IdempotencyKey,
	error: &CommandError,
) -> CommandOutput {
	let text = match format {
		OutputFormat::Json => serde_json::to_string(&RejectedDocument {
			schema: RESET_CARD_OUTPUT_SCHEMA,
			command: "use",
			outcome: "rejected",
			idempotency_key: idempotency_key.as_str(),
			dispatch_state: UseDispatchState::RejectedBeforeAcceptance,
			error,
		})
		.expect("bounded reset-card rejection serialization cannot fail"),
		OutputFormat::Human => format!(
			concat!(
				"reset-card use rejected: {}\nidempotency_key: {}\n",
				"dispatch_state: rejected_before_acceptance"
			),
			command_error_name(error),
			idempotency_key.as_str(),
		),
	};

	CommandOutput { text, exit_code: 1, error_stream: false }
}

fn render_use_client_failure(
	format: OutputFormat,
	idempotency_key: &IdempotencyKey,
	dispatch_state: UseDispatchState,
	failure: ClientFailure,
) -> CommandOutput {
	let code = client_failure_code(failure);
	let human = failure.to_string();

	render_use_failure(format, idempotency_key, dispatch_state, code, &human)
}

fn render_use_input_failure(
	format: OutputFormat,
	idempotency_key: &IdempotencyKey,
	failure: InputFailure,
) -> CommandOutput {
	render_use_failure(
		format,
		idempotency_key,
		UseDispatchState::DefinitelyNotDispatched,
		failure.code(),
		failure.code(),
	)
}

fn render_use_failure(
	format: OutputFormat,
	idempotency_key: &IdempotencyKey,
	dispatch_state: UseDispatchState,
	code: &'static str,
	human: &str,
) -> CommandOutput {
	let (text, error_stream) = match format {
		OutputFormat::Json => (
			serde_json::to_string(&UseFailureDocument {
				schema: RESET_CARD_OUTPUT_SCHEMA,
				command: "use",
				outcome: "failure",
				idempotency_key: idempotency_key.as_str(),
				dispatch_state,
				failure: code,
			})
			.expect("closed reset-card use failure serialization cannot fail"),
			false,
		),
		OutputFormat::Human => (
			format!(
				concat!(
					"decodex reset-card use failed: {}\nidempotency_key: {}\n",
					"dispatch_state: {}"
				),
				human,
				idempotency_key.as_str(),
				use_dispatch_state_name(dispatch_state),
			),
			true,
		),
	};

	CommandOutput { text, exit_code: 2, error_stream }
}

fn render_client_failure(
	command: &'static str,
	format: OutputFormat,
	failure: ClientFailure,
) -> CommandOutput {
	render_failure(command, format, client_failure_code(failure), &failure.to_string())
}

fn render_input_failure(
	command: &'static str,
	format: OutputFormat,
	failure: InputFailure,
) -> CommandOutput {
	render_failure(command, format, failure.code(), failure.code())
}

fn render_failure(
	command: &'static str,
	format: OutputFormat,
	code: &'static str,
	human: &str,
) -> CommandOutput {
	let (text, error_stream) = match format {
		OutputFormat::Json => (
			serde_json::to_string(&FailureDocument {
				schema: RESET_CARD_OUTPUT_SCHEMA,
				command,
				outcome: "failure",
				failure: code,
			})
			.expect("closed reset-card failure serialization cannot fail"),
			false,
		),
		OutputFormat::Human => (format!("decodex reset-card {command} failed: {human}"), true),
	};

	CommandOutput { text, exit_code: 2, error_stream }
}

const fn operation_exit_code(state: ResetCardOperationResult) -> u8 {
	match state {
		ResetCardOperationResult::Completed { .. } => 0,
		ResetCardOperationResult::Unavailable { .. } => 2,
		ResetCardOperationResult::NotFound
		| ResetCardOperationResult::Prepared
		| ResetCardOperationResult::EffectAmbiguous
		| ResetCardOperationResult::FailedBeforeEffect { .. } => 1,
	}
}

const fn use_dispatch_state_name(state: UseDispatchState) -> &'static str {
	match state {
		UseDispatchState::DefinitelyNotDispatched => "definitely_not_dispatched",
		UseDispatchState::PotentiallyDispatched => "potentially_dispatched",
		UseDispatchState::DurablyAccepted => "durably_accepted",
		UseDispatchState::RejectedBeforeAcceptance => "rejected_before_acceptance",
	}
}

const fn operation_state_name(state: ResetCardOperationResult) -> &'static str {
	match state {
		ResetCardOperationResult::NotFound => "not_found",
		ResetCardOperationResult::Prepared => "prepared",
		ResetCardOperationResult::EffectAmbiguous => "effect_ambiguous",
		ResetCardOperationResult::Completed { .. } => "completed",
		ResetCardOperationResult::FailedBeforeEffect { .. } => "failed_before_effect",
		ResetCardOperationResult::Unavailable { .. } => "unavailable",
	}
}

fn operation_detail(state: ResetCardOperationResult) -> String {
	match state {
		ResetCardOperationResult::NotFound => "operation not found".into(),
		ResetCardOperationResult::Prepared => "operation is prepared".into(),
		ResetCardOperationResult::EffectAmbiguous =>
			"provider effect requires authoritative reconciliation".into(),
		ResetCardOperationResult::Completed { outcome } => {
			format!("completed({})", reset_outcome_name(outcome))
		},
		ResetCardOperationResult::FailedBeforeEffect { error } => {
			format!("failed_before_effect({})", reset_error_name(error))
		},
		ResetCardOperationResult::Unavailable { error } => {
			format!("unavailable({})", reset_error_name(error))
		},
	}
}

const fn reset_outcome_name(outcome: ResetCardOutcome) -> &'static str {
	match outcome {
		ResetCardOutcome::Reset => "reset",
		ResetCardOutcome::NothingToReset => "nothing_to_reset",
		ResetCardOutcome::NoCredit => "no_credit",
		ResetCardOutcome::AlreadyRedeemed => "already_redeemed",
	}
}

const fn reset_error_name(error: ResetCardError) -> &'static str {
	match error {
		ResetCardError::InvalidRequest => "invalid_request",
		ResetCardError::AccountNotFound => "account_not_found",
		ResetCardError::AccountStateRejected => "account_state_rejected",
		ResetCardError::VaultUnavailable => "vault_unavailable",
		ResetCardError::SchemaUnsupported => "schema_unsupported",
		ResetCardError::ProviderUnavailable => "provider_unavailable",
		ResetCardError::InventoryIncomplete => "inventory_incomplete",
		ResetCardError::InventoryChanged => "inventory_changed",
		ResetCardError::RequestTimedOut => "request_timed_out",
		ResetCardError::ResourceExhausted => "resource_exhausted",
		ResetCardError::ProductStateUnavailable => "product_state_unavailable",
		ResetCardError::EffectAmbiguous => "effect_ambiguous",
	}
}

const fn command_error_name(error: &CommandError) -> &'static str {
	match error {
		CommandError::ExpectedRevisionMismatch { .. } => "expected_revision_mismatch",
		CommandError::IdempotencyConflict => "idempotency_conflict",
		CommandError::IdempotencyCapacityExceeded { .. } => "idempotency_capacity_exceeded",
		CommandError::ApplicationUnavailable { .. } => "application_unavailable",
		CommandError::QuickTaskUnavailable { .. } => "quick_task_unavailable",
		CommandError::QuickTaskRecoveryRequired { .. } => "quick_task_recovery_required",
		CommandError::AcceptanceUnknown => "acceptance_unknown",
		CommandError::AccountCommandRejected { .. } => "account_command_rejected",
	}
}

const fn client_failure_code(failure: ClientFailure) -> &'static str {
	match failure {
		ClientFailure::ConfigurationMissing => "configuration_missing",
		ClientFailure::ConfigurationMalformed => "configuration_malformed",
		ClientFailure::ConfigurationVersion => "configuration_version",
		ClientFailure::ProfileMissing => "profile_missing",
		ClientFailure::UnsafeHostPath => "unsafe_host_path",
		ClientFailure::ServerIdentityUnavailable => "server_identity_unavailable",
		ClientFailure::LocalTransportDisabled => "local_transport_disabled",
		ClientFailure::RemoteTransportDisabled => "remote_transport_disabled",
		ClientFailure::LocalTransportUnsupported => "local_transport_unsupported",
		ClientFailure::UnsafeLocalEndpoint => "unsafe_local_endpoint",
		ClientFailure::LocalPeerIdentityUnavailable => "local_peer_identity_unavailable",
		ClientFailure::LocalPeerUidMismatch => "local_peer_uid_mismatch",
		ClientFailure::ProtocolDisconnected => "protocol_disconnected",
		ClientFailure::ProtocolTimeout => "protocol_timeout",
		ClientFailure::ProtocolMajorMismatch => "protocol_major_mismatch",
		ClientFailure::ProtocolMinorMismatch => "protocol_minor_mismatch",
		ClientFailure::ServerIdentityMismatch => "server_identity_mismatch",
		ClientFailure::ProtocolMalformed => "protocol_malformed",
		ClientFailure::ProtocolViolation => "protocol_violation",
		ClientFailure::ProtocolBackpressure => "protocol_backpressure",
		ClientFailure::RemoteMutationUnsupported => "remote_mutation_unsupported",
		ClientFailure::ApplicationAcceptanceUnknown => "application_acceptance_unknown",
	}
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
	use std as standard;

	use clap::{CommandFactory as _, Parser as _};
	use decodex_protocol::{
		ClientFailure, ClientProfile, CommandError, EntityId, EntityRevision, IdempotencyKey,
		ResetCardDescriptorDto, ResetCardError, ResetCardInventoryResult, ResetCardOperationResult,
		ResetCardOutcome,
	};

	use crate::{Cli, Command, OutputFormat};

	const SERVER_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

	fn write_client_config(root: &std::path::Path, kind: &str) {
		#[cfg(unix)]
		let service_owner_uid = {
			use std::os::unix::fs::MetadataExt as _;

			standard::fs::metadata(root).expect("test operation must succeed").uid()
		};
		#[cfg(not(unix))]
		let service_owner_uid = 0_u32;
		let profile = match kind {
			"local" => format!(
				r#"kind = "local"
policy = "same_uid"
service_owner_uid = {service_owner_uid}
expected_server_identity = "{SERVER_ID}""#
			),
			"remote" => format!(
				r#"kind = "remote"
host = "remote.example.test"
port = 49152
expected_server_identity = "{SERVER_ID}""#
			),
			_ => panic!("unsupported test profile kind"),
		};
		let config = format!(
			r#"version = 1
active_profile = "selected"
postgres = {{}}
cache = {{}}

[profiles.selected]
{profile}
"#
		);
		let path = root.join("config.toml");

		standard::fs::write(&path, config).expect("test operation must succeed");

		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt as _;

			standard::fs::set_permissions(path, standard::fs::Permissions::from_mode(0o600))
				.expect("test operation must succeed");
		}
	}

	fn prepare_client_root(temp: &tempfile::TempDir, kind: &str) -> std::path::PathBuf {
		let root =
			temp.path().canonicalize().expect("test operation must succeed").join(".decodex");

		standard::fs::create_dir(&root).expect("test operation must succeed");

		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt as _;

			standard::fs::set_permissions(&root, standard::fs::Permissions::from_mode(0o700))
				.expect("test operation must succeed");
		}

		write_client_config(&root, kind);

		root
	}

	fn local_profile() -> ClientProfile {
		let temp = tempfile::TempDir::new().expect("test operation must succeed");
		let root = prepare_client_root(&temp, "local");

		ClientProfile::load(&root, None).expect("test operation must succeed")
	}

	#[test]
	fn reset_card_surface_parses_exact_commands_and_requires_confirmation() {
		Cli::command().debug_assert();

		let list = Cli::try_parse_from([
			"decodex",
			"reset-card",
			"list",
			"--account",
			"40000000-0000-4000-8000-000000000001",
		])
		.expect("test operation must succeed");
		let use_card = Cli::try_parse_from([
			"decodex",
			"--output",
			"json",
			"reset-card",
			"use",
			"--account",
			"40000000-0000-4000-8000-000000000001",
			"--granted-at",
			"1700000000",
			"--expires-at",
			"1700003600",
			"--expected-revision",
			"7",
			"--idempotency-key",
			"operator-key",
			"--yes",
		])
		.expect("test operation must succeed");
		let status = Cli::try_parse_from([
			"decodex",
			"reset-card",
			"status",
			"--idempotency-key",
			"operator-key",
		])
		.expect("test operation must succeed");

		assert!(matches!(list.command, Command::ResetCard(super::ResetCardCommand::List { .. })));
		assert!(matches!(
			use_card.command,
			Command::ResetCard(super::ResetCardCommand::Use {
				expected_revision: 7,
				yes: true,
				..
			})
		));
		assert_eq!(use_card.output, OutputFormat::Json);
		assert!(matches!(
			status.command,
			Command::ResetCard(super::ResetCardCommand::Status { .. })
		));
		assert!(
			Cli::try_parse_from([
				"decodex",
				"reset-card",
				"use",
				"--account",
				"40000000-0000-4000-8000-000000000001",
				"--granted-at",
				"1",
				"--expires-at",
				"2",
				"--expected-revision",
				"1",
				"--yes",
			])
			.is_err()
		);
	}

	#[test]
	fn cli_debug_redacts_reset_card_idempotency_keys() {
		let marker = "reset-card-idempotency-secret-marker";
		for args in [
			vec!["decodex", "reset-card", "status", "--idempotency-key", marker],
			vec![
				"decodex",
				"reset-card",
				"use",
				"--account",
				"40000000-0000-4000-8000-000000000001",
				"--granted-at",
				"1",
				"--expires-at",
				"2",
				"--expected-revision",
				"1",
				"--idempotency-key",
				marker,
				"--yes",
			],
		] {
			let cli = Cli::try_parse_from(args).expect("test operation must succeed");

			assert!(!format!("{cli:?}").contains(marker));
		}
	}

	#[test]
	fn reset_card_surface_accepts_and_redacts_the_explicit_server_pin() {
		let cli = Cli::try_parse_from([
			"decodex",
			"reset-card",
			"list",
			"--account",
			"40000000-0000-4000-8000-000000000001",
			"--expected-server-id",
			SERVER_ID,
		])
		.expect("test operation must succeed");
		let debug = format!("{cli:?}");

		assert_eq!(cli.expected_server_id.as_deref(), Some(SERVER_ID));
		assert!(!debug.contains(SERVER_ID));
		assert!(debug.contains("server_identity_selected: true"));
	}

	#[cfg(unix)]
	#[tokio::test]
	async fn valid_use_key_is_retained_for_every_pre_send_failure() {
		let temp = tempfile::TempDir::new().expect("test operation must succeed");
		let temp = temp.path().canonicalize().expect("test operation must succeed");
		let target = temp.join("root");
		let root = temp.join("root-link");

		standard::fs::create_dir(&target).expect("test operation must succeed");
		standard::os::unix::fs::symlink(&target, &root).expect("test operation must succeed");
		let cases = [
			(
				super::ResetCardCommand::Use {
					account: "40000000-0000-4000-8000-000000000001".into(),
					granted_at: 1,
					expires_at: 2,
					expected_revision: 1,
					idempotency_key: "operator-key".into(),
					yes: false,
				},
				"confirmation_required",
			),
			(
				super::ResetCardCommand::Use {
					account: "not-an-account".into(),
					granted_at: 1,
					expires_at: 2,
					expected_revision: 1,
					idempotency_key: "operator-key".into(),
					yes: true,
				},
				"invalid_account_id",
			),
			(
				super::ResetCardCommand::Use {
					account: "40000000-0000-4000-8000-000000000001".into(),
					granted_at: 2,
					expires_at: 1,
					expected_revision: 1,
					idempotency_key: "operator-key".into(),
					yes: true,
				},
				"invalid_descriptor",
			),
			(
				super::ResetCardCommand::Use {
					account: "40000000-0000-4000-8000-000000000001".into(),
					granted_at: 1,
					expires_at: 2,
					expected_revision: 1,
					idempotency_key: "operator-key".into(),
					yes: true,
				},
				"unsafe_host_path",
			),
		];

		for (command, expected_failure) in cases {
			let output = super::execute(command, OutputFormat::Json, Some(&root), None, None).await;
			let value: serde_json::Value =
				serde_json::from_str(output.text()).expect("test operation must succeed");

			assert_eq!(value["command"], "use");
			assert_eq!(value["outcome"], "failure");
			assert_eq!(value["idempotency_key"], "operator-key");
			assert_eq!(value["dispatch_state"], "definitely_not_dispatched");
			assert_eq!(value["failure"], expected_failure);
			assert_eq!(output.exit_code(), 2);
		}
	}

	#[tokio::test]
	async fn omitted_confirmation_reaches_stable_key_preserving_output() {
		let root = tempfile::TempDir::new().expect("test operation must succeed");
		let cli = Cli::try_parse_from([
			"decodex",
			"--root",
			root.path().to_str().expect("test operation must succeed"),
			"--output",
			"json",
			"reset-card",
			"use",
			"--account",
			"40000000-0000-4000-8000-000000000001",
			"--granted-at",
			"1",
			"--expires-at",
			"2",
			"--expected-revision",
			"1",
			"--idempotency-key",
			"operator-key",
		])
		.expect("test operation must succeed");
		let output = crate::execute(cli).await;
		let value: serde_json::Value =
			serde_json::from_str(output.text()).expect("test operation must succeed");

		assert_eq!(value["failure"], "confirmation_required");
		assert_eq!(value["idempotency_key"], "operator-key");
		assert_eq!(value["dispatch_state"], "definitely_not_dispatched");
	}

	#[tokio::test]
	async fn invalid_use_key_wins_before_other_pre_send_validation() {
		let root = tempfile::TempDir::new().expect("test operation must succeed");
		let output = super::execute(
			super::ResetCardCommand::Use {
				account: "not-an-account".into(),
				granted_at: 2,
				expires_at: 1,
				expected_revision: 1,
				idempotency_key: "\n".into(),
				yes: false,
			},
			OutputFormat::Json,
			Some(root.path()),
			None,
			None,
		)
		.await;
		let value: serde_json::Value =
			serde_json::from_str(output.text()).expect("test operation must succeed");

		assert_eq!(value["failure"], "invalid_idempotency_key");
		assert!(value.get("idempotency_key").is_none());
		assert!(value.get("dispatch_state").is_none());
	}

	#[tokio::test]
	async fn invalid_server_pin_preserves_a_valid_use_key_before_dispatch() {
		let temp = tempfile::TempDir::new().expect("test operation must succeed");
		let root = prepare_client_root(&temp, "local");

		let output = super::execute(
			super::ResetCardCommand::Use {
				account: "40000000-0000-4000-8000-000000000001".into(),
				granted_at: 1,
				expires_at: 2,
				expected_revision: 1,
				idempotency_key: "operator-key".into(),
				yes: true,
			},
			OutputFormat::Json,
			Some(&root),
			None,
			Some("not-a-canonical-server-id"),
		)
		.await;
		let value: serde_json::Value =
			serde_json::from_str(output.text()).expect("test operation must succeed");

		assert_eq!(value["failure"], "configuration_malformed");
		assert_eq!(value["idempotency_key"], "operator-key");
		assert_eq!(value["dispatch_state"], "definitely_not_dispatched");
	}

	#[tokio::test]
	async fn remote_profile_is_rejected_before_reset_card_transport() {
		let temp = tempfile::TempDir::new().expect("test operation must succeed");
		let root = prepare_client_root(&temp, "remote");

		let output = super::execute(
			super::ResetCardCommand::List {
				account: "40000000-0000-4000-8000-000000000001".to_owned(),
			},
			OutputFormat::Json,
			Some(&root),
			None,
			None,
		)
		.await;
		let value: serde_json::Value =
			serde_json::from_str(output.text()).expect("test operation must succeed");

		assert_eq!(value["failure"], "remote_mutation_unsupported");
		assert_eq!(output.exit_code(), 2);
	}

	#[test]
	fn inventory_json_binds_the_selected_profile_and_server() {
		let profile = local_profile();
		let inventory = super::render_inventory(
			OutputFormat::Json,
			&profile,
			&ResetCardInventoryResult::Unavailable {
				error: ResetCardError::ProductStateUnavailable,
			},
		);

		let value: serde_json::Value =
			serde_json::from_str(inventory.text()).expect("test operation must succeed");

		assert_eq!(value["authority"]["profile_name"], "selected");
		assert_eq!(value["authority"]["server_id"], SERVER_ID);
	}

	#[test]
	fn stable_json_use_result_has_no_provider_identifier() {
		let key = IdempotencyKey::new("operator-key").expect("test operation must succeed");
		let account = EntityId::new("40000000-0000-4000-8000-000000000001")
			.expect("test operation must succeed");
		let output = super::render_use(
			OutputFormat::Json,
			&key,
			&account,
			ResetCardDescriptorDto::new(1, 2).expect("test operation must succeed"),
			EntityRevision(8),
			ResetCardOperationResult::Completed { outcome: ResetCardOutcome::Reset },
		);
		let value: serde_json::Value =
			serde_json::from_str(output.text()).expect("test operation must succeed");

		assert_eq!(value["schema"], "decodex/reset-card-cli/1");
		assert_eq!(value["command"], "use");
		assert_eq!(value["outcome"], "completed");
		assert_eq!(value["idempotency_key"], "operator-key");
		assert_eq!(value["dispatch_state"], "durably_accepted");
		assert_eq!(value["account_id"], account.as_str());
		assert_eq!(value["state"]["state"], "completed");
		assert!(value.get("credit_id").is_none());
		assert_eq!(output.exit_code(), 0);
	}

	#[test]
	fn use_outputs_retain_the_key_and_typed_dispatch_state_after_key_creation() {
		let key = IdempotencyKey::new("operator-key").expect("test operation must succeed");

		for (dispatch_state, expected) in [
			(super::UseDispatchState::DefinitelyNotDispatched, "definitely_not_dispatched"),
			(super::UseDispatchState::PotentiallyDispatched, "potentially_dispatched"),
		] {
			let output = super::render_use_client_failure(
				OutputFormat::Json,
				&key,
				dispatch_state,
				ClientFailure::ProtocolDisconnected,
			);
			let value: serde_json::Value =
				serde_json::from_str(output.text()).expect("test operation must succeed");

			assert_eq!(value["schema"], "decodex/reset-card-cli/1");
			assert_eq!(value["command"], "use");
			assert_eq!(value["outcome"], "failure");
			assert_eq!(value["idempotency_key"], "operator-key");
			assert_eq!(value["dispatch_state"], expected);
			assert_eq!(value["failure"], "protocol_disconnected");
			assert_eq!(output.exit_code(), 2);
			assert!(!output.is_error_stream());
		}

		let rejected =
			super::render_rejected(OutputFormat::Json, &key, &CommandError::IdempotencyConflict);
		let value: serde_json::Value =
			serde_json::from_str(rejected.text()).expect("test operation must succeed");

		assert_eq!(value["idempotency_key"], "operator-key");
		assert_eq!(value["dispatch_state"], "rejected_before_acceptance");

		let unknown = super::render_use_client_failure(
			OutputFormat::Json,
			&key,
			super::UseDispatchState::PotentiallyDispatched,
			ClientFailure::ApplicationAcceptanceUnknown,
		);
		let value: serde_json::Value =
			serde_json::from_str(unknown.text()).expect("test operation must succeed");

		assert_eq!(value["failure"], "application_acceptance_unknown");
		assert_eq!(value["dispatch_state"], "potentially_dispatched");

		let human = super::render_use_client_failure(
			OutputFormat::Human,
			&key,
			super::UseDispatchState::PotentiallyDispatched,
			ClientFailure::ProtocolTimeout,
		);

		assert!(human.text().contains("idempotency_key: operator-key"));
		assert!(human.text().contains("dispatch_state: potentially_dispatched"));
		assert!(human.is_error_stream());
	}

	#[test]
	fn operation_exit_codes_distinguish_terminal_success_from_uncertainty() {
		let key = IdempotencyKey::new("operator-key").expect("test operation must succeed");

		assert_eq!(
			super::render_operation(
				OutputFormat::Json,
				"status",
				&key,
				ResetCardOperationResult::Completed { outcome: ResetCardOutcome::AlreadyRedeemed },
			)
			.exit_code(),
			0,
		);
		assert_eq!(
			super::render_operation(
				OutputFormat::Json,
				"status",
				&key,
				ResetCardOperationResult::EffectAmbiguous,
			)
			.exit_code(),
			1,
		);
		let unavailable = super::render_operation(
			OutputFormat::Json,
			"status",
			&key,
			ResetCardOperationResult::Unavailable {
				error: decodex_protocol::ResetCardError::ProductStateUnavailable,
			},
		);
		let value: serde_json::Value =
			serde_json::from_str(unavailable.text()).expect("test operation must succeed");

		assert_eq!(unavailable.exit_code(), 2);
		assert_eq!(value["outcome"], "unavailable");
		assert_eq!(value["state"]["state"], "unavailable");
	}

	#[test]
	fn status_poll_failure_cannot_downgrade_proved_durable_acceptance() {
		let state = super::accepted_state_after_poll(Err(ClientFailure::ProtocolDisconnected));
		let key = IdempotencyKey::new("operator-key").expect("test operation must succeed");
		let account = EntityId::new("40000000-0000-4000-8000-000000000001")
			.expect("test operation must succeed");
		let output = super::render_use(
			OutputFormat::Json,
			&key,
			&account,
			ResetCardDescriptorDto::new(1, 2).expect("test operation must succeed"),
			EntityRevision(7),
			state,
		);
		let value: serde_json::Value =
			serde_json::from_str(output.text()).expect("test operation must succeed");

		assert_eq!(state, ResetCardOperationResult::Prepared);
		assert_eq!(value["dispatch_state"], "durably_accepted");
		assert_eq!(value["state"]["state"], "prepared");
		assert_eq!(output.exit_code(), 1);
	}
}
