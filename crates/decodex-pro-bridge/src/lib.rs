//! Independent ChatGPT Pro execution through the installed official Codex browser runtime.
//!
//! This component does not configure Main, the model catalog, Chrome, or native messaging.
//! It needs the official Chrome extension, a signed-in ChatGPT session, and the existing
//! ChatGPT.app Codex and Node runtime. It does not install or download these programs.
//!
//! Each connection keeps one genuine native turn active for browser approval checks. It
//! retains the native execution metadata without edits. A zero-model worker is not used.
//! The contract was checked against official app-server schemas and openai/codex commit
//! `0e67502123b4c6952b9fa5d672d2907fe4059f56`, including `session/mcp.rs` and the
//! `mcpServer/tool/call` tests. The installed official binary must support these methods.
//!
//! Callers own user authorization. Only submit prompts that the user authorized for ChatGPT.
//! Recovery observes a saved conversation. An uncertain Send is never replayed automatically.
//! The control turn receives an immutable file descriptor with hashes. Rust transfers the
//! message through the existing browser runtime without adding its text to the control model.
//! The component has no fixed prompt byte limit. ChatGPT can reject an oversized message.
//!
//! Run the standalone CLI with `cargo run --locked -p decodex-pro-bridge -- --help`.
//! `ask --id ID --prompt-file FILE` creates a request. Add `--previous ID` to continue
//! a completed request. `resume --id ID` observes a pending request. `status` and `read`
//! do not start the runtime. Use `read --json` to retain code blocks and source links.
//! If Send was interrupted before the first URL was saved, `resume --conversation URL`
//! verifies that candidate against the exact saved prompt before accepting it.
//! Ctrl+C waits for owned-tab and child-process shutdown. Abrupt OS termination can
//! leave a background tab; it does not invalidate saved submission receipts.
//! Supply an absolute private `--state-dir` and, if needed, `--extension-instance`.
//!
//! The opt-in `installed_official_runtime_selects_pro_in_background` test uses
//! `DECODEX_PRO_LIVE_APP` and `DECODEX_PRO_LIVE_EXTENSION`. It operates one temporary
//! test tab and sends no ChatGPT message. Normal tests do not use the account or browser.

mod native;
mod page;
mod store;
mod workflow;

#[cfg(test)] mod live_tests;

use clap as _;
use std::{path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

/// Component result. Runtime failures retain their action and official failure reason.
pub type Result<T> = std::result::Result<T, String>;

/// Authority of the caller that starts the native browser control turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlOrigin {
	/// A direct user request, such as the standalone CLI.
	User,
	/// Application-owned work, such as a consultation delegated by Main.
	Application,
}

/// Installed runtime paths and bounded wait settings. No global settings are written.
#[derive(Clone, Debug)]
pub struct Config {
	/// Preserve application authority when Main delegates work. Never infer this from model input.
	pub control_origin: ControlOrigin,
	/// Installed official Codex app-server executable.
	pub codex: PathBuf,
	/// Installed official browser client module.
	pub browser_client: PathBuf,
	/// Configured native MCP server that provides the `js` tool.
	pub mcp_server: String,
	/// Optional exact Chrome extension instance. Required if discovery is ambiguous.
	pub extension_instance: Option<String>,
	/// Maximum wait for one ChatGPT answer. Expiry retains a resumable request.
	pub answer_timeout: Duration,
	/// Delay between page observations.
	pub poll_interval: Duration,
}

impl Config {
	/// Select an existing ChatGPT.app bundle without installing or configuring it.
	pub fn installed_app(app: impl Into<PathBuf>) -> Self {
		let resources = app.into().join("Contents/Resources");
		Self {
			control_origin: ControlOrigin::User,
			codex: resources.join("codex-cli/CodexCLI.app/Contents/MacOS/codex"),
			browser_client: resources.join("cua_node/lib/node_modules/@oai/cua/dist/lib/js/oai_js_browser/dist/skill/scripts/browser-client.mjs"),
			mcp_server: "node_repl".into(),
			extension_instance: None,
			answer_timeout: Duration::from_secs(30 * 60),
			poll_interval: Duration::from_secs(3),
		}
	}
}

/// User-authorized input. IDs are stable within one private component directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
	/// Idempotency key, using 1 to 64 ASCII letters, digits, or hyphens.
	pub id: String,
	/// Exact question to send to ChatGPT, preceded by a component request marker.
	pub prompt: String,
	/// Completed request in this store whose ChatGPT conversation will continue.
	pub previous: Option<String>,
}

