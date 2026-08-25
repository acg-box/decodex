//! Sole Decodex vNext server composition root.

#[cfg(unix)]
mod parent_lifetime;

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
	/// Print the exact local artifact/protocol cohort without starting the daemon.
	#[command(hide = true)]
	ArtifactCohort,
	/// Serve the same-UID Decodex vNext protocol.
	Serve {
		/// Inherited Unix socket whose EOF binds this daemon to one desktop-app lifetime.
		#[arg(long, hide = true)]
		parent_fd: Option<i32>,
	},
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
		None => serve(None).await,
		Some(Command::Serve { parent_fd }) => serve(parent_fd).await,
		Some(Command::ArtifactCohort) => {
			println!(
				"{}",
				serde_json::json!({
					"schema": "decodex/artifact-cohort/1",
					"artifact_cohort": decodex_protocol::CURRENT_ARTIFACT_COHORT,
					"protocol": decodex_protocol::CURRENT_VERSION,
				})
			);
			Ok(())
		},
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

async fn serve(parent_fd: Option<i32>) -> Result<(), Box<dyn Error>> {
	#[cfg(unix)]
	let mut parent_lifetime = parent_fd
		.map(parent_lifetime::ParentLifetime::from_inherited_fd)
		.transpose()?;
	#[cfg(not(unix))]
	if parent_fd.is_some() {
		return Err("parent lifetime channel is unsupported on this platform".into());
	}
	let bootstrap = ServiceComposition::bootstrap_default().await;
	let mut bound = bootstrap.bind(ServerConfig::default()).await?;
	let mut signals = ShutdownSignals::new()?;

	println!("decodexd serving WebSocket /v1/ws over same-UID local transport");

	#[cfg(unix)]
	if let Some(parent_lifetime) = parent_lifetime.as_mut() {
		tokio::select! {
			result = bound.wait() => {
				result?;
			},
			signal = signals.recv() => {
				signal?;
				bound.shutdown().await?;
			},
			parent = parent_lifetime.wait_for_parent_exit() => {
				parent?;
				bound.shutdown().await?;
			},
		}
	} else {
		wait_for_shutdown(&mut bound, &mut signals).await?;
	}
	#[cfg(not(unix))]
	wait_for_shutdown(&mut bound, &mut signals).await?;

	Ok(())
}

async fn wait_for_shutdown(
	bound: &mut decodex_runtime::BoundServer,
	signals: &mut ShutdownSignals,
) -> Result<(), Box<dyn Error>> {
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
		assert!(matches!(explicit.command, Some(Command::Serve { parent_fd: None })));
		let parent = Cli::try_parse_from(["decodexd", "serve", "--parent-fd", "9"])
			.expect("parse bundled parent lifetime");
		assert!(matches!(parent.command, Some(Command::Serve { parent_fd: Some(9) })));

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
