//! Sole Decodex vNext server composition root.

mod service_supervisor;

use std::{error::Error, path::PathBuf};

use clap::{Parser, Subcommand};
#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
use clap::ValueEnum;
use decodex_runtime::{ServerConfig, ServiceComposition};
#[cfg(test)] use {libc as _, tempfile as _};

#[derive(Parser)]
#[command(about, version)]
struct Cli {
	#[command(subcommand)]
	command: Option<Command>,
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
#[derive(Clone, Copy, ValueEnum)]
enum AccountMigrationCredentialGateAction {
	Readback,
	ProveCreateConflict,
	CleanupRun,
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
#[derive(Clone, Copy, ValueEnum)]
enum AccountMigrationCredentialGateSlot {
	#[value(name = "account-1")]
	Account1,
	#[value(name = "account-2")]
	Account2,
	#[value(name = "account-3")]
	Account3,
	#[value(name = "account-4")]
	Account4,
	Conflict,
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
#[derive(Clone, Copy, ValueEnum)]
enum AccountMigrationAdmissionGateBoundary {
	Unsettled,
	Completed,
}

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
#[derive(Clone, Copy, ValueEnum)]
enum AccountMigrationRecoveryGatePhase {
	Prepared,
	RecoveryRequired,
}

#[derive(Subcommand)]
enum Command {
	/// Serve the same-UID Decodex vNext protocol.
	Serve,
	/// Supervise the ordinary local PostgreSQL and decodexd service generation.
	SuperviseLocal {
		#[arg(long)]
		postgres: PathBuf,
		#[arg(long)]
		pg_isready: PathBuf,
		#[arg(long)]
		data_directory: PathBuf,
		#[arg(long)]
		socket_directory: PathBuf,
		#[arg(long)]
		port: u16,
		#[arg(long)]
		working_directory: PathBuf,
	},
	/// Execute the installer-owned one-shot account cutover while the service is offline.
	#[cfg(target_os = "macos")]
	#[command(hide = true)]
	MigrateAccounts {
		#[arg(long, hide = true)]
		installer_lock_fd: std::os::fd::RawFd,
		#[cfg(feature = "account-migration-transition-gate")]
		#[arg(long, hide = true)]
		transition_gate_fd: Option<std::os::fd::RawFd>,
		#[arg(long)]
		config: PathBuf,
		#[arg(long)]
		manifest: PathBuf,
		#[arg(long)]
		credential_directory: PathBuf,
		#[arg(long)]
		launch_agent: PathBuf,
	},
	/// Finalize the offline receipt after exact config swap and staging retirement.
	#[cfg(target_os = "macos")]
	#[command(hide = true)]
	FinalizeAccountMigration {
		#[arg(long, hide = true)]
		installer_lock_fd: std::os::fd::RawFd,
		#[cfg(feature = "account-migration-transition-gate")]
		#[arg(long, hide = true)]
		transition_gate_fd: Option<std::os::fd::RawFd>,
		#[arg(long)]
		config: PathBuf,
		#[arg(long)]
		manifest: PathBuf,
		#[arg(long)]
		launch_agent: PathBuf,
		#[arg(long)]
		retired_staging_config: PathBuf,
		#[arg(long)]
		retired_credential_directory: PathBuf,
		#[arg(long = "retired-active-source")]
		retired_active_sources: Vec<PathBuf>,
		#[arg(long = "installed-asset")]
		installed_assets: Vec<PathBuf>,
	},
	/// Verify a prepared destination before the installer continues retirement.
	#[cfg(target_os = "macos")]
	#[command(hide = true)]
	VerifyPreparedAccountMigration {
		#[arg(long, hide = true)]
		installer_lock_fd: std::os::fd::RawFd,
		#[cfg(feature = "account-migration-transition-gate")]
		#[arg(long, hide = true)]
		transition_gate_fd: Option<std::os::fd::RawFd>,
		#[arg(long)]
		config: PathBuf,
		#[arg(long)]
		manifest: PathBuf,
		#[arg(long)]
		launch_agent: PathBuf,
	},
	/// Verify a completed cutover without reopening retired legacy sources.
	#[cfg(target_os = "macos")]
	#[command(hide = true)]
	VerifyAccountMigration {
		#[arg(long, hide = true)]
		installer_lock_fd: std::os::fd::RawFd,
		#[cfg(feature = "account-migration-transition-gate")]
		#[arg(long, hide = true)]
		transition_gate_fd: Option<std::os::fd::RawFd>,
		#[arg(long)]
		config: PathBuf,
		#[arg(long)]
		launch_agent: PathBuf,
		#[arg(long)]
		retired_staging_config: PathBuf,
		#[arg(long)]
		retired_credential_directory: PathBuf,
		#[arg(long = "retired-active-source")]
		retired_active_sources: Vec<PathBuf>,
		#[arg(long = "installed-asset")]
		installed_assets: Vec<PathBuf>,
	},
	/// Exercise the finite run-owned protected-store fixture for the canonical transition gate.
	#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
	#[command(hide = true)]
	AccountMigrationCredentialGate {
		#[arg(value_enum)]
		action: AccountMigrationCredentialGateAction,
		#[arg(long)]
		run_descriptor: PathBuf,
		#[arg(long, value_enum)]
		slot: Option<AccountMigrationCredentialGateSlot>,
	},
	/// Exercise real runtime admission owners at a canonical migration gate boundary.
	#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
	#[command(hide = true)]
	AccountMigrationAdmissionGate {
		#[arg(value_enum)]
		boundary: AccountMigrationAdmissionGateBoundary,
		#[arg(long)]
		config: PathBuf,
		#[arg(long)]
		account_id: String,
		#[arg(long)]
		expected_revision: i64,
	},
	/// Exercise both cancellation owners for one finite manifest-bound operation.
	#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
	#[command(hide = true)]
	AccountMigrationRecoveryGate {
		#[arg(value_enum)]
		phase: AccountMigrationRecoveryGatePhase,
		#[arg(long)]
		run_descriptor: PathBuf,
	},
	/// Retain the real daemon-side local transport namespace for a gate barrier.
	#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
	#[command(hide = true)]
	AccountMigrationLiveDaemonGate {
		#[arg(long)]
		root: PathBuf,
		#[arg(long, hide = true)]
		barrier_fd: std::os::fd::RawFd,
	},
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	match Cli::parse().command {
		None | Some(Command::Serve) => serve().await,
		Some(Command::SuperviseLocal {
			postgres,
			pg_isready,
			data_directory,
			socket_directory,
			port,
			working_directory,
		}) => service_supervisor::supervise(service_supervisor::ServiceSupervisorConfig {
			postgres,
			pg_isready,
			data_directory,
			socket_directory,
			port,
			working_directory,
		})
		.await
		.map_err(Into::into),
		#[cfg(target_os = "macos")]
		Some(Command::MigrateAccounts {
			installer_lock_fd,
			#[cfg(feature = "account-migration-transition-gate")]
			transition_gate_fd,
			config,
			manifest,
			credential_directory,
			launch_agent,
		}) => {
			let report = decodex_runtime::run_offline_account_migration(
				decodex_runtime::OfflineAccountMigrationOptions {
					installer_lock_fd,
					#[cfg(feature = "account-migration-transition-gate")]
					transition_gate_fd,
					config,
					manifest,
					credential_directory,
					launch_agent,
				},
			)
			.await?;
			println!("{}", serde_json::to_string(&report)?);
			Ok(())
		},
		#[cfg(target_os = "macos")]
		Some(Command::FinalizeAccountMigration {
			installer_lock_fd,
			#[cfg(feature = "account-migration-transition-gate")]
			transition_gate_fd,
			config,
			manifest,
			launch_agent,
			retired_staging_config,
			retired_credential_directory,
			retired_active_sources,
			installed_assets,
		}) => {
			let report = decodex_runtime::finalize_offline_account_migration(
				decodex_runtime::OfflineAccountMigrationFinalizeOptions {
					installer_lock_fd,
					#[cfg(feature = "account-migration-transition-gate")]
					transition_gate_fd,
					config,
					manifest,
					launch_agent,
					retired_staging_config,
					retired_credential_directory,
					retired_active_sources,
					installed_assets,
				},
			)
			.await?;
			println!("{}", serde_json::to_string(&report)?);
			Ok(())
		},
		#[cfg(target_os = "macos")]
		Some(Command::VerifyPreparedAccountMigration {
			installer_lock_fd,
			#[cfg(feature = "account-migration-transition-gate")]
			transition_gate_fd,
			config,
			manifest,
			launch_agent,
		}) => {
			let report = decodex_runtime::verify_prepared_offline_account_migration_destination(
				decodex_runtime::OfflineAccountMigrationDestinationVerifyOptions {
					installer_lock_fd,
					#[cfg(feature = "account-migration-transition-gate")]
					transition_gate_fd,
					config,
					manifest,
					launch_agent,
				},
			)
			.await?;
			println!("{}", serde_json::to_string(&report)?);
			Ok(())
		},
		#[cfg(target_os = "macos")]
		Some(Command::VerifyAccountMigration {
			installer_lock_fd,
			#[cfg(feature = "account-migration-transition-gate")]
			transition_gate_fd,
			config,
			launch_agent,
			retired_staging_config,
			retired_credential_directory,
			retired_active_sources,
			installed_assets,
		}) => {
			let report = decodex_runtime::verify_completed_offline_account_migration(
				decodex_runtime::OfflineAccountMigrationVerifyOptions {
					installer_lock_fd,
					#[cfg(feature = "account-migration-transition-gate")]
					transition_gate_fd,
					config,
					launch_agent,
					retired_staging_config,
					retired_credential_directory,
					retired_active_sources,
					installed_assets,
				},
			)
			.await?;
			println!("{}", serde_json::to_string(&report)?);
			Ok(())
		},
		#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
		Some(Command::AccountMigrationCredentialGate {
			action,
			run_descriptor,
			slot,
		}) => {
			let action = match action {
				AccountMigrationCredentialGateAction::Readback => "readback",
				AccountMigrationCredentialGateAction::ProveCreateConflict =>
					"prove_create_conflict",
				AccountMigrationCredentialGateAction::CleanupRun => "cleanup_run",
			};
			let slot = slot.map(|slot| match slot {
				AccountMigrationCredentialGateSlot::Account1 => "account_1",
				AccountMigrationCredentialGateSlot::Account2 => "account_2",
				AccountMigrationCredentialGateSlot::Account3 => "account_3",
				AccountMigrationCredentialGateSlot::Account4 => "account_4",
				AccountMigrationCredentialGateSlot::Conflict => "conflict",
			});
			let report = decodex_runtime::run_account_migration_credential_gate(
				&run_descriptor,
				action,
				slot,
			)?;
			println!("{}", serde_json::to_string(&report)?);
			Ok(())
		},
		#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
		Some(Command::AccountMigrationAdmissionGate {
			boundary,
			config,
			account_id,
			expected_revision,
		}) => {
			let boundary = match boundary {
				AccountMigrationAdmissionGateBoundary::Unsettled => "unsettled",
				AccountMigrationAdmissionGateBoundary::Completed => "completed",
			};
			let report = decodex_runtime::exercise_account_migration_admission_for_gate(
				&config,
				&account_id,
				expected_revision,
				boundary,
			)
			.await?;
			println!("{}", serde_json::to_string(&report)?);
			Ok(())
		},
		#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
		Some(Command::AccountMigrationRecoveryGate {
			phase,
			run_descriptor,
		}) => {
			let phase = match phase {
				AccountMigrationRecoveryGatePhase::Prepared => "prepared",
				AccountMigrationRecoveryGatePhase::RecoveryRequired => "recovery_required",
			};
			let report = decodex_runtime::exercise_account_migration_recovery_for_gate(
				&run_descriptor,
				phase,
			)
			.await?;
			println!("{}", serde_json::to_string(&report)?);
			Ok(())
		},
		#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
		Some(Command::AccountMigrationLiveDaemonGate { root, barrier_fd }) => {
			let report = decodex_runtime::hold_account_migration_live_daemon_for_gate(
				&root,
				barrier_fd,
			)
			.await?;
			println!("{}", serde_json::to_string(&report)?);
			Ok(())
		},
	}
}

async fn serve() -> Result<(), Box<dyn Error>> {
	let bootstrap = ServiceComposition::bootstrap_default().await;
	let mut bound = bootstrap.bind(ServerConfig::default()).await?;
	let mut signals = ShutdownSignals::new()?;

	println!("decodexd serving WebSocket /v1/ws over same-UID local transport");

	tokio::select! {
		result = bound.wait() => {
			result?;
		},
		signal = signals.recv() => {
			signal?;
			bound.shutdown().await?;
		},
	}

	Ok(())
}

#[cfg(unix)]
struct ShutdownSignals {
	interrupt: tokio::signal::unix::Signal,
	terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl ShutdownSignals {
	fn new() -> std::io::Result<Self> {
		use tokio::signal::unix::{SignalKind, signal};

		Ok(Self {
			interrupt: signal(SignalKind::interrupt())?,
			terminate: signal(SignalKind::terminate())?,
		})
	}

	async fn recv(&mut self) -> std::io::Result<()> {
		tokio::select! {
			_ = self.interrupt.recv() => {},
			_ = self.terminate.recv() => {},
		}

		Ok(())
	}
}

#[cfg(not(unix))]
struct ShutdownSignals;

#[cfg(not(unix))]
impl ShutdownSignals {
	fn new() -> std::io::Result<Self> {
		Ok(Self)
	}

	async fn recv(&mut self) -> std::io::Result<()> {
		tokio::signal::ctrl_c().await
	}
}

#[cfg(test)]
mod tests {
	use clap::{CommandFactory as _, Parser as _};

	use super::{Cli, Command};

	#[test]
	fn command_surface_keeps_no_argument_serve_and_explicit_supervisor() {
		let default = Cli::try_parse_from(["decodexd"]).expect("parse default serve");
		assert!(default.command.is_none());

		let explicit = Cli::try_parse_from(["decodexd", "serve"]).expect("parse explicit serve");
		assert!(matches!(explicit.command, Some(Command::Serve)));

		Cli::command().debug_assert();
	}
}
