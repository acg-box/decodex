//! ChatGPT research through Decodex's own Chrome extension and Rust native host.
//! The extension operates only research tabs that it created in the existing browser.
//! No official desktop, control-model inference, Node, new browser, or API key is used.
//! Send fencing and exact message receipts survive controller and host disconnects.
//! Provider login and verification remain user actions; they are never bypassed.

pub mod events;
pub mod install;
pub mod mcp;
pub mod native_host;

mod chrome;
mod identity;
mod native_wire;
mod page;
mod projects;
mod store;
mod workflow;

pub use identity::ExecutionIdentity;

use std::{path::PathBuf, time::Duration};

use clap as _;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use tokio::{
	sync::watch::{self, Sender},
	time::{self, Instant},
};

use chrome::Chrome;
use store::Store;

/// Component result. Runtime failures retain their action and official failure reason.
pub type Result<T> = std::result::Result<T, String>;

/// Bounded answer observation settings. The browser is owned by its user.
#[derive(Clone, Debug)]
pub struct Config {
	/// Maximum wait for one ChatGPT answer. Expiry retains a resumable request.
	pub answer_timeout: Duration,
	/// Delay between page observations.
	pub poll_interval: Duration,
}
impl Default for Config {
	fn default() -> Self {
		Self { answer_timeout: Duration::from_secs(30 * 60), poll_interval: Duration::from_secs(1) }
	}
}

/// Compact setup report suitable for the existing Components settings view.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
	/// Authentication and page readiness, separate from per-turn Pro verification.
	pub state: ConnectionState,
	/// Informational picker label. This is not execution evidence.
	pub selected_model: Option<String>,
}

/// User-authorized input. IDs are stable within one private component directory.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
	/// Idempotency key, using 1 to 64 ASCII letters, digits, or hyphens.
	pub id: String,
	/// Original question, retained verbatim even when delivered inside an attachment.
	pub prompt: String,
	/// Completed request in this store whose ChatGPT conversation will continue.
	pub previous: Option<String>,
	/// Stable Decodex project identity. Continuations inherit the original project.
	#[serde(default)]
	pub project: Option<Project>,
	/// Selected UTF-8 source content, saved before the caller returns.
	#[serde(default)]
	pub context: Option<String>,
	/// Auto keeps short input inline and sends large input as a source attachment.
	#[serde(default)]
	pub delivery: Delivery,
}

/// Immutable source manifest and the exact file reference observed after Send.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
	/// Generated .txt name, including the request ID and content digest prefix.
	pub name: String,
	/// Exact UTF-8 file length.
	pub bytes: usize,
	/// Full SHA-256 of the uploaded bytes.
	pub sha256: String,
	/// Whether the file contains the question as well as its selected context.
	pub includes_question: bool,
	/// Provider file ID bound to the exact submitted user message, when observed.
	pub file_id: Option<String>,
}

/// A Decodex project, distinct from its display name and ChatGPT URL.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
	/// Stable project ID supplied by the calling Agent.
	pub id: String,
	/// Human-readable project name.
	pub name: String,
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
	/// Actual model from the response. Never populated from the model picker.
	pub model: Option<String>,
	/// Exact assistant-message execution evidence. Absent until observed.
	pub execution: Option<ExecutionIdentity>,
	/// Exact matching user message identity.
	pub user_message_id: Option<String>,
	/// Whether message text matched verbatim or through declared Markdown encoding.
	/// Attachment integrity and submitted-file identity are recorded separately.
	pub input_binding: Option<String>,
	/// ChatGPT Project that owns this research conversation.
	pub project_url: Option<String>,
	/// Number of messages observed before this request was submitted.
	pub baseline_messages: usize,
	/// Most recent recoverable failure. It can include rendered UI diagnostics.
	pub last_error: Option<String>,
	/// Actual saved delivery mode. Existing plain-text records remain inline.
	#[serde(default = "inline_delivery")]
	pub delivery: Delivery,
	/// Source integrity and submission evidence; absent for inline input.
	#[serde(default)]
	pub attachment: Option<Attachment>,
}

/// Link rendered inside the completed assistant message.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Link {
	/// Visible link label.
	pub label: String,
	/// DOM-backed link URL.
	pub url: String,
}

/// Complete answer, using original response text when available and retaining code and links.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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
	cancelled: Sender<bool>,
}
impl ProBridge {
	/// Use one absolute private state directory. Construction starts no browser.
	pub fn new(config: Config, state_dir: impl Into<PathBuf>) -> Result<Self> {
		let root = state_dir.into();

		Store::open(&root)?;

		let (cancelled, _) = watch::channel(false);

		Ok(Self { config, root, cancelled })
	}

	/// Open an owned ChatGPT tab in the existing Chrome. Never sends a question.
	pub async fn connect(&self) -> Result<Connection> {
		let store = Store::open(&self.root)?;
		let mut browser = Chrome::connect(&self.root).await?;
		let saved = store.setup_target()?;
		let target = browser.login_target(saved.as_deref()).await?;

		browser.open(target.as_deref(), Some("https://chatgpt.com/"), true).await?;

		{
			let _lease = store.lease()?;

			store.save_setup_target(browser.target.as_deref().ok_or("Missing setup tab.")?)?;
		}

		page::connection(&mut browser, Duration::from_secs(15)).await
	}

	/// Read setup readiness without opening a window or starting Chrome.
	pub async fn check(&self) -> Result<Option<Connection>> {
		let store = Store::open(&self.root)?;
		let saved = store.setup_target()?;

		if !native_host::socket_path(&self.root).exists() {
			return Ok(None);
		}

		let mut browser = Chrome::connect(&self.root).await?;
		let Some(_) = saved else {
			return Ok(None);
		};
		let Some(target) = browser.login_target(saved.as_deref()).await? else { return Ok(None) };

		browser.open(Some(&target), None, false).await?;

		if saved.as_deref() != Some(&target) {
			let _lease = store.lease()?;

			store.save_setup_target(&target)?;
		}

		page::connection(&mut browser, Duration::ZERO).await.map(Some)
	}

