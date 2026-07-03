use std::{
	path::{Path, PathBuf},
	process::Command,
};

use clap::Args;

use crate::prelude::{Result, eyre};

#[derive(Debug, Args)]
pub(in crate::cli) struct AppCommand {
	/// Open this Decodex app bundle instead of the installed `Decodex` app.
	#[arg(long, value_name = "APP_BUNDLE")]
	pub(in crate::cli) bundle: Option<PathBuf>,
	/// Ask LaunchServices to open a new app instance.
	#[arg(short = 'n', long)]
	pub(in crate::cli) new: bool,
}
impl AppCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		open_decodex_app(self.bundle.as_deref(), self.new)
	}
}

#[cfg(any(target_os = "macos", test))]
pub(in crate::cli) fn decodex_app_open_args(
	bundle: Option<&Path>,
	new: bool,
) -> Vec<std::ffi::OsString> {
	let mut args = Vec::new();

	if new {
		args.push(std::ffi::OsString::from("-n"));
	}

	if let Some(bundle) = bundle {
		args.push(bundle.as_os_str().to_owned());
	} else {
		args.push(std::ffi::OsString::from("-a"));
		args.push(std::ffi::OsString::from("Decodex"));
	}

	args
}

#[cfg(target_os = "macos")]
fn open_decodex_app(bundle: Option<&Path>, new: bool) -> Result<()> {
	let args = decodex_app_open_args(bundle, new);
	let status = Command::new("/usr/bin/open")
		.args(args)
		.status()
		.map_err(|error| eyre::eyre!("Failed to start `open` for Decodex App: {error}"))?;

	if !status.success() {
		eyre::bail!("Failed to open Decodex App: `open` exited with {status}");
	}

	println!("Opened Decodex App.");

	Ok(())
}

#[cfg(not(target_os = "macos"))]
fn open_decodex_app(_bundle: Option<&Path>, _new: bool) -> Result<()> {
	eyre::bail!("`decodex app` is only supported on macOS");
}
