#![allow(missing_docs)]

use std::{error::Error, process::Command};

use vergen_gitcl::{Cargo, Emitter, Gitcl};

fn main() -> Result<(), Box<dyn Error>> {
	let mut emitter = Emitter::default();

	emit_git_rerun_hints();

	emitter.add_instructions(&Cargo::builder().target_triple(true).build())?;

	// Disable the git version if installed from <https://crates.io>.
	if emitter.add_instructions(&Gitcl::builder().sha(true).build()).is_err() {
		println!("cargo:rustc-env=VERGEN_GIT_SHA=crates.io");
	}

	emitter.emit()?;

	Ok(())
}

fn emit_git_rerun_hints() {
	println!("cargo:rerun-if-changed=build.rs");

	let Ok(git_dir) = git_output(&["rev-parse", "--path-format=absolute", "--git-dir"]) else {
		return;
	};

	println!("cargo:rerun-if-changed={git_dir}/HEAD");

	let Ok(head_ref) = git_output(&["symbolic-ref", "-q", "HEAD"]) else {
		return;
	};
	let git_common_dir = git_output(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
		.unwrap_or_else(|_| git_dir.clone());

	println!("cargo:rerun-if-changed={git_common_dir}/{head_ref}");
}

fn git_output(args: &[&str]) -> Result<String, Box<dyn Error>> {
	let output = Command::new("git").args(args).output()?;

	if !output.status.success() {
		return Err(format!("git {} failed", args.join(" ")).into());
	}

	Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
