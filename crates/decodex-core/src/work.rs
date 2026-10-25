//! Project Work and Collaboration contracts. Native execution has a separate lifecycle.
use serde::{Deserialize, Serialize};

/// A user-created persistent collaborator with one current native conversation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
	/// Stable local identity.
	pub id: String,
	/// User-visible name, shared across its Works.
	pub name: String,
	/// Current native identity, absent before first execution.
	pub native_thread_id: Option<String>,
	/// Optional personal-assistance responsibility. This grants no other Agent's history.
	pub assistant: bool,
}

/// Business state of an outcome, independent of a native turn.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
	/// The outcome still needs work or assessment.
	#[default]
	Open,
	/// A named condition or explicit hold prevents continuation.
	Blocked,
	/// The owner assessed the current requirements and evidence.
	Completed,
	/// The authorized scope ended without completion.
	Cancelled,
}

/// One authoritative, immutable requirements revision.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkRequirements {
	/// Stable revision identity.
	pub id: String,
	/// Outcome to deliver.
	pub goal: String,
	/// Conditions that establish completion.
	pub criteria: String,
	/// Exact decision or authorized input reference; never a synthesized user message.
	pub source: String,
}

/// One durable outcome and its accountable Agent.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
	/// Stable business identity, never a native thread identity.
	pub id: String,
	/// Continuing context, if the user selected one.
	pub project_id: Option<String>,
	/// Sole accountable persistent Agent.
	pub owner_agent_id: String,
	/// Concise outcome name.
	pub title: String,
	/// Current immutable requirements.
	pub requirements: WorkRequirements,
	/// Current business state.
	pub state: WorkState,
	/// Concrete blocker or cancellation reason.
	pub reason: Option<String>,
	/// Exact dependency, decision or explicit-release condition.
	pub resume_condition: Option<String>,
	/// A user hold requires explicit user release.
	pub user_hold: bool,
	/// Current completed assessment, if any.
	pub assessment: Option<WorkAssessment>,
}

/// Input that can be shared only through an explicit scoped reference.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
	/// Stable identity within its Work.
	pub id: String,
	/// Work that owns the reference.
	pub work_id: String,
	/// Human-readable purpose.
	pub title: String,
	/// Explicit file, URL or immutable decision reference. Does not grant full chat access.
	pub reference: String,
	/// Required for a mutable artifact; absent for immutable decision references.
	pub sha256: Option<String>,
}

/// Exact evidence identity; the service verifies source inclusion before saving it.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultSource {
	/// Exact public native output, including a genuine native Subagent output.
	NativeItem {
		/// Native conversation that produced the item.
		thread_id: String,
		/// Exact producing turn.
		turn_id: String,
		/// Exact public item.
		item_id: String,
	},
	/// An exact version of a file within the authorized Project directory.
	Artifact {
		/// Canonical local file path.
		path: String,
		/// SHA-256 of its actual bytes.
		sha256: String,
	},
}

/// A contribution returned against a specific instruction and requirements revision.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkResult {
	/// Stable identity; duplicate delivery retains the original.
	pub id: String,
	/// Outcome this evidence supports.
	pub work_id: String,
	/// Requirements used during execution, never silently rewritten.
	pub requirements_id: String,
	/// Exact contributor instruction, absent for a self-produced Result.
	pub instruction_id: Option<String>,
	/// Exact native Subagent contribution, absent for self-produced or peer Results.
	#[serde(default)]
	pub native_contribution_id: Option<String>,
	/// Persistent Agent accountable for the contribution.
	pub agent_id: String,
	/// Outcome actually produced.
	pub summary: String,
	/// Exact evidence, not the Agent's latest reply.
	pub source: ResultSource,
	/// Unmeasured or incomplete parts.
	pub limits: String,
}

/// The Work owner's current assessment.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkAssessment {
	/// Requirements being assessed.
	pub requirements_id: String,
	/// Exact Results, including explicitly rechecked older Results.
	pub result_ids: Vec<String>,
	/// What was delivered and why it meets the criteria.
	pub summary: String,
	/// Explicit retained qualification limits.
	pub limits: String,
}

