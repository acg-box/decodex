//! Validate generated Codex protocol schemas for the bundled runtime lock.
use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let directory = PathBuf::from(env::args_os().nth(1).ok_or("expected schema directory")?);
	let evidence = decodex_codex::schema::GeneratedSchemaEvidence::load(&directory)
		.map_err(|missing| format!("incompatible Codex schema: {}", missing.join(", ")))?;
	println!("{}", evidence.fingerprint);
	Ok(())
}
