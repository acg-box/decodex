//! Sole Decodex service composition.

#[cfg(unix)] mod parent_lifetime;

use std::{error::Error, path::Path};

use decodex_runtime::{DecodexRoot, ServerConfig, ServiceComposition};

use crate::CommandOutput;

pub(crate) async fn initialize_local_database(root: Option<&Path>) -> CommandOutput {
	run_database_command(root, ServiceComposition::initialize_local_database).await
}

pub(crate) async fn validate_local_database(root: Option<&Path>) -> CommandOutput {
	run_database_command(root, ServiceComposition::validate_local_database).await
}

async fn run_database_command<F, Fut>(root: Option<&Path>, command: F) -> CommandOutput
where
	F: FnOnce(DecodexRoot) -> Fut,
	Fut: Future<Output = Result<(), decodex_runtime::LocalDatabaseError>>,
{
	let result = async {
		let root = root.ok_or("--root is required for this command")?;
		let root = DecodexRoot::new(root.to_path_buf())?;
		command(root).await?;
		Ok::<(), Box<dyn Error>>(())
	}
	.await;
	command_result(result)
}

pub(crate) async fn serve(parent_fd: Option<i32>) -> CommandOutput {
	command_result(serve_inner(parent_fd).await)
}

async fn serve_inner(parent_fd: Option<i32>) -> Result<(), Box<dyn Error>> {
	#[cfg(unix)]
	let mut parent_lifetime =
		parent_fd.map(parent_lifetime::ParentLifetime::from_inherited_fd).transpose()?;
	#[cfg(not(unix))]
	if parent_fd.is_some() {
		return Err("parent lifetime channel is unsupported on this platform".into());
	}
	let bootstrap = ServiceComposition::bootstrap_default().await;
	let mut bound = bootstrap.bind(ServerConfig::default()).await?;
	let mut signals = ShutdownSignals::new()?;

	println!("decodex serving WebSocket /v1/ws over same-UID local transport");

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

fn command_result(result: Result<(), Box<dyn Error>>) -> CommandOutput {
	match result {
		Ok(()) => CommandOutput { text: String::new(), exit_code: 0, error_stream: false },
		Err(error) => CommandOutput {
			text: format!("decodex failed: {error}"),
			exit_code: 2,
			error_stream: true,
		},
	}
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

	use crate::{Cli, Command};

	#[test]
	fn command_surface_requires_explicit_serve() {
		assert!(Cli::try_parse_from(["decodex"]).is_err());

		let explicit = Cli::try_parse_from(["decodex", "serve"]).expect("parse explicit serve");
		assert!(matches!(explicit.command, Command::Serve { parent_fd: None }));
		let parent = Cli::try_parse_from(["decodex", "serve", "--parent-fd", "9"])
			.expect("parse bundled parent lifetime");
		assert!(matches!(parent.command, Command::Serve { parent_fd: Some(9) }));

		let initialize = Cli::try_parse_from([
			"decodex",
			"initialize-local-database",
			"--root",
			"/private/tmp/decodex-root",
		])
		.expect("parse local database initialization");
		assert!(matches!(initialize.command, Command::InitializeLocalDatabase));
		assert_eq!(
			initialize.root.as_deref(),
			Some(std::path::Path::new("/private/tmp/decodex-root"))
		);

		let validate = Cli::try_parse_from([
			"decodex",
			"validate-local-database",
			"--root",
			"/private/tmp/decodex-root",
		])
		.expect("parse current-authority validation");
		assert!(matches!(validate.command, Command::ValidateLocalDatabase));

		Cli::command().debug_assert();
	}
}
