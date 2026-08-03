//! Operator account client over the same-UID V2.0 daemon protocol.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use decodex_protocol::{
	AccountClient, AccountCommandResponse, AccountManualRecoveryActionDto, CommandPayload,
	EntityId, EntityRevision, IdempotencyKey, WireText,
};
use serde::Serialize;

use crate::{CommandOutput, OutputFormat, load_client_profile};

const ACCOUNT_OUTPUT_SCHEMA: &str = "decodex/cli-account/1";

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum AccountCommand {
	/// Read the canonical fast account skeleton and routing controls.
	List,
	/// Inspect one daemon-owned account row.
	Inspect(AccountIdentityArgs),
	/// Read one bounded daemon-observed account profile.
	Profile(AccountProfileArgs),
	/// Read the current normal shared Codex authentication projection.
	CodexProjection,
	/// Enroll credentials from the normal shared Codex auth file.
	Enroll(EnrollArgs),
	/// Import one owner-private versioned credential file.
	Import(ImportArgs),
	/// Project one exact daemon account into normal shared Codex auth.
	UseInCodex(AdministrationArgs),
	/// Enable new work admission for one account.
	Enable(AdministrationArgs),
	/// Disable new work admission for one account.
	Disable(AdministrationArgs),
	/// Log out and tombstone one account.
	Logout(OperationAccountArgs),
	/// Select one fixed account.
	SetFixedSelection(FixedSelectionArgs),
	/// Select balanced initial account routing.
	SetBalancedSelection(RoutingRevisionArgs),
	/// Replace the complete deterministic account order.
	SetAccountOrder(AccountOrderArgs),
	/// Refresh one account through the serialized daemon path.
	Refresh(OperationAccountArgs),
	/// Apply one typed manual credential-operation recovery action.
	Recover(RecoverArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct AccountIdentityArgs {
	#[arg(long)]
	account_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct AccountProfileArgs {
	#[arg(long)]
	account_id: String,
	/// Include the bounded current credential email. Email is redacted by default.
	#[arg(long)]
	include_email: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct EnrollArgs {
	#[arg(long)]
	operation_id: String,
	#[arg(long)]
	account_id: String,
	#[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
	enabled: bool,
	#[arg(long)]
	idempotency_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct ImportArgs {
	#[arg(long)]
	operation_id: String,
	#[arg(long)]
	account_id: String,
	#[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
	enabled: bool,
	#[arg(long, value_name = "PATH")]
	source: PathBuf,
	#[arg(long)]
	idempotency_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct AdministrationArgs {
	#[arg(long)]
	account_id: String,
	#[arg(long)]
	expected_revision: u64,
	#[arg(long)]
	idempotency_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct OperationAccountArgs {
	#[arg(long)]
	operation_id: String,
	#[arg(long)]
	account_id: String,
	#[arg(long)]
	expected_revision: u64,
	#[arg(long)]
	idempotency_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct FixedSelectionArgs {
	#[arg(long)]
	account_id: String,
	#[arg(long)]
	expected_account_revision: u64,
	#[arg(long)]
	expected_revision: u64,
	#[arg(long)]
	idempotency_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct RoutingRevisionArgs {
	#[arg(long)]
	expected_revision: u64,
	#[arg(long)]
	idempotency_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct AccountOrderArgs {
	#[arg(long, value_delimiter = ',')]
	order: Vec<String>,
	#[arg(long)]
	expected_revision: u64,
	#[arg(long)]
	idempotency_key: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum RecoveryAction {
	Reconcile,
	CancelBeforeEffect,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct RecoverArgs {
	#[arg(long)]
	operation_id: String,
	#[arg(long, value_enum)]
	action: RecoveryAction,
	#[arg(long)]
	expected_revision: u64,
	#[arg(long)]
	idempotency_key: String,
}

#[derive(Serialize)]
struct OutputDocument<T> {
	schema: &'static str,
	command: &'static str,
	outcome: &'static str,
	result: T,
}

pub async fn execute(
	command: AccountCommand,
	format: OutputFormat,
	root: Option<&Path>,
	selected_profile: Option<&str>,
	expected_server_id: Option<&str>,
) -> CommandOutput {
	let profile = match load_client_profile(root, selected_profile, expected_server_id) {
		Ok(profile) => profile,
		Err(failure) => return failure_output(format, failure),
	};
	let client = AccountClient::new(profile);
	match command {
		AccountCommand::List => return render("list", format, client.list().await),
		AccountCommand::Inspect(args) => {
			let account_id = match entity(&args.account_id) {
				Ok(value) => value,
				Err(output) => return output,
			};
			return render("inspect", format, client.inspect(account_id).await);
		},
		AccountCommand::Profile(args) => {
			let account_id = match entity(&args.account_id) {
				Ok(value) => value,
				Err(output) => return output,
			};
			return render("profile", format, client.profile(account_id, args.include_email).await);
		},
		AccountCommand::CodexProjection =>
			return render("codex_projection", format, client.codex_auth_projection().await),
		AccountCommand::UseInCodex(args) => {
			let input = (
				entity(&args.account_id),
				revision(args.expected_revision),
				idempotency_key(args.idempotency_key),
			);
			let (Ok(account_id), Ok(account_revision), Ok(key)) = input else {
				return invalid_input();
			};
			return render_command(
				format,
				client.use_account_in_codex(account_id, account_revision, key).await,
			);
		},
		AccountCommand::SetFixedSelection(args) => {
			let input = (
				entity(&args.account_id),
				revision(args.expected_account_revision),
				revision(args.expected_revision),
				idempotency_key(args.idempotency_key),
			);
			let (Ok(account_id), Ok(account_revision), Ok(routing_revision), Ok(key)) = input
			else {
				return invalid_input();
			};
			return render_command(
				format,
				client
					.set_fixed_account_selection(
						account_id,
						account_revision,
						routing_revision,
						key,
					)
					.await,
			);
		},
		AccountCommand::SetBalancedSelection(args) => {
			let input = (revision(args.expected_revision), idempotency_key(args.idempotency_key));
			let (Ok(routing_revision), Ok(key)) = input else {
				return invalid_input();
			};
			return render_command(
				format,
				client.set_balanced_account_selection(routing_revision, key).await,
			);
		},
		AccountCommand::SetAccountOrder(args) => {
			let input = (
				account_order(&args.order),
				revision(args.expected_revision),
				idempotency_key(args.idempotency_key),
			);
			let (Ok(order), Ok(routing_revision), Ok(key)) = input else {
				return invalid_input();
			};
			return render_command(
				format,
				client.set_account_order(order, routing_revision, key).await,
			);
		},
		command => {
			let (payload, expected_revision, key) = match prepare_command(command) {
				Ok(value) => value,
				Err(output) => return output,
			};
			return render_command(format, client.execute(payload, expected_revision, key).await);
		},
	}
}

type PreparedCommand = (CommandPayload, Option<EntityRevision>, IdempotencyKey);

fn prepare_command(command: AccountCommand) -> Result<PreparedCommand, CommandOutput> {
	match command {
		AccountCommand::Enroll(args) => command_input(
			CommandPayload::EnrollAccountFromSharedCodex {
				operation_id: entity(&args.operation_id)?,
				account_id: entity(&args.account_id)?,
				enabled: args.enabled,
			},
			None,
			args.idempotency_key,
		),
		AccountCommand::Import(args) => command_input(
			CommandPayload::ImportAccountCredentialFile {
				operation_id: entity(&args.operation_id)?,
				account_id: entity(&args.account_id)?,
				enabled: args.enabled,
				source_descriptor: text(args.source.to_string_lossy().into_owned())?,
			},
			None,
			args.idempotency_key,
		),
		AccountCommand::Enable(args) => command_input(
			CommandPayload::SetAccountEnabled {
				account_id: entity(&args.account_id)?,
				enabled: true,
			},
			Some(EntityRevision(args.expected_revision)),
			args.idempotency_key,
		),
		AccountCommand::Disable(args) => command_input(
			CommandPayload::SetAccountEnabled {
				account_id: entity(&args.account_id)?,
				enabled: false,
			},
			Some(EntityRevision(args.expected_revision)),
			args.idempotency_key,
		),
		AccountCommand::Logout(args) => command_input(
			CommandPayload::LogoutAccount {
				operation_id: entity(&args.operation_id)?,
				account_id: entity(&args.account_id)?,
			},
			Some(EntityRevision(args.expected_revision)),
			args.idempotency_key,
		),
		AccountCommand::Refresh(args) => command_input(
			CommandPayload::RefreshAccount {
				operation_id: entity(&args.operation_id)?,
				account_id: entity(&args.account_id)?,
			},
			Some(EntityRevision(args.expected_revision)),
			args.idempotency_key,
		),
		AccountCommand::Recover(args) => command_input(
			CommandPayload::RecoverAccountOperation {
				operation_id: entity(&args.operation_id)?,
				action: match args.action {
					RecoveryAction::Reconcile =>
						AccountManualRecoveryActionDto::ReconcileExactStoreState,
					RecoveryAction::CancelBeforeEffect =>
						AccountManualRecoveryActionDto::CancelBeforeEffect,
				},
			},
			Some(EntityRevision(args.expected_revision)),
			args.idempotency_key,
		),
		AccountCommand::List
		| AccountCommand::Inspect(_)
		| AccountCommand::Profile(_)
		| AccountCommand::CodexProjection
		| AccountCommand::UseInCodex(_)
		| AccountCommand::SetFixedSelection(_)
		| AccountCommand::SetBalancedSelection(_)
		| AccountCommand::SetAccountOrder(_) => Err(invalid_input()),
	}
}

fn command_input(
	payload: CommandPayload,
	expected_revision: Option<EntityRevision>,
	idempotency_key: String,
) -> Result<(CommandPayload, Option<EntityRevision>, IdempotencyKey), CommandOutput> {
	let key = IdempotencyKey::new(idempotency_key).map_err(|_| invalid_input())?;
	Ok((payload, expected_revision, key))
}

fn entity(value: &str) -> Result<EntityId, CommandOutput> {
	if !crate::is_canonical_uuid(value) {
		return Err(invalid_input());
	}
	EntityId::new(value.to_owned()).map_err(|_| invalid_input())
}

fn revision(value: u64) -> Result<EntityRevision, CommandOutput> {
	if value == 0 { Err(invalid_input()) } else { Ok(EntityRevision(value)) }
}

fn idempotency_key(value: String) -> Result<IdempotencyKey, CommandOutput> {
	IdempotencyKey::new(value).map_err(|_| invalid_input())
}

fn account_order(values: &[String]) -> Result<Vec<EntityId>, CommandOutput> {
	if values.len() > 512 {
		return Err(invalid_input());
	}
	let order = values.iter().map(|value| entity(value)).collect::<Result<Vec<_>, _>>()?;
	let unique = order.iter().map(EntityId::as_str).collect::<std::collections::HashSet<_>>();
	if unique.len() != order.len() {
		return Err(invalid_input());
	}
	Ok(order)
}

fn text(value: String) -> Result<WireText, CommandOutput> {
	WireText::new(value).map_err(|_| invalid_input())
}

fn render<T: Serialize>(
	command: &'static str,
	format: OutputFormat,
	result: Result<T, decodex_protocol::ClientFailure>,
) -> CommandOutput {
	match result {
		Ok(result) => {
			let document = OutputDocument {
				schema: ACCOUNT_OUTPUT_SCHEMA,
				command,
				outcome: "success",
				result,
			};
			let text = match format {
				OutputFormat::Json => serde_json::to_string(&document),
				OutputFormat::Human => serde_json::to_string_pretty(&document),
			}
			.expect("typed account output serialization cannot fail");
			CommandOutput { text, exit_code: 0, error_stream: false }
		},
		Err(failure) => failure_output(format, failure),
	}
}

fn render_command(
	format: OutputFormat,
	result: Result<AccountCommandResponse, decodex_protocol::ClientFailure>,
) -> CommandOutput {
	let result = match result {
		Ok(result) => result,
		Err(failure) => return failure_output(format, failure),
	};
	let (outcome, exit_code) = match &result {
		AccountCommandResponse::Applied { .. } => ("applied", 0),
		AccountCommandResponse::Rejected { .. } => ("rejected", 1),
		AccountCommandResponse::PotentiallyDispatched { .. } => ("potentially_dispatched", 2),
	};
	let document =
		OutputDocument { schema: ACCOUNT_OUTPUT_SCHEMA, command: "command", outcome, result };
	let text = match format {
		OutputFormat::Json => serde_json::to_string(&document),
		OutputFormat::Human => serde_json::to_string_pretty(&document),
	}
	.expect("typed account command output serialization cannot fail");
	CommandOutput { text, exit_code, error_stream: false }
}

fn failure_output(format: OutputFormat, failure: decodex_protocol::ClientFailure) -> CommandOutput {
	let text = serde_json::to_string(&serde_json::json!({
		"schema": ACCOUNT_OUTPUT_SCHEMA,
		"outcome": "failure",
		"failure": failure,
	}))
	.expect("closed account client failure serialization cannot fail");
	CommandOutput { text, exit_code: 2, error_stream: matches!(format, OutputFormat::Human) }
}

fn invalid_input() -> CommandOutput {
	CommandOutput {
		text: "decodex account: invalid bounded account input".to_owned(),
		exit_code: 2,
		error_stream: true,
	}
}

#[cfg(test)]
mod tests {
	use clap::Parser as _;
	use decodex_protocol::{
		AccountProfileDailyUsageDto, AccountProfileDto, AccountProfileEmailDto,
		AccountProfileErrorDto, AccountProfileResult, EntityId, EntityRevision, WireText,
	};

	use crate::{Cli, Command};

	use super::{ACCOUNT_OUTPUT_SCHEMA, AccountCommand, OutputDocument};

	const OPERATION_ID: &str = "40000000-0000-4000-8000-000000000001";
	const ACCOUNT_ID: &str = "40000000-0000-4000-8000-000000000002";

	#[test]
	fn enroll_and_import_accept_explicit_false_without_changing_the_true_default() {
		let enroll = Cli::try_parse_from([
			"decodex",
			"account",
			"enroll",
			"--operation-id",
			OPERATION_ID,
			"--account-id",
			ACCOUNT_ID,
			"--enabled",
			"false",
			"--idempotency-key",
			"enroll-disabled",
		])
		.expect("explicit false must parse");
		let import = Cli::try_parse_from([
			"decodex",
			"account",
			"import",
			"--operation-id",
			OPERATION_ID,
			"--account-id",
			ACCOUNT_ID,
			"--source",
			"/private/input.json",
			"--idempotency-key",
			"import-default",
		])
		.expect("the true default must parse");

		assert!(matches!(
			enroll.command,
			Command::Account(AccountCommand::Enroll(args)) if !args.enabled
		));
		assert!(matches!(
			import.command,
			Command::Account(AccountCommand::Import(args)) if args.enabled
		));
	}

	#[test]
	fn projection_status_and_use_in_codex_are_distinct_from_routing() {
		let status = Cli::try_parse_from(["decodex", "account", "codex-projection"])
			.expect("projection status must parse");
		let use_in_codex = Cli::try_parse_from([
			"decodex",
			"account",
			"use-in-codex",
			"--account-id",
			ACCOUNT_ID,
			"--expected-revision",
			"7",
			"--idempotency-key",
			"use-in-codex",
		])
		.expect("explicit Codex projection must parse");

		assert!(matches!(status.command, Command::Account(AccountCommand::CodexProjection)));
		assert!(matches!(
			use_in_codex.command,
			Command::Account(AccountCommand::UseInCodex(args))
				if args.expected_revision == 7
		));
		assert!(
			Cli::try_parse_from(["decodex", "account", "rename", "--account-id", ACCOUNT_ID,])
				.is_err()
		);
	}

	#[test]
	fn routing_uses_three_explicit_subcommands_without_the_combined_alias() {
		let fixed = Cli::try_parse_from([
			"decodex",
			"account",
			"set-fixed-selection",
			"--account-id",
			ACCOUNT_ID,
			"--expected-account-revision",
			"4",
			"--expected-revision",
			"2",
			"--idempotency-key",
			"fixed-selection",
		])
		.expect("fixed selection must parse");
		let balanced = Cli::try_parse_from([
			"decodex",
			"account",
			"set-balanced-selection",
			"--expected-revision",
			"2",
			"--idempotency-key",
			"balanced-selection",
		])
		.expect("balanced selection must parse");
		let order = Cli::try_parse_from([
			"decodex",
			"account",
			"set-account-order",
			"--order",
			ACCOUNT_ID,
			"--expected-revision",
			"2",
			"--idempotency-key",
			"account-order",
		])
		.expect("account order must parse");

		assert!(matches!(
			fixed.command,
			Command::Account(AccountCommand::SetFixedSelection(args))
				if args.expected_account_revision == 4 && args.expected_revision == 2
		));
		assert!(matches!(
			balanced.command,
			Command::Account(AccountCommand::SetBalancedSelection(args))
				if args.expected_revision == 2
		));
		assert!(matches!(
			order.command,
			Command::Account(AccountCommand::SetAccountOrder(args))
				if args.order == vec![ACCOUNT_ID.to_owned()]
		));
		assert!(Cli::try_parse_from(["decodex", "account", "route"]).is_err());
	}

	#[test]
	fn profile_redacts_email_by_default_and_requires_explicit_inclusion() {
		let redacted =
			Cli::try_parse_from(["decodex", "account", "profile", "--account-id", ACCOUNT_ID])
				.expect("bounded profile query must parse");
		let visible = Cli::try_parse_from([
			"decodex",
			"account",
			"profile",
			"--account-id",
			ACCOUNT_ID,
			"--include-email",
		])
		.expect("explicit email inclusion must parse");

		assert!(matches!(
			redacted.command,
			Command::Account(AccountCommand::Profile(args)) if !args.include_email
		));
		assert!(matches!(
			visible.command,
			Command::Account(AccountCommand::Profile(args)) if args.include_email
		));
	}

	#[test]
	fn profile_json_has_stable_current_cached_and_unavailable_document_shapes() {
		let profile = AccountProfileDto {
			account_id: EntityId::new(ACCOUNT_ID).unwrap(),
			account_revision: EntityRevision(7),
			observed_at_unix_micros: 1_700_000_000_000_000,
			email: AccountProfileEmailDto::Redacted,
			plan_type: Some(WireText::new("pro").unwrap()),
			display_name: Some(WireText::new("Iris").unwrap()),
			username: Some(WireText::new("iris").unwrap()),
			lifetime_tokens: Some(12_345),
			peak_daily_tokens: Some(900),
			longest_task_seconds: Some(600),
			current_streak_days: Some(3),
			longest_streak_days: Some(8),
			daily_usage: vec![AccountProfileDailyUsageDto {
				start_date: WireText::new("2026-07-28").unwrap(),
				tokens: 900,
			}],
		};
		let document = OutputDocument {
			schema: ACCOUNT_OUTPUT_SCHEMA,
			command: "profile",
			outcome: "success",
			result: AccountProfileResult::Current(Box::new(profile.clone())),
		};

		assert_eq!(
			serde_json::to_value(document).unwrap(),
			serde_json::json!({
				"schema": "decodex/cli-account/1",
				"command": "profile",
				"outcome": "success",
				"result": {
					"outcome": "current",
					"data": {
						"account_id": ACCOUNT_ID,
						"account_revision": 7,
						"observed_at_unix_micros": 1_700_000_000_000_000_i64,
						"email": {"visibility": "redacted"},
						"plan_type": "pro",
						"display_name": "Iris",
						"username": "iris",
						"lifetime_tokens": 12_345,
						"peak_daily_tokens": 900,
						"longest_task_seconds": 600,
						"current_streak_days": 3,
						"longest_streak_days": 8,
						"daily_usage": [{"start_date": "2026-07-28", "tokens": 900}],
					},
				},
			}),
		);

		let cached = OutputDocument {
			schema: ACCOUNT_OUTPUT_SCHEMA,
			command: "profile",
			outcome: "success",
			result: AccountProfileResult::Cached {
				profile: Box::new(profile),
				refresh_error: AccountProfileErrorDto::ProviderUnavailable,
			},
		};
		assert_eq!(
			serde_json::to_value(cached).unwrap(),
			serde_json::json!({
				"schema": "decodex/cli-account/1",
				"command": "profile",
				"outcome": "success",
				"result": {
					"outcome": "cached",
					"data": {
						"profile": {
							"account_id": ACCOUNT_ID,
							"account_revision": 7,
							"observed_at_unix_micros": 1_700_000_000_000_000_i64,
							"email": {"visibility": "redacted"},
							"plan_type": "pro",
							"display_name": "Iris",
							"username": "iris",
							"lifetime_tokens": 12_345,
							"peak_daily_tokens": 900,
							"longest_task_seconds": 600,
							"current_streak_days": 3,
							"longest_streak_days": 8,
							"daily_usage": [{"start_date": "2026-07-28", "tokens": 900}],
						},
						"refresh_error": "provider_unavailable",
					},
				},
			}),
		);

		let unavailable = OutputDocument {
			schema: ACCOUNT_OUTPUT_SCHEMA,
			command: "profile",
			outcome: "success",
			result: AccountProfileResult::Unavailable {
				error: AccountProfileErrorDto::ProviderUnavailable,
				email: AccountProfileEmailDto::Redacted,
				plan_type: Some(WireText::new("pro").unwrap()),
			},
		};
		assert_eq!(
			serde_json::to_value(unavailable).unwrap(),
			serde_json::json!({
				"schema": "decodex/cli-account/1",
				"command": "profile",
				"outcome": "success",
				"result": {
					"outcome": "unavailable",
					"data": {
						"error": "provider_unavailable",
						"email": {"visibility": "redacted"},
						"plan_type": "pro",
					},
				},
			}),
		);
	}
}