/// Independent Agent contribution state. Delivery is recorded separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationState {
	/// Saved or queued; native admission does not imply acceptance.
	Pending,
	/// The named recipient explicitly accepted the current instruction.
	Accepted,
	/// Exact Results await the requesting owner's assessment.
	Returned,
	/// The owner assessed or cancelled the contribution, or the recipient declined.
	Closed,
}

/// One immutable contribution instruction; a repair appends another one.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationInstruction {
	/// Stable instruction identity, reused for retries.
	pub id: String,
	/// Work requirements current when this instruction was issued.
	pub requirements_id: String,
	/// Actual bounded request sent through application-origin input.
	pub text: String,
	/// Only these Material references can be shared with the recipient.
	pub material_ids: Vec<String>,
	/// Exact retained inbox event, used for transport recovery.
	pub event_id: Option<i64>,
	/// Acknowledged native receiving turn; absence is not proof of non-execution.
	pub delivered_turn_id: Option<String>,
}

/// A scoped exchange between existing independent Agents. It never changes Work ownership.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationRequest {
	/// Stable request identity across repair cycles.
	pub id: String,
	/// Requesting outcome.
	pub work_id: String,
	/// Independent existing recipient.
	pub recipient_agent_id: String,
	/// Current contribution cycle.
	pub instruction: CollaborationInstruction,
	/// Business acceptance and return state.
	pub state: CollaborationState,
	/// Whether completion requires this contribution to be handled.
	pub required: bool,
	/// Final used, not_used, declined, cancelled or failed disposition.
	pub outcome: Option<String>,
	/// Current disposition or conflict explanation.
	pub note: Option<String>,
}

/// Attribution of a native helper. Native Codex remains the only executor and mailbox.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeContribution {
	/// Stable intent identity, saved before native spawn.
	pub id: String,
	/// Accountable outcome.
	pub work_id: String,
	/// Requirements actually assigned.
	pub requirements_id: String,
	/// Bounded assignment text, excluding the service-generated attribution marker.
	pub instruction: String,
	/// Exact native child after positive binding; absent means binding is unconfirmed.
	pub child_thread_id: Option<String>,
	/// Exact parent native turn with the verified collaboration or child activity.
	pub parent_turn_id: Option<String>,
	/// Exact native collaboration or child activity item. V2 instruction proof is the child input.
	pub parent_item_id: Option<String>,
}

/// Exact native input observed before its parent activity is necessarily available.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct NativeWorkDelivery {
	/// Native owner conversation, verified from the child's source.
	pub parent_thread_id: String,
	/// Native root turn that issued this instruction.
	pub parent_turn_id: String,
	/// Genuine native child that received the instruction.
	pub child_thread_id: String,
	/// Exact receiving turn; other turns cannot supply this contribution's Results.
	pub child_turn_id: String,
	/// Exact inbound native message, independent of a later parent activity.
	pub message_id: String,
}
impl NativeContribution {
	/// Exact prompt to pass to the native helper. A missing binding never authorizes duplicate
	/// spawn.
	pub fn native_prompt(&self) -> String {
		format!(
			"[Work: {}; Requirements: {}; Contribution: {}]\n{}",
			self.work_id, self.requirements_id, self.id, self.instruction
		)
	}
}

/// A bounded detail view. Older evidence is retained in SQLite and can be queried per Work.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkPortfolio {
	/// Open Works with current returned evidence that still needs an owner assessment.
	#[serde(default)]
	pub awaiting_assessment: Vec<String>,
	/// This view contains Work headers only. Read the selected Work before using its evidence.
	#[serde(default)]
	pub summary_only: bool,
	/// Saved native execution attribution; unbound intent requires exact history reconciliation.
	#[serde(default)]
	pub native_contributions: Vec<NativeContribution>,
	/// Selected current outcomes.
	pub works: Vec<Work>,
	/// Selected scoped exchanges.
	pub collaborations: Vec<CollaborationRequest>,
	/// Exact Results; a returned Result does not establish completion.
	pub results: Vec<WorkResult>,
	/// Explicitly authorized Materials.
	pub materials: Vec<Material>,
	/// At least one collection has more retained entries.
	pub has_more: bool,
}

