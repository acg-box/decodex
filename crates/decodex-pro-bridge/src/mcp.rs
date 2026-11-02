//! Opt-in MCP entry. One-shot tool clients do not own the Chrome connection.

use std::{
	env, fs,
	os::unix::process::CommandExt as _,
	path::{Path, PathBuf},
	process::{Command, Stdio},
	thread,
};

use serde::Deserialize;
use serde_json::Value;
use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

use crate::{Config, Delivery, Phase, ProBridge, Project, Receipt, Request, Result};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
	id: String,
	prompt: Option<String>,
	prompt_file: Option<PathBuf>,
	context_file: Option<PathBuf>,
	#[serde(default)]
	delivery: Delivery,
	previous: Option<String>,
	project: Option<Project>,
}
impl Input {
	fn request(self) -> Result<Request> {
		let prompt = match (self.prompt, self.prompt_file) {
			(Some(prompt), None) => prompt,
			(None, Some(path)) if path.is_absolute() => fs::read_to_string(path).map_err(|e| e.to_string())?,
			_ => return Err("Provide exactly one of prompt or an absolute promptFile. The file is read once and saved as immutable request input.".into()),
		};
		let context = self
			.context_file
			.map(|path| {
				if !path.is_absolute() {
					return Err("contextFile must be an absolute UTF-8 source-file path.".into());
				}

				fs::read_to_string(path).map_err(|e| e.to_string())
			})
			.transpose()?;

		Ok(Request {
			id: self.id,
			prompt,
			previous: self.previous,
			project: self.project,
			context,
			delivery: self.delivery,
		})
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
	command.process_group(0);

	let mut child = command.spawn().map_err(|e| e.to_string())?;

	thread::spawn(move || {
		let _ = child.wait();
	});

	Ok(receipt)
}

/// Serve newline-delimited MCP on stdio. No shared Codex configuration is changed.
pub async fn serve(root: &Path) -> Result<()> {
	let bridge = ProBridge::new(Config::default(), root)?;
	let executable = env::current_exe().map_err(|e| e.to_string())?;
	let mut input = BufReader::new(io::stdin()).lines();
	let mut output = io::stdout();

	while let Some(line) = input.next_line().await.map_err(|e| e.to_string())? {
		let request: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
		let Some(id) = request.get("id") else { continue };
		let result = match request["method"].as_str() {
			Some("initialize") =>
				serde_json::json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"decodex-research","version":env!("CARGO_PKG_VERSION")}}),
			Some("ping") => serde_json::json!({}),
			Some("tools/list") => serde_json::json!({"tools":tools()}),
			Some("tools/call") => match call(&bridge, root, &executable, &request["params"]) {
				Ok(value) =>
					serde_json::json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}),
				Err(error) =>
					serde_json::json!({"content":[{"type":"text","text":error}],"isError":true}),
			},
			_ => {
				let error = serde_json::json!({"jsonrpc":"2.0","id":id,"error":{"code":-32_601,"message":"Method not found"}});

				output
					.write_all(format!("{error}\n").as_bytes())
					.await
					.map_err(|e| e.to_string())?;

				continue;
			},
		};
		let response = serde_json::json!({"jsonrpc":"2.0","id":id,"result":result});

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
			let request = serde_json::from_value::<Input>(args.clone())
				.map_err(|e| e.to_string())?
				.request()?;

			bridge.prepare(request)?;

			asynchronous_receipt(start_worker(executable, root, id, &bridge.config)?)
		},
		Some("pro_resume") =>
			asynchronous_receipt(start_worker(executable, root, id, &bridge.config)?),
		Some("pro_status") => serde_json::to_value(bridge.status(id)?).map_err(|e| e.to_string()),
		Some("pro_result") =>
			Ok(serde_json::json!({"receipt":bridge.status(id)?,"answer":bridge.read(id)?})),
		_ => Err("Unknown research tool".into()),
	}
}

fn asynchronous_receipt(receipt: Receipt) -> Result<Value> {
	let mut value = serde_json::to_value(receipt).map_err(|e| e.to_string())?;

	value["completionNotification"] = serde_json::json!("decodex");

	Ok(value)
}

