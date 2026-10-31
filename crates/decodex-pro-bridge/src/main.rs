//! Standalone command interface. This executable does not start Decodex Main.

use std::{fs, path::PathBuf, process::ExitCode, time::Duration};

use clap::{Parser, Subcommand};
use decodex_codex as _;
use decodex_pro_bridge::{Config, ProBridge, Request, Result};
use serde as _;
use sha2 as _;
use tempfile as _;
use url as _;

#[derive(Parser)]
#[command(about = "Independent ChatGPT Pro component using the existing official Chrome extension")]
struct Cli {
	/// Existing ChatGPT.app bundle. No installation or configuration is performed.
	#[arg(long, default_value = "/Applications/ChatGPT.app")]
	app: PathBuf,
	/// Absolute private component directory (0700). It is separate from the Decodex database.
	#[arg(long)]
	state_dir: PathBuf,
	/// Exact extension instance when more than one Chrome instance is connected.
	#[arg(long)]
	extension_instance: Option<String>,
	/// Maximum answer wait. A timeout retains the request for `resume`.
	#[arg(long, default_value_t = 1800)]
	wait_seconds: u64,
	#[command(subcommand)]
	command: Action,
}

#[derive(Subcommand)]
enum Action {
	/// Submit one authorized question, or recover the same exact request ID.
	Ask {
		#[arg(long)]
		id: String,
		#[arg(long)]
		prompt_file: PathBuf,
		/// Completed component request whose ChatGPT conversation will continue.
		#[arg(long)]
		previous: Option<String>,
	},
	/// Observe the saved conversation without sending another question.
	Resume {
		#[arg(long)]
		id: String,
		/// Candidate URL only for an uncertain first Send whose URL was not saved.
		/// It is accepted only after the exact saved request is observed. Never resends.
		#[arg(long)]
		conversation: Option<String>,
	},
	/// Print one receipt without its prompt or answer body. No runtime is started.
	Status {
		#[arg(long)]
		id: String,
	},
	/// Read the complete saved answer. No runtime is started.
	Read {
		#[arg(long)]
		id: String,
		/// Include exact message identity, full code blocks, and rendered links.
		#[arg(long)]
		json: bool,
	},
}

#[tokio::main]
async fn main() -> ExitCode {
	match run(Cli::parse()).await {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			eprintln!("Pro bridge: {error}");
			ExitCode::FAILURE
		},
	}
}

async fn run(cli: Cli) -> Result<()> {
	let mut config = Config::installed_app(cli.app);
	config.extension_instance = cli.extension_instance;
	config.answer_timeout = Duration::from_secs(cli.wait_seconds);
	let bridge = ProBridge::new(config, cli.state_dir)?;
	let action = async {
		match cli.command {
			Action::Ask { id, prompt_file, previous } => {
				let prompt = fs::read_to_string(prompt_file).map_err(|e| e.to_string())?;
				print_json(&bridge.ask(Request { id, prompt, previous }).await?)?;
			},
			Action::Resume { id, conversation } => {
				let receipt = match conversation {
					Some(url) => bridge.resume_at(&id, &url).await?,
					None => bridge.resume(&id).await?,
				};
				print_json(&receipt)?;
			},
			Action::Status { id } => print_json(&bridge.status(&id)?)?,
			Action::Read { id, json } => {
				let answer = bridge.read(&id)?;
				if json {
					print_json(&answer)?;
				} else {
					println!("{}", answer.text);
				}
			},
		};
		Ok(())
	};
	tokio::pin!(action);
	tokio::select! {
		biased;
		result = &mut action => result,
		signal = tokio::signal::ctrl_c() => {
			bridge.cancel();
			let result = action.await;
			signal.map_err(|e| e.to_string())?;
			result
		},
	}
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
	println!("{}", serde_json::to_string_pretty(value).map_err(|e| e.to_string())?);
	Ok(())
}
