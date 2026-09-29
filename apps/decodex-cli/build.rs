//! Embed the exact source checkout identity in the unified Decodex executable.

use std::{io, process::Command};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let commit = git(["rev-parse", "HEAD"])?;
	let dirty = !git(["status", "--porcelain"])?.is_empty();
	// A Git ref does not change when source files are edited or restored. This deliberately
	// absent path refreshes identity on each build, including changes outside this package.
	println!("cargo:rerun-if-changed={}/refresh-build-identity", std::env::var("OUT_DIR")?);
	println!("cargo:rustc-env=DECODEX_BUILD_COMMIT={commit}");
	println!("cargo:rustc-env=DECODEX_BUILD_DIRTY={dirty}");
	Ok(())
}

fn git<const N: usize>(args: [&str; N]) -> Result<String, io::Error> {
	let output = Command::new("git").args(args).output()?;
	if !output.status.success() {
		return Err(io::Error::other("git build identity command failed"));
	}
	String::from_utf8(output.stdout)
		.map(|value| value.trim().to_owned())
		.map_err(|_| io::Error::other("git build identity was not UTF-8"))
}
