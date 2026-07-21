use std::{env, path::PathBuf, process::ExitCode};

use decodex_core::DecodexRoot;
use decodex_runtime::run_manual_retained_title_experiment;

#[tokio::main]
async fn main() -> ExitCode {
	let mut arguments = env::args_os();
	let _program = arguments.next();
	let Some(request_path) = arguments.next().map(PathBuf::from) else {
		eprintln!("usage: decodex-retained-title-experiment REQUEST.json");
		return ExitCode::from(2);
	};
	if arguments.next().is_some() {
		eprintln!("usage: decodex-retained-title-experiment REQUEST.json");
		return ExitCode::from(2);
	}
	let root = match DecodexRoot::platform_default() {
		Ok(root) => root,
		Err(_) => {
			eprintln!("manual retained-title experiment failed: ConfigurationUnavailable");
			return ExitCode::FAILURE;
		},
	};
	match run_manual_retained_title_experiment(root, &request_path).await {
		Ok(report) => match serde_json::to_string(&report) {
			Ok(document) => {
				println!("{document}");
				ExitCode::SUCCESS
			},
			Err(_) => ExitCode::FAILURE,
		},
		Err(error) => {
			eprintln!("manual retained-title experiment failed: {error}");
			ExitCode::FAILURE
		},
	}
}
