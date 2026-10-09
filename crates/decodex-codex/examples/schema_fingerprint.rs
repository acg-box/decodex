//! Validate generated Codex protocol schemas for the bundled runtime lock.
use base64 as _;
use decodex_core as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use std::{env, path::PathBuf};
use tempfile as _;
use tokio as _;
use url as _;
use zeroize as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let directory = PathBuf::from(env::args_os().nth(1).ok_or("expected schema directory")?);
	let evidence = decodex_codex::schema::GeneratedSchemaEvidence::load(&directory)
		.map_err(|missing| format!("incompatible Codex schema: {}", missing.join(", ")))?;
	println!("{}", evidence.fingerprint);
	Ok(())
}
