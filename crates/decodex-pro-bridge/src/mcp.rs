//! Opt-in MCP entry. One-shot tool clients do not own the Chrome connection.

use std::{
	path::Path,
	process::{Command, Stdio},
};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

use crate::{Config, Phase, ProBridge, Receipt, Request, Result};

/// Serve newline-delimited MCP on stdio. No shared Codex configuration is changed.
pub async fn serve(root: &Path) -> Result<()> {
	let bridge = ProBridge::new(Config::default(), root)?;
	let executable = std::env::current_exe().map_err(|e| e.to_string())?;
	let mut input = BufReader::new(tokio::io::stdin()).lines();
	let mut output = tokio::io::stdout();
	while let Some(line) = input.next_line().await.map_err(|e| e.to_string())? {
		let request: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
		let Some(id) = request.get("id") else { continue };
		let result = match request["method"].as_str() {
			Some("initialize") =>
				json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"decodex-research","version":env!("CARGO_PKG_VERSION")}}),
			Some("ping") => json!({}),
			Some("tools/list") => json!({"tools":tools()}),
			Some("tools/call") => match call(&bridge, root, &executable, &request["params"]) {
				Ok(value) =>
					json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}),
				Err(error) => json!({"content":[{"type":"text","text":error}],"isError":true}),
			},
			_ => {
				let error = json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}});
				output
					.write_all(format!("{error}\n").as_bytes())
					.await
					.map_err(|e| e.to_string())?;
				continue;
			},
		};
		let response = json!({"jsonrpc":"2.0","id":id,"result":result});
		output.write_all(format!("{response}\n").as_bytes()).await.map_err(|e| e.to_string())?;
		output.flush().await.map_err(|e| e.to_string())?;
	}
	Ok(())
}

fn call(bridge: &ProBridge, root: &Path, executable: &Path, params: &Value) -> Result<Value> {
	let args = &params["arguments"];
	let id = args["id"].as_str().ok_or("A saved request ID is required")?;
	match params["name"].as_str() {
		Some("pro_research") => {
			let request: Request =
				serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
			bridge.prepare(request)?;
			serde_json::to_value(start_worker(executable, root, id, &bridge.config)?)
				.map_err(|e| e.to_string())
		},
		Some("pro_resume") =>
			serde_json::to_value(start_worker(executable, root, id, &bridge.config)?)
				.map_err(|e| e.to_string()),
		Some("pro_status") => serde_json::to_value(bridge.status(id)?).map_err(|e| e.to_string()),
		Some("pro_result") => Ok(json!({"receipt":bridge.status(id)?,"answer":bridge.read(id)?})),
		_ => Err("Unknown research tool".into()),
	}
}

/// Start a saved request independently of its MCP caller. Never replay uncertain Send.
pub fn start_worker(executable: &Path, root: &Path, id: &str, config: &Config) -> Result<Receipt> {
	let bridge = ProBridge::new(config.clone(), root)?;
	let receipt = bridge.status(id)?;
	if receipt.phase == Phase::Complete {
		return Ok(receipt);
	}
	let mut command = Command::new(executable);
	command
		.arg("--state-dir")
		.arg(root)
		.args([
			"--wait-seconds",
			&config.answer_timeout.as_secs().to_string(),
			"resume",
			"--id",
			id,
		])
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null());
	use std::os::unix::process::CommandExt as _;
	command.process_group(0);
	let mut child = command.spawn().map_err(|e| e.to_string())?;
	std::thread::spawn(move || {
		let _ = child.wait();
	});
	Ok(receipt)
}

fn tools() -> Vec<Value> {
	let identity = json!({"type":"string","pattern":"^[A-Za-z0-9-]{1,64}$"});
	[
		("pro_research", "Ask ChatGPT Pro an authorized difficult research question. Returns a durable ID immediately; use pro_status and pro_result later. previous continues a completed research request.", json!({"id":identity,"prompt":{"type":"string","minLength":1},"previous":{"type":["string","null"]}}), vec!["id","prompt"]),
		("pro_status", "Read saved research state without reconnecting or sending a message.", json!({"id":identity}), vec!["id"]),
		("pro_result", "Read the complete saved answer, code, links, conversation URL and execution identity.", json!({"id":identity}), vec!["id"]),
		("pro_resume", "Recover the saved request after a disconnect or timeout. Does not resend uncertain input.", json!({"id":identity}), vec!["id"]),
	].into_iter().map(|(name, description, properties, required)| json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect()
}
