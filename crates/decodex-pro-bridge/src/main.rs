//! Standalone command interface. This executable does not start Decodex Main.

use std::{
	fs,
	path::PathBuf,
	process::{ExitCode, Stdio},
	time::Duration,
};

use clap::{Parser, Subcommand};
use decodex_pro_bridge::{Config, ProBridge, Request, Result};
use futures_util as _;
use serde as _;
use sha2 as _;
use tempfile as _;
use tokio_tungstenite as _;
use url as _;

#[derive(Parser)]
#[command(about = "ChatGPT research in a dedicated Chrome profile; no API key or control model")]
struct Cli {
	/// Existing Chrome executable. No browser installation is performed.
	#[arg(long, default_value = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")]
	chrome: PathBuf,
	/// Absolute private component directory (0700). It is separate from the Decodex database.
	#[arg(long)]
	state_dir: PathBuf,
	/// Use a headless browser in isolated tests; normal login needs a window.
	#[arg(long)]
	headless: bool,
	/// Maximum answer wait. A timeout retains the request for `resume`.
	#[arg(long, default_value_t = 1800)]
	wait_seconds: u64,
	#[command(subcommand)]
	command: Action,
}

#[derive(Subcommand)]
enum Action {
	/// Open only the component's login window. Never sends a message.
	Connect,
	/// Check the login window without opening or focusing a browser.
	Check,
	/// Close only the dedicated browser; keep saved requests and login state.
	Disconnect,
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
	/// Persist the request, start an independent worker and return its ID immediately.
	Start {
		#[arg(long)]
		id: String,
		#[arg(long)]
		prompt_file: PathBuf,
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
	let mut config = Config::chrome(cli.chrome.clone());
	config.headless = cli.headless;
	config.answer_timeout = Duration::from_secs(cli.wait_seconds);
	let bridge = ProBridge::new(config, cli.state_dir.clone())?;
	let detached = matches!(&cli.command, Action::Start { .. });
	let action = async {
		match cli.command {
			Action::Connect => print_json(&bridge.connect().await?)?,
			Action::Check => print_json(&bridge.check().await?)?,
			Action::Disconnect => bridge.disconnect().await?,
			Action::Ask { id, prompt_file, previous }
			| Action::Start { id, prompt_file, previous } => {
				let prompt = fs::read_to_string(prompt_file).map_err(|e| e.to_string())?;
				let request = Request { id: id.clone(), prompt, previous };
				if detached {
					let receipt = bridge.prepare(request)?;
					if receipt.phase != decodex_pro_bridge::Phase::Complete {
						let mut worker = std::process::Command::new(
							std::env::current_exe().map_err(|e| e.to_string())?,
						);
						worker
							.arg("--chrome")
							.arg(&cli.chrome)
							.arg("--state-dir")
							.arg(&cli.state_dir)
							.arg("--wait-seconds")
							.arg(cli.wait_seconds.to_string());
						if cli.headless {
							worker.arg("--headless");
						}
						worker
							.args(["resume", "--id", &id])
							.stdin(Stdio::null())
							.stdout(Stdio::null())
							.stderr(Stdio::null());
						#[cfg(unix)]
						{
							use std::os::unix::process::CommandExt as _;
							worker.process_group(0);
						}
						let mut child = worker.spawn().map_err(|e| e.to_string())?;
						std::thread::spawn(move || {
							let _ = child.wait();
						});
					}
					print_json(&receipt)?;
				} else {
					print_json(&bridge.ask(request).await?)?;
				}
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
