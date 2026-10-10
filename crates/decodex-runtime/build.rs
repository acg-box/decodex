//! Bind a source-built runtime to the service that packages it.
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	println!("cargo:rerun-if-env-changed=DECODEX_BUNDLED_RUNTIME_EVIDENCE");
	let evidence = if let Some(path) = env::var_os("DECODEX_BUNDLED_RUNTIME_EVIDENCE") {
		println!("cargo:rerun-if-changed={}", PathBuf::from(&path).display());
		fs::read(path)?
	} else {
		b"{}".to_vec()
	};
	fs::write(
		PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?)
			.join("runtime-evidence.json"),
		evidence,
	)?;
	Ok(())
}
