//! Sole Decodex vNext server composition root.

use std::error::Error;

use decodex_runtime::{ServerConfig, ServiceComposition};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let bootstrap = ServiceComposition::bootstrap_default().await;
	let address = bootstrap.address();

	println!("decodexd listening on ws://{address}/v1/ws (loopback only; auth/TLS disabled)");

	bootstrap.run(ServerConfig::default()).await?;

	Ok(())
}
