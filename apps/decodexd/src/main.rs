//! Sole Decodex vNext server composition root.

#[cfg(unix)] mod local_supervisor;

use std::error::Error;
#[cfg(unix)] use std::path::PathBuf;

#[cfg(unix)] use clap::Args;
use clap::{Parser, Subcommand};
use decodex_runtime::{ServerConfig, ServiceComposition};
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
	/// Supervise the local PostgreSQL and credential-injected daemon processes.
	#[cfg(unix)]
	SuperviseLocal(SuperviseLocalArgs),
}

#[cfg(unix)]
#[derive(Args)]
struct SuperviseLocalArgs {
	/// PostgreSQL 18 foreground server executable.
	#[arg(long)]
	postgres: PathBuf,
	/// PostgreSQL 18 readiness probe executable.
	#[arg(long)]
	pg_isready: PathBuf,
	/// Initialized PostgreSQL 18 data directory.
	#[arg(long)]
	data_directory: PathBuf,
	/// Private PostgreSQL Unix socket directory.
	#[arg(long)]
	socket_directory: PathBuf,
	/// PostgreSQL Unix socket port.
	#[arg(long)]
	port: u16,
	/// Legacy account-pool JSONL file read under its writer lock.
	#[arg(long)]
	legacy_accounts: PathBuf,
	/// Non-secret legacy-to-vNext slot mapping manifest.
	#[arg(long)]
	legacy_mapping: PathBuf,
	/// Working directory inherited by supervised children.
	#[arg(long)]
	working_directory: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	match Cli::parse().command {
		None | Some(Command::Serve) => serve().await,
		#[cfg(unix)]
		Some(Command::SuperviseLocal(arguments)) => {
			local_supervisor::supervise(local_supervisor::LocalSupervisorConfig {
				postgres: arguments.postgres,
				pg_isready: arguments.pg_isready,
				data_directory: arguments.data_directory,
				socket_directory: arguments.socket_directory,
				port: arguments.port,
				legacy_accounts: arguments.legacy_accounts,
				legacy_mapping: arguments.legacy_mapping,
				working_directory: arguments.working_directory,
			})
			.await?;

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