fn tools() -> Vec<Value> {
	let identity = serde_json::json!({"type":"string","pattern":"^[A-Za-z0-9-]{1,64}$"});

	[
		("pro_research", "Ask ChatGPT Pro an authorized research question. Provide prompt or promptFile, with optional contextFile for selected source material. Auto sends short input inline and input above 128 KiB as one UTF-8 attachment; delivery can override this. Files are read once and saved immutably. project uses a stable Decodex ID and name; omission uses General. previous continues a completed request and inherits its project and earlier attachments. Returns a durable ID immediately.", serde_json::json!({"id":identity,"prompt":{"type":"string","minLength":1},"promptFile":{"type":"string","description":"Absolute UTF-8 question-file path; not an attachment selector."},"contextFile":{"type":"string","description":"Absolute UTF-8 source-file path. Read once; large material is attached by default."},"delivery":{"type":"string","enum":["auto","inline","attachment"],"default":"auto","description":"Auto uses a 128 KiB UTF-8 byte threshold, not a token limit. Use inline for acknowledged source parts."},"project":{"type":"object","properties":{"id":{"type":"string","minLength":1},"name":{"type":"string","minLength":1}},"required":["id","name"],"additionalProperties":false},"previous":{"type":["string","null"]}}), vec!["id"]),
		("pro_status", "Read saved research state without reconnecting or sending a message.", serde_json::json!({"id":identity}), vec!["id"]),
		("pro_result", "Read the complete saved answer, code, links, conversation URL and execution identity.", serde_json::json!({"id":identity}), vec!["id"]),
		("pro_resume", "Recover the saved request after a disconnect or timeout. Does not resend uncertain input.", serde_json::json!({"id":identity}), vec!["id"]),
	].into_iter().map(|(name, description, properties, required)| serde_json::json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect()
}

#[cfg(test)]
mod tests {
	use std::fs;

	use crate::{
		mcp::{self, Config, Delivery, Input, ProBridge},
		store,
	};

	#[test]
	fn file_input_is_selected_once_and_conflicting_sources_are_rejected() {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().join("context.txt");

		fs::write(&path, "研究🦀\n".repeat(70_000)).unwrap();

		let input = serde_json::json!({"id":"long-input","promptFile":path});
		let request = serde_json::from_value::<Input>(input.clone()).unwrap().request().unwrap();
		let bridge = ProBridge::new(Config::default(), root.path()).unwrap();

		bridge.prepare(request.clone()).unwrap();

		fs::write(&path, "changed").unwrap();

		let changed = serde_json::from_value::<Input>(input.clone()).unwrap().request().unwrap();

		assert!(bridge.prepare(changed).is_err());

		let mut conflicting = input;

		conflicting["prompt"] = serde_json::json!("second source");

		assert!(serde_json::from_value::<Input>(conflicting).unwrap().request().is_err());
		assert!(
			serde_json::from_value::<Input>(serde_json::json!({"id":"none"}))
				.unwrap()
				.request()
				.is_err()
		);
	}

	#[test]
	fn context_files_are_saved_once_and_the_catalog_exposes_delivery_control() {
		let root = tempfile::tempdir().unwrap();
		let source = root.path().join("source.txt");
		let content = "独立资料🧪\n".repeat(20_000);

		fs::write(&source, &content).unwrap();

		let input = serde_json::json!({"id":"with-source","prompt":"Review the source.","contextFile":source});
		let request = serde_json::from_value::<Input>(input).unwrap().request().unwrap();
		let bridge = ProBridge::new(Config::default(), root.path()).unwrap();
		let receipt = bridge.prepare(request).unwrap();

		assert_eq!(receipt.delivery, Delivery::Attachment);
		assert_eq!(receipt.attachment.unwrap().sha256, store::digest(content.as_bytes()));

		fs::remove_file(source).unwrap();

		let record = crate::store::Store::open(root.path()).unwrap().load("with-source").unwrap();

		assert_eq!(record.request.context.as_deref(), Some(content.as_str()));

		let catalog = mcp::tools();
		let schema = &catalog.iter().find(|tool| tool["name"] == "pro_research").unwrap()["inputSchema"]
			["properties"];

		assert_eq!(schema["delivery"]["enum"], serde_json::json!(["auto", "inline", "attachment"]));
		assert!(schema.get("contextFile").is_some());
		assert!(
			serde_json::from_value::<Input>(
				serde_json::json!({"id":"relative","prompt":"Q","contextFile":"relative.txt"})
			)
			.unwrap()
			.request()
			.is_err()
		);
	}
}
