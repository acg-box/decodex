//! Continuing personal assistance. Execution remains in native Codex conversations.
use serde::{Deserialize, Serialize};

/// User-selected settings for the personal Agent.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantSettings {
	/// Current user mandate, rebuilt directly in each native context.
	pub mandate: String,
	/// Completed turns per native context. Zero disables periodic renewal.
	pub renew_after_turns: u32,
	/// Optional existing RAGFlow instance. No installation is implied.
	pub ragflow: Option<RagflowConnection>,
}
impl Default for AssistantSettings {
	fn default() -> Self {
		Self { mandate: String::new(), renew_after_turns: 24, ragflow: None }
	}
}

/// Explicit scopes and a process-environment credential reference, never a saved API key.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RagflowConnection {
	/// Origin of an existing instance, for example https://ragflow.example.com.
	pub url: String,
	/// Service environment variable that contains this instance's API key.
	pub api_key_env: String,
	/// Exact user namespace. Required by the API-key Memory API.
	pub user_id: String,
	/// Explicit datasets. An empty list disables knowledge retrieval.
	pub dataset_ids: Vec<String>,
	/// One dedicated Memory, or None to keep native memory behavior.
	pub memory_id: Option<String>,
	/// Export completed public user/assistant exchanges after this opt-in.
	pub ingest_chats: bool,
}

/// A lightweight group of existing persistent Agents.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTeam {
	/// Stable identity within the Assistant.
	pub id: String,
	/// User-visible name.
	pub name: String,
	/// Optional continuing Project context.
	pub project_id: Option<String>,
	/// Responsibilities do not grant access to private chats or change Work ownership.
	pub members: Vec<TeamMember>,
}

/// Membership can be shared by several teams.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMember {
	/// Existing persistent Agent identity.
	pub agent_id: String,
	/// Concise responsibility in this team.
	pub responsibility: String,
}

/// One independent user-delegated wake. It uses the host's existing due-check tick.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantWake {
	/// Stable identity within this Assistant.
	pub id: String,
	/// What to inspect or continue. The Agent decides whether a notification is useful.
	pub instruction: String,
	/// Original delivered user decision.
	pub authority_event_id: i64,
	/// Next Unix timestamp in microseconds, or None after completion/cancellation.
	pub due_at_micros: Option<i64>,
	/// Optional recurring interval in seconds. Missed occurrences coalesce into one wake.
	pub interval_seconds: Option<u32>,
}

/// Mutations use stable command identities in the existing Inbox journal.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssistantCommand {
	/// Direct user settings. External export and Memory require this explicit selection.
	Configure {
		/// User-selected connection and lifecycle policy.
		settings: AssistantSettings,
	},
	/// Save the current mandate from an exact delivered user decision.
	SetMandate {
		/// Continuing user instructions.
		text: String,
		/// Exact delivered user decision.
		authority_event_id: i64,
	},
	/// Create or replace a team. Reuse existing member Agents.
	PutTeam {
		/// Current membership and responsibilities.
		team: AgentTeam,
	},
	/// Remove membership only; Work and native conversations continue.
	RemoveTeam {
		/// Existing team identity.
		team_id: String,
	},
	/// Create an independent persistent Agent through the normal service admission path.
	CreateAgent {
		/// New stable persistent Agent identity.
		agent_id: String,
		/// Conversation name.
		name: String,
		/// Initial continuing responsibility.
		instructions: String,
		/// Existing Project, if selected.
		project_id: Option<String>,
		/// Exact delivered user delegation.
		authority_event_id: i64,
	},
	/// Register an independent wake, without replacing other duties.
	Schedule {
		/// One independent duty.
		wake: AssistantWake,
	},
	/// Cancel this wake only. Already delivered instructions keep their original authority.
	CancelWake {
		/// Existing duty identity.
		wake_id: String,
	},
	/// Request a fresh context at the next safe boundary.
	RefreshContext,
}

/// Readable state. Original source records retain their execution authority.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct AssistantState {
	/// Selected persistent Agent, if any.
	pub agent_id: Option<String>,
	/// Current user-selected settings.
	pub settings: AssistantSettings,
	/// Current lightweight teams.
	pub teams: Vec<AgentTeam>,
	/// Independent continuing duties.
	pub wakes: Vec<AssistantWake>,
	/// Previous native contexts, newest first. They are read-only history.
	pub previous_thread_ids: Vec<String>,
	/// An unknown native creation must be reconciled instead of repeated.
	pub context_pending: bool,
	/// Last external import failure or asynchronous acceptance, without credential details.
	#[serde(default)]
	pub memory_status: Option<String>,
}
