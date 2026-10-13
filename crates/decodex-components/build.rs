//! Select one runtime requirement for all binaries that use this crate.
use std::{env, fs, path::PathBuf};

fn main() {
	println!("cargo:rerun-if-env-changed=DECODEX_RUNTIME_LOCK_PATH");
	let input = env::var_os("DECODEX_RUNTIME_LOCK_PATH").map_or_else(
		|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../codex-runtime.lock.json"),
		PathBuf::from,
	);
	println!("cargo:rerun-if-changed={}", input.display());
	let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
	fs::copy(&input, output.join("codex-runtime.lock.json"))
		.expect("runtime requirement must exist");
}
