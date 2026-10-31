//! Standalone component commands. No Main, browser or official App is started.

use std::{fs, path::PathBuf, process::ExitCode, time::Duration};

use clap::{Parser, Subcommand};
use decodex_pro_bridge::{Config, ProBridge, Request, Result, install, mcp, native_host};
use futures_util as _;
use serde as _;
use sha2 as _;
use tempfile as _;
use tokio_tungstenite as _;
use url as _;

#[derive(Parser)]
#[command(about = "ChatGPT Pro research through the Decodex Chrome extension")]
struct Cli {
	/// Absolute private component root, separate from the Decodex database.
	#[arg(long)]
	state_dir: PathBuf,
	/// Maximum answer wait; expiry preserves the request for recovery.
	#[arg(long, default_value_t = 1800)]
	wait_seconds: u64,
	#[command(subcommand)]
	command: Action,
}

#[derive(Subcommand)]
enum Action {
	/// Materialize extension, opt-in Codex plugin and native registration.
	Install {
		#[arg(long)]
		registration_dir: PathBuf,
	},
	/// Remove owned installation artifacts. Keep request and answer records.
	Uninstall {
		#[arg(long)]
		registration_dir: PathBuf,
	},
	/// Chrome invokes this command through its registered Native Messaging manifest.
	#[command(hide = true)]
	NativeHost { origin: String },
	/// Serve the opt-in Agent research tools over MCP stdio.
	Mcp,
	/// Open the owned ChatGPT setup tab in existing Chrome. Never sends a question.
	Connect,
	/// Read setup readiness without opening a tab.
	Check,
	/// Close only the setup tab. Leave Chrome and research work running.
	Disconnect,
	/// Send one exact question and wait for its result.
	Ask {
		#[arg(long)]
		id: String,
		#[arg(long)]
		prompt_file: PathBuf,
		#[arg(long)]
		previous: Option<String>,
	},
	/// Save a question, start its worker and return the receipt immediately.
	Start {
		#[arg(long)]
		id: String,
		#[arg(long)]
		prompt_file: PathBuf,
		#[arg(long)]
		previous: Option<String>,
	},
	/// Recover saved work without replaying uncertain input.
	Resume {
		#[arg(long)]
		id: String,
		#[arg(long)]
		conversation: Option<String>,
	},
	/// Read a saved request receipt. Does not contact Chrome.
	Status {
		#[arg(long)]
		id: String,
	},
	/// Read the complete saved answer. Does not contact Chrome.
	Read {
		#[arg(long)]
		id: String,
		#[arg(long)]
		json: bool,
	},
}

#[tokio::main]
async fn main() -> ExitCode {
	match run(Cli::parse()).await {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			eprintln!("Decodex Research: {error}");
			ExitCode::FAILURE
		},
	}
}

async fn run(cli: Cli) -> Result<()> {
	match cli.command {
		Action::NativeHost { origin } => return native_host::serve(&cli.state_dir, &origin).await,
		Action::Mcp => return mcp::serve(&cli.state_dir).await,
		Action::Install { registration_dir } =>
			return print_json(&install::install(
				&cli.state_dir,
				&registration_dir,
				&std::env::current_exe().map_err(|e| e.to_string())?,
			)?),
		Action::Uninstall { registration_dir } =>
			return install::uninstall(&cli.state_dir, &registration_dir),
		_ => {},
	}
	let config =
		Config { answer_timeout: Duration::from_secs(cli.wait_seconds), ..Default::default() };
	let bridge = ProBridge::new(config.clone(), &cli.state_dir)?;
	let detached = matches!(&cli.command, Action::Start { .. });
	let action = async {
		match cli.command {
			Action::Connect => print_json(&bridge.connect().await?)?,
			Action::Check => print_json(&bridge.check().await?)?,
			Action::Disconnect => bridge.disconnect().await?,
			Action::Ask { id, prompt_file, previous }
			| Action::Start { id, prompt_file, previous } => {
				let request = Request {
					id: id.clone(),
					prompt: fs::read_to_string(prompt_file).map_err(|e| e.to_string())?,
					previous,
				};
				if detached {
					bridge.prepare(request)?;
					print_json(&mcp::start_worker(
						&std::env::current_exe().map_err(|e| e.to_string())?,
						&cli.state_dir,
						&id,
						&config,
					)?)?;
				} else {
					print_json(&bridge.ask(request).await?)?;
				}
			},
			Action::Resume { id, conversation } => print_json(&match conversation {
				Some(url) => bridge.resume_at(&id, &url).await?,
				None => bridge.resume(&id).await?,
			})?,
			Action::Status { id } => print_json(&bridge.status(&id)?)?,
			Action::Read { id, json } => {
				let answer = bridge.read(&id)?;
				if json {
					print_json(&answer)?;
				} else {
					println!("{}", answer.text);
				}
			},
			_ => unreachable!("handled before research work"),
		}
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
