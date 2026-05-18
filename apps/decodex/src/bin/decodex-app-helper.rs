//! Decodex App helper entrypoint.

#![allow(unused_crate_dependencies)]

use std::process::ExitCode;

use decodex::app_bridge;

fn main() -> ExitCode {
	match app_bridge::run() {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			eprintln!("{error:?}");

			ExitCode::FAILURE
		},
	}
}
