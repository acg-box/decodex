//! Decodex vNext command-line client composition root.

use std::{
	io::{self, Write as _},
	process::ExitCode,
};

use clap::Parser as _;
use decodex_protocol as _;
use serde as _;
use serde_json as _;
#[cfg(test)] use tempfile as _;
use toml_edit as _;

use decodex_cli::{self, Cli};

#[tokio::main]
async fn main() -> ExitCode {
	let output = decodex_cli::execute(Cli::parse()).await;

	if !output.text().is_empty() {
		let write = if output.is_error_stream() {
			writeln!(io::stderr(), "{}", output.text())
		} else {
			writeln!(io::stdout(), "{}", output.text())
		};

		if write.is_err() {
			return ExitCode::from(2);
		}
	}

	ExitCode::from(output.exit_code())
}
