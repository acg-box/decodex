//! Decodex runtime bootstrap and CLI entrypoint.

pub mod app_bridge;
pub mod config;
pub mod state;
pub mod workflow;

mod accounts;
mod active_run_env;
mod agent;
mod archive_hygiene;
mod autonomy_objective;
mod autonomy_proposal;
mod autonomy_runtime_policy;
mod autonomy_signal;
mod cli;
mod codex_config;
mod commit_message;
mod default_branch_sync;
mod execution_program;
mod git_credentials;
mod github;
mod loop_contract;
mod maintenance;
mod manual;
mod mcp;
mod orchestrator;
mod program_intake;
mod pull_request;
mod prelude {
	pub use color_eyre::{Result, eyre};
}
mod recovery;
mod run_control;
mod runtime;
mod tracker;
mod worktree;

use std::{fs, panic, process};

use clap::Parser;
use tracing_appender::{
	non_blocking::WorkerGuard,
	rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::EnvFilter;

use crate::{cli::Cli, prelude::Result};

#[cfg(not(unix))]
compile_error!("Decodex supports only Unix targets (macOS and Linux). Windows is unsupported.");

/// Run the Decodex CLI after initializing error reporting, logging, and the panic hook.
pub fn run() -> Result<()> {
	color_eyre::install()?;

	let _guard = init_tracing()?;

	install_panic_hook();

	Cli::parse().run()
}

fn init_tracing() -> Result<WorkerGuard> {
	let log_dir = runtime::log_dir()?;

	fs::create_dir_all(&log_dir)?;

	let (non_blocking, guard) = tracing_appender::non_blocking(
		RollingFileAppender::builder()
			.rotation(Rotation::DAILY)
			.max_log_files(30)
			.filename_suffix("log")
			.build(log_dir)?,
	);
	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

	tracing_subscriber::fmt()
		.with_env_filter(filter)
		.with_ansi(false)
		.with_writer(non_blocking)
		.init();

	Ok(guard)
}

fn install_panic_hook() {
	let default_hook = panic::take_hook();

	panic::set_hook(Box::new(move |panic_info| {
		default_hook(panic_info);

		process::abort();
	}));
}

#[cfg(test)] mod test_support;