/// Durable submission state. `Sending` must be reconciled, never replayed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
	/// No Send has been attempted.
	Prepared,
	/// A durable fence was written before Send. Its outcome can be uncertain.
	Sending,
	/// The matching user message and exact conversation URL were observed.
	Submitted,
	/// The matching assistant answer and final UI actions were observed.
	Complete,
}

/// Request status without the prompt or answer body.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
	/// Stable component request ID.
	pub id: String,
	/// Current durable phase.
	pub phase: Phase,
	/// Canonical owned-tab URL. It can be unconfirmed while the phase is `Sending`.
	pub conversation_url: Option<String>,
	/// Verified visible ChatGPT model label.
	pub model: Option<String>,
	/// Exact matching user message identity.
	pub user_message_id: Option<String>,
	/// Number of messages observed before this request was submitted.
	pub baseline_messages: usize,
	/// Most recent recoverable failure. It can include rendered UI diagnostics.
	pub last_error: Option<String>,
}

/// Link rendered inside the completed assistant message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Link {
	/// Visible link label.
	pub label: String,
	/// DOM-backed link URL.
	pub url: String,
}

/// Complete DOM-backed answer, with code and links retained separately.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Answer {
	/// Exact assistant message identity.
	pub message_id: String,
	/// Full rendered text. No preview or character truncation is applied.
	pub text: String,
	/// Complete code block text, preserving line breaks.
	pub code_blocks: Vec<String>,
	/// Links rendered in the answer.
	pub links: Vec<Link>,
}

/// Standalone component owner. It has no Main or model-selector dependencies.
#[derive(Clone, Debug)]
pub struct ProBridge {
	config: Config,
	root: PathBuf,
	cancelled: watch::Sender<bool>,
}

impl ProBridge {
	/// Use one absolute private state directory. The runtime is started only by `ask` or `resume`.
	pub fn new(config: Config, state_dir: impl Into<PathBuf>) -> Result<Self> {
		let root = state_dir.into();
		store::Store::open(&root)?;
		let (cancelled, _) = watch::channel(false);
		Ok(Self { config, root, cancelled })
	}

	/// Persist an exact request before acknowledging it to an asynchronous caller.
	/// This does not start a process or send a message. Resume the returned ID to execute it.
	pub fn prepare(&self, request: Request) -> Result<Receipt> {
		let store = store::Store::open(&self.root)?;
		if let Some(saved) = store.existing(&request)? {
			return Ok(saved.receipt);
		}
		let _lease = store.lease()?;
		Ok(store.prepare(request)?.receipt)
	}

	/// Submit or recover one exact request. The same ID cannot identify different input.
	pub async fn ask(&self, request: Request) -> Result<Receipt> {
		workflow::run(self.clone(), Some(request), None, None).await
	}

	/// Recover a saved request without creating a new question or replaying an uncertain Send.
	pub async fn resume(&self, id: &str) -> Result<Receipt> {
		workflow::run(self.clone(), None, Some(id.into()), None).await
	}

	/// Reconcile an uncertain first Send against an explicit candidate conversation.
	/// Completion requires the exact saved request marker and prompt to be observed.
	/// This method never sends a message.
	pub async fn resume_at(&self, id: &str, conversation_url: &str) -> Result<Receipt> {
		let candidate = page::conversation_url(conversation_url)?;
		workflow::run(self.clone(), None, Some(id.into()), Some(candidate)).await
	}

	/// Stop this owner and its clones. Await active runs for cleanup.
	/// A new owner can resume saved requests. This does not stop the remote ChatGPT answer.
	pub fn cancel(&self) {
		self.cancelled.send_replace(true);
	}

	/// Read the latest atomic receipt without starting a process or acquiring a run lease.
	pub fn status(&self, id: &str) -> Result<Receipt> {
		Ok(store::Store::open(&self.root)?.load(id)?.receipt)
	}

	/// Read the complete saved answer without starting the official runtime.
	pub fn read(&self, id: &str) -> Result<Answer> {
		let record = store::Store::open(&self.root)?.load(id)?;
		if record.receipt.phase != Phase::Complete {
			return Err("The request is not complete. Resume it with the same ID.".into());
		}
		record.answer.ok_or_else(|| "The completed request has no saved answer.".into())
	}
}
