//! Sole Decodex vNext server composition root.

mod service_supervisor;

use std::{error::Error, path::PathBuf};

use clap::{Parser, Subcommand};
use decodex_runtime::{
	DecodexRoot, LocalAccountAuthorityRestoreReport, ServerConfig, ServiceComposition,
};
#[cfg(test)] use {libc as _, tempfile as _};

#[derive(Parser)]
#[command(about, version)]
struct Cli {
	#[command(subcommand)]
	command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
	/// Serve the same-UID Decodex vNext protocol.
	Serve,
	/// Create the latest PostgreSQL schema once on an empty target.
	#[command(hide = true)]
	BootstrapLatestSchema {
		#[arg(long)]
		root: PathBuf,
		#[arg(long)]
		schema_owner_user: String,
		#[arg(long)]
		schema_owner_credential_env_var: Option<String>,
	},
	/// Restore credential-negative local account authority into a fresh latest schema.
	#[command(hide = true)]
	RestoreLocalAccountAuthority {
		#[arg(long)]
		root: PathBuf,
		#[arg(long)]
		schema_owner_user: String,
		#[arg(long)]
		schema_owner_credential_env_var: Option<String>,
	},
	/// Verify the latest catalog and authority through the runtime identity only.
	#[command(hide = true)]
	ValidateCurrentAuthority {
		#[arg(long)]
		root: PathBuf,
	},
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	match Cli::parse().command {
		None | Some(Command::Serve) => serve().await,
		Some(Command::BootstrapLatestSchema {
			root,
			schema_owner_user,
			schema_owner_credential_env_var,
		}) => {
			let result = ServiceComposition::bootstrap_latest_schema(
				DecodexRoot::new(root)?,
				schema_owner_user,
				schema_owner_credential_env_var,
			)
			.await;
			match result {
				Ok(()) => Ok(()),
				Err(error) => {
					if let Some(report) = error.bootstrap_report_line() {
						eprintln!("{report}");
					}
					Err(Box::<dyn Error>::from(error))
				},
			}
		},
		Some(Command::RestoreLocalAccountAuthority {
			root,
			schema_owner_user,
			schema_owner_credential_env_var,
		}) => {
			let report = {
				#[cfg(target_os = "macos")]
				{
					match DecodexRoot::new(root) {
						Ok(root) =>
							ServiceComposition::restore_local_account_authority(
								root,
								schema_owner_user,
								schema_owner_credential_env_var,
								std::io::stdin().lock(),
							)
							.await,
						Err(_) => LocalAccountAuthorityRestoreReport::configuration_refused(),
					}
				}
				#[cfg(not(target_os = "macos"))]
				{
					drop((root, schema_owner_user, schema_owner_credential_env_var));
					LocalAccountAuthorityRestoreReport::host_refused()
				}
			};
			{
				use std::io::Write as _;

				let stdout = std::io::stdout();
				let mut output = stdout.lock();
				let _ = writeln!(output, "{report}");
				let _ = output.flush();
			}
			if report.succeeded() { Ok(()) } else { std::process::exit(1) }
		},
		Some(Command::ValidateCurrentAuthority { root }) =>
			ServiceComposition::validate_current_authority(DecodexRoot::new(root)?)
				.await
				.map_err(Into::into),
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

		let bootstrap = Cli::try_parse_from([
			"decodexd",
			"bootstrap-latest-schema",
			"--root",
			"/private/tmp/decodex-root",
			"--schema-owner-user",
			"decodex_owner",
			"--schema-owner-credential-env-var",
			"DECODEX_SCHEMA_OWNER_PASSWORD",
		])
		.expect("parse latest-schema bootstrap");
		assert!(matches!(
			bootstrap.command,
			Some(Command::BootstrapLatestSchema {
				root,
				schema_owner_user,
				schema_owner_credential_env_var,
			}) if root.as_path() == std::path::Path::new("/private/tmp/decodex-root")
				&& schema_owner_user == "decodex_owner"
				&& schema_owner_credential_env_var.as_deref()
					== Some("DECODEX_SCHEMA_OWNER_PASSWORD")
		));

		let restore = Cli::try_parse_from([
			"decodexd",
			"restore-local-account-authority",
			"--root",
			"/private/tmp/decodex-root",
			"--schema-owner-user",
			"decodex_owner",
			"--schema-owner-credential-env-var",
			"DECODEX_SCHEMA_OWNER_PASSWORD",
		])
		.expect("parse local account authority restore");
		assert!(matches!(
			restore.command,
			Some(Command::RestoreLocalAccountAuthority {
				root,
				schema_owner_user,
				schema_owner_credential_env_var,
			}) if root.as_path() == std::path::Path::new("/private/tmp/decodex-root")
				&& schema_owner_user == "decodex_owner"
				&& schema_owner_credential_env_var.as_deref()
					== Some("DECODEX_SCHEMA_OWNER_PASSWORD")
		));

		let validate = Cli::try_parse_from([
			"decodexd",
			"validate-current-authority",
			"--root",
			"/private/tmp/decodex-root",
		])
		.expect("parse current-authority validation");
		assert!(matches!(
			validate.command,
			Some(Command::ValidateCurrentAuthority { root })
				if root.as_path() == std::path::Path::new("/private/tmp/decodex-root")
		));

		Cli::command().debug_assert();
	}
}
