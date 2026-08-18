//! Account CLI command definitions.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::{
	accounts::{self, AccountImportRequest, AccountUseRequest},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(super) struct AccountCommand {
	#[command(subcommand)]
	pub(super) command: AccountSubcommand,
}
impl AccountCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			AccountSubcommand::List(args) => accounts::run_account_list(args.json),
			AccountSubcommand::Select(args) =>
				accounts::run_account_select(&args.selector, args.json),
			AccountSubcommand::Clear(args) => accounts::run_account_clear(args.json),
			AccountSubcommand::Logout(args) =>
				accounts::run_account_logout(&args.selector, args.json),
			AccountSubcommand::ImportAuth(args) =>
				accounts::run_account_import(&AccountImportRequest {
					auth_json_path: args.auth_json.clone(),
					json: args.json,
				}),
			AccountSubcommand::Use(args) => accounts::run_account_use(&AccountUseRequest {
				selector: args.selector.clone(),
				auth_json_path: args.auth_json.clone(),
				json: args.json,
			}),
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct AccountListCommand {
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct AccountSelectCommand {
	/// Email, full account id, or redacted fingerprint to pin.
	pub(super) selector: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct AccountClearCommand {
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct AccountLogoutCommand {
	/// Email, full account id, or redacted fingerprint to remove.
	pub(super) selector: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct AccountImportCommand {
	/// Path to a Codex `auth.json` file to import.
	#[arg(value_name = "AUTH_JSON")]
	pub(super) auth_json: PathBuf,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct AccountUseCommand {
	/// Email, full account id, or redacted fingerprint to write into Codex `auth.json`.
	pub(super) selector: String,
	/// Override the Codex `auth.json` destination. Defaults to `$CODEX_HOME/auth.json`
	/// or `~/.codex/auth.json`.
	#[arg(long, value_name = "AUTH_JSON")]
	pub(super) auth_json: Option<PathBuf>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Subcommand)]
pub(super) enum AccountSubcommand {
	/// List configured Codex accounts without printing token material.
	List(AccountListCommand),
	/// Pin new Decodex runs to one account.
	Select(AccountSelectCommand),
	/// Return new Decodex runs to balanced account selection.
	Clear(AccountClearCommand),
	/// Remove one account from the Decodex account pool.
	Logout(AccountLogoutCommand),
	/// Import an existing Codex `auth.json` into the Decodex account pool.
	ImportAuth(AccountImportCommand),
	/// Force Codex to use one stored account by overwriting its `auth.json`.
	Use(AccountUseCommand),
}