	/// Reload updated extension files after idle installation. Keep its owned tabs.
	pub async fn reload_extension(&self) -> Result<()> {
		let store = Store::open(&self.root)?;
		let _execution = store
			.execution_lease()?
			.ok_or("Wait for research to finish before reloading its extension.")?;
		let mut browser = Chrome::connect(&self.root).await?;

		browser.command("Decodex.reload", serde_json::json!({})).await?;

		Ok(())
	}

	/// Remove only the connected research extension through Chrome's self-removal API.
	/// If Chrome is not connected, its extension can also be removed in Chrome settings.
	pub async fn remove_extension(&self) -> Result<()> {
		let store = Store::open(&self.root)?;
		let _execution = store
			.execution_lease()?
			.ok_or("Wait for research to finish before removing its extension.")?;

		if !native_host::socket_path(&self.root).exists() {
			return Ok(());
		}

		let mut browser = Chrome::connect(&self.root).await?;

		browser.command("Decodex.uninstall", serde_json::json!({})).await?;

		let deadline = Instant::now() + Duration::from_secs(5);

		loop {
			if store.browser_lease()?.is_some() {
				return Ok(());
			}
			if Instant::now() >= deadline {
				return Err("Chrome did not remove its research extension. Remove Decodex Research in Chrome, then retry.".into());
			}

			time::sleep(Duration::from_millis(100)).await;
		}
	}

	/// Close only the setup tab. Research records, other tabs and Chrome are retained.
	pub async fn disconnect(&self) -> Result<()> {
		let store = Store::open(&self.root)?;
		let _lease = store
			.execution_lease()?
			.ok_or("Research is still running. Wait before disconnecting setup.")?;

		if !native_host::socket_path(&self.root).exists() {
			return Ok(());
		}

		let mut browser = Chrome::connect(&self.root).await?;

		if let Some(target) = store.setup_target()? {
			let targets = browser.command("Target.getTargets", serde_json::json!({})).await?;

			if targets["targetInfos"]
				.as_array()
				.is_some_and(|targets| targets.iter().any(|t| t["targetId"] == target))
			{
				browser.target = Some(target);

				browser.close_tab().await?;
			}
		}

		browser.detach().await
	}

	/// Persist an exact request before acknowledging it to an asynchronous caller.
	/// This does not start a process or send a message. Resume the returned ID to execute it.
	pub fn prepare(&self, request: Request) -> Result<Receipt> {
		let store = Store::open(&self.root)?;

		if let Some(saved) = store.existing(&request)? {
			return Ok(saved.receipt);
		}

		let _lease = store.lease()?;

		Ok(store.prepare(request)?.receipt)
	}

	/// Submit or recover one exact request. The same ID cannot identify different input.
	pub async fn ask(&self, request: Request) -> Result<Receipt> {
		workflow::run(self.clone(), Some(request), None, None, false).await
	}

	/// Recover a saved request without creating a new question or replaying an uncertain Send.
	pub async fn resume(&self, id: &str) -> Result<Receipt> {
		workflow::run(self.clone(), None, Some(id.into()), None, false).await
	}

	/// Collect an already submitted answer. Prepared requests are never sent, even after recovery.
	pub async fn collect(&self, id: &str) -> Result<Receipt> {
		workflow::run(self.clone(), None, Some(id.into()), None, true).await
	}

	/// Reconcile an uncertain first Send against an explicit candidate conversation.
	/// Completion requires the exact saved request marker and prompt to be observed.
	/// This method never sends a message.
	pub async fn resume_at(&self, id: &str, conversation_url: &str) -> Result<Receipt> {
		let candidate = page::conversation_url(conversation_url)?;

		workflow::run(self.clone(), None, Some(id.into()), Some(candidate), false).await
	}

	/// Stop this owner and its clones. Await active runs for cleanup.
	/// A new owner can resume saved requests. This does not stop the remote ChatGPT answer.
	pub fn cancel(&self) {
		self.cancelled.send_replace(true);
	}

	/// Read the latest atomic receipt without starting a process or acquiring a run lease.
	pub fn status(&self, id: &str) -> Result<Receipt> {
		Ok(Store::open(&self.root)?.load(id)?.receipt)
	}

	/// Read the complete, Pro-verified saved answer without starting Chrome.
	pub fn read(&self, id: &str) -> Result<Answer> {
		let record = Store::open(&self.root)?.load(id)?;

		if record.receipt.phase != Phase::Complete {
			return Err("The request is not complete. Resume it with the same ID.".into());
		}

		record.answer.ok_or_else(|| "The completed request has no saved answer.".into())
	}
}

/// User-facing connection state; connected does not imply a verified Pro turn.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
	/// The user must sign in to ChatGPT in the owned research tab.
	SignInRequired,
	/// The provider requires a human security check; automation must wait.
	VerificationRequired,
	/// A provider challenge or changed page requires the user's attention.
	AttentionRequired,
	/// ChatGPT's signed-in composer is available. Model selection is checked per request.
	Connected,
}

/// Source delivery preference. Receipts contain the resolved Inline or Attachment value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
	/// Use the component's text-size threshold; this is not a model context limit.
	#[default]
	Auto,
	/// Put all selected input in the composer, including acknowledged source parts.
	Inline,
	/// Send a short question with one immutable UTF-8 file.
	Attachment,
}

/// Durable submission state. `Sending` must be reconciled, never replayed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

fn inline_delivery() -> Delivery {
	Delivery::Inline
}
