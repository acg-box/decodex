//! Sole Decodex vNext server composition root.

use std::error::Error;

use decodex_runtime::{ServerConfig, ServiceComposition};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let bootstrap = ServiceComposition::bootstrap_default().await;
	let mut bound = bootstrap.bind(ServerConfig::default()).await?;

	println!("decodexd serving WebSocket /v1/ws over same-UID local transport");

	bound.wait().await?;

	Ok(())
}