/// Authorized business mutations. Identities make retries idempotent; expected revisions fence
/// races.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkCommand {
	/// Save attribution intent before using native spawn or follow-up; creates no executor.
	PrepareSubagent {
		/// Native contribution intent, saved before dispatch.
		contribution: NativeContribution,
	},
	/// Bind an actual native collaboration event and verified child, without spawning or messaging.
	BindSubagent {
		/// Exact saved native contribution identity.
		contribution_id: String,
		/// Exact parent turn that issued the native instruction.
		parent_turn_id: String,
		/// Exact native collaboration item.
		parent_item_id: String,
		/// Verified native child identity.
		child_thread_id: String,
	},
	/// Create an outcome; does not create an executor or dispatch a turn.
	Create {
		/// New open outcome and its requirements.
		work: Work,
	},
	/// Change only this Work's requirements and retain all prior instructions and Results.
	Revise {
		/// Exact existing Work identity.
		work_id: String,
		/// Last read current requirements identity; a mismatch rejects the change.
		expected_requirements: String,
		/// New immutable authorized requirements revision.
		requirements: WorkRequirements,
	},
	/// Set a blocker, cancel, or resume an authorized current Work.
	SetState {
		/// Exact existing Work identity.
		work_id: String,
		/// Last read current requirements identity; a mismatch rejects the change.
		expected_requirements: String,
		/// Requested business state; completion uses an assessment.
		state: WorkState,
		/// Concrete blocker, hold or cancellation explanation.
		reason: Option<String>,
		/// Condition or explicit user release needed to continue.
		resume_condition: Option<String>,
		/// An explicit user hold, which only the user can release.
		user_hold: bool,
	},
	/// Add one authorized reference; does not grant whole-conversation access.
	AddMaterial {
		/// Exact authorized Material reference.
		material: Material,
	},
	/// Return exact verified evidence. A recipient can accept and return in the same operation.
	RecordResult {
		/// Exact verified Result and source identity.
		result: WorkResult,
	},
	/// Explicitly recheck older evidence against current requirements, preserving its source
	/// revision.
	ReuseResult {
		/// Exact existing Work identity.
		work_id: String,
		/// Last read current requirements identity; a mismatch rejects the change.
		expected_requirements: String,
		/// Retained Result selected for explicit reuse.
		result_id: String,
		/// Owner judgment and its supporting evidence or reuse explanation.
		assessment: String,
	},
	/// Complete with an atomic current-owner assessment.
	Complete {
		/// Exact existing Work identity.
		work_id: String,
		/// Last read current requirements identity; a mismatch rejects the change.
		expected_requirements: String,
		/// Owner judgment and its supporting evidence or reuse explanation.
		assessment: WorkAssessment,
	},
	/// Send a scoped request to an existing Agent. A busy recipient remains Pending.
	Request {
		/// New scoped request targeting an existing independent Agent.
		request: CollaborationRequest,
	},
	/// Explicit recipient acceptance or decline of the current instruction.
	Respond {
		/// Exact existing Collaboration Request identity.
		request_id: String,
		/// Exact current contribution instruction.
		instruction_id: String,
		/// Whether the actual recipient accepts the request.
		accept: bool,
		/// Acceptance, conflict or assessment explanation.
		note: String,
	},
	/// Request repair in the same logical exchange, preserving previous Results.
	Repair {
		/// Exact existing Collaboration Request identity.
		request_id: String,
		/// Last read current instruction identity.
		expected_instruction: String,
		/// New immutable repair instruction in the same logical request.
		instruction: CollaborationInstruction,
	},
	/// Assess, cancel or mark a returned contribution not used.
	CloseRequest {
		/// Exact existing Collaboration Request identity.
		request_id: String,
		/// Exact current contribution instruction.
		instruction_id: String,
		/// Explicit contribution disposition; this does not close the Work.
		outcome: String,
		/// Acceptance, conflict or assessment explanation.
		note: String,
	},
}
