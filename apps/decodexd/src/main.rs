//! Sole Decodex vNext server composition root.

use std::{error::Error, path::PathBuf};

use clap::{Parser, Subcommand};
use decodex_runtime::{DecodexRoot, ServerConfig, ServiceComposition};
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
	/// Initialize or upgrade the bundled SQLite product database.
	#[command(hide = true)]
	InitializeLocalDatabase {
		#[arg(long)]
		root: PathBuf,
	},
	/// Verify the bundled SQLite database and migration ledger.
	#[command(hide = true)]
	ValidateLocalDatabase {
		#[arg(long)]
		root: PathBuf,
	},
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	match Cli::parse().command {
		None | Some(Command::Serve) => serve().await,
		Some(Command::InitializeLocalDatabase { root }) =>
			ServiceComposition::initialize_local_database(DecodexRoot::new(root)?)
				.await
				.map_err(Into::into),
		Some(Command::ValidateLocalDatabase { root }) =>
			ServiceComposition::validate_local_database(DecodexRoot::new(root)?)
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
	fn command_surface_keeps_no_argument_and_explicit_serve() {
		let default = Cli::try_parse_from(["decodexd"]).expect("parse default serve");
		assert!(default.command.is_none());

		let explicit = Cli::try_parse_from(["decodexd", "serve"]).expect("parse explicit serve");
		assert!(matches!(explicit.command, Some(Command::Serve)));

		let initialize = Cli::try_parse_from([
			"decodexd",
			"initialize-local-database",
			"--root",
			"/private/tmp/decodex-root",
		])
		.expect("parse local database initialization");
		assert!(matches!(
			initialize.command,
			Some(Command::InitializeLocalDatabase { root })
				if root.as_path() == std::path::Path::new("/private/tmp/decodex-root")
		));

		let validate = Cli::try_parse_from([
			"decodexd",
			"validate-local-database",
			"--root",
			"/private/tmp/decodex-root",
		])
		.expect("parse current-authority validation");
		assert!(matches!(
			validate.command,
			Some(Command::ValidateLocalDatabase { root })
				if root.as_path() == std::path::Path::new("/private/tmp/decodex-root")
		));

		Cli::command().debug_assert();
	}
}
