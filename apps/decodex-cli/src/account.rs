//! Operator account client over the same-UID V1.4 daemon protocol.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use decodex_protocol::{
	AccountClient, AccountCommandResponse, AccountManualRecoveryActionDto, AccountSelectionModeDto,
	CommandPayload, EntityId, EntityRevision, IdempotencyKey, WireText,
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
	/// Enroll credentials from the normal shared Codex auth file.
	Enroll(EnrollArgs),
	/// Import one owner-private versioned credential file.
	Import(ImportArgs),
	/// Rename one account.
	Rename(RenameArgs),
	/// Enable new work admission for one account.
	Enable(AdministrationArgs),
	/// Disable new work admission for one account.
	Disable(AdministrationArgs),
	/// Log out and tombstone one account.
	Logout(OperationAccountArgs),
	/// Replace fixed/balanced selection and complete user order.
	Route(RouteArgs),
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
pub struct EnrollArgs {
	#[arg(long)]
	operation_id: String,
	#[arg(long)]
	account_id: String,
	#[arg(long)]
	label: String,
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
	#[arg(long)]
	label: String,
	#[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
	enabled: bool,
	#[arg(long, value_name = "PATH")]
	source: PathBuf,
	#[arg(long)]
	idempotency_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct RenameArgs {
	#[arg(long)]
	account_id: String,
	#[arg(long)]
	label: String,
	#[arg(long)]
	expected_revision: u64,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum RouteMode {
	Fixed,
	Balanced,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct RouteArgs {
	#[arg(long, value_enum)]
	mode: RouteMode,
	#[arg(long)]
	fixed_account_id: Option<String>,
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
				display_label: text(args.label)?,
				enabled: args.enabled,
			},
			None,
			args.idempotency_key,
		),
		AccountCommand::Import(args) => command_input(
			CommandPayload::ImportAccountCredentialFile {
				operation_id: entity(&args.operation_id)?,
				account_id: entity(&args.account_id)?,
				display_label: text(args.label)?,
				enabled: args.enabled,
				source_descriptor: text(args.source.to_string_lossy().into_owned())?,
			},
			None,
			args.idempotency_key,
		),
		AccountCommand::Rename(args) => command_input(
			CommandPayload::RenameAccount {
				account_id: entity(&args.account_id)?,
				display_label: text(args.label)?,
			},
			Some(EntityRevision(args.expected_revision)),
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
		AccountCommand::Route(args) => {
			let mode = match (args.mode, args.fixed_account_id) {
				(RouteMode::Fixed, Some(account_id)) =>
					AccountSelectionModeDto::Fixed(entity(&account_id)?),
				(RouteMode::Balanced, None) => AccountSelectionModeDto::Balanced,
				_ => return Err(invalid_input()),
			};
			let order =
				args.order.iter().map(|value| entity(value)).collect::<Result<Vec<_>, _>>()?;
			command_input(
				CommandPayload::ConfigureAccountRouting {
					expected_routing_revision: EntityRevision(args.expected_revision),
					mode,
					order,
				},
				Some(EntityRevision(args.expected_revision)),
				args.idempotency_key,
			)
		},
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
		AccountCommand::List | AccountCommand::Inspect(_) => Err(invalid_input()),
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

	use crate::{Cli, Command};

	use super::AccountCommand;

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
			"--label",
			"disabled enrollment",
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
			"--label",
			"default enrollment",
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
}
