//! Sole Decodex vNext server composition root.

use std::{
	error::Error,
	net::{Ipv4Addr, SocketAddr},
	process,
	time::{SystemTime, UNIX_EPOCH},
};

use decodex_runtime::{ServerConfig, ServerId, ServiceComposition};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let address = SocketAddr::from((Ipv4Addr::LOCALHOST, 49_152));
	let epoch_nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
	let server_id = ServerId::new(format!("server-{}-{epoch_nanos}", process::id()))
		.expect("generated server ID is bounded");

	println!("decodexd listening on ws://{address}/v1/ws (loopback only; auth/TLS disabled)");

	ServiceComposition::foundation()
		.protocol_server(server_id, ServerConfig::default())
		.run(address)
		.await?;

	Ok(())
}
