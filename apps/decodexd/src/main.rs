//! Sole Decodex vNext server composition root.

use std::error::Error;

use decodex_runtime::{ServerConfig, ServiceComposition};
#[cfg(test)] use {libc as _, tempfile as _};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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
