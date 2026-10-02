//! Account approval configuration observed for one native pending request.
use serde::{Deserialize, Serialize};

/// An explicit edit to one account override. None restores inheritance.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentAppSettingEdit {
	/// Native account approval mode override.
	ApprovalMode(Option<AgentAppApprovalMode>),
	/// Native account reviewer override.
	Reviewer(Option<AgentAppReviewer>),
}

/// Native account-level tool approval modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAppApprovalMode {
	/// Native automatic approval policy.
	Auto,
	/// Prompt for tool approval.
	Prompt,
	/// Native write-sensitive approval policy.
	Writes,
	/// Approve calls without per-call prompts, subject to higher-priority native policy.
	Approve,
}

/// Native approval reviewer selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAppReviewer {
	/// Send approval requests to the user.
	User,
	/// Use native automatic review.
	AutoReview,
}

/// Configuration readback, distinct from effective tool policy or live approval readiness.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentAppSettingsResult {
	/// Native account identity and configuration remain bound to the same task and source.
	Available {
		/// True only while the exact original native request remains pending and no write is
		/// uncertain.
		can_update: bool,
		/// Shared writable configuration file.
		config_file: Box<str>,
		/// Most recent shared edit, including edits made from other tasks or hook settings.
		last_edit: Option<Box<AgentConfigEditReceipt>>,
		/// Opaque native connector identity.
		connector_id: String,
		/// Opaque native connected account identity.
		link_id: String,
		/// Source-bound identity of the exact configuration shown for review.
		review_token: String,
		/// Account mode after config layering, before tool and managed-policy precedence.
		effective_mode: Option<String>,
		/// Account reviewer after config layering, before managed-policy precedence.
		effective_reviewer: Option<String>,
		/// Account mode stored in the writable user layer; None inherits.
		user_mode: Option<String>,
		/// Account reviewer stored in the writable user layer; None inherits.
		user_reviewer: Option<String>,
	},
	/// No current native request, verified source, or readable configuration.
	Unavailable,
}

/// Durable shared config result; it does not assert effective tool policy.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentConfigEditReceipt {
	/// Reserved, saved, overridden, rejected, unknown, target_observed or superseded.
	pub outcome: String,
	/// App connection or hook identity for display.
	pub target: String,
	/// Original local task.
	pub work_id: String,
	/// Original Codex account.
	pub account_id: String,
	/// Native acknowledgement version, if the app write was acknowledged.
	pub saved_version: Option<String>,
}

/// One native connection with an explicit saved override.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentSavedAppConnection {
	/// Exact native app key.
	pub connector_id: String,
	/// Exact native connection key, without claiming current cloud connectivity.
	pub link_id: String,
	/// Source and native configuration reviewed for one edit.
	pub review_token: String,
	/// Saved approval mode; None inherits.
	pub user_mode: Option<String>,
	/// Saved reviewer; None inherits.
	pub user_reviewer: Option<String>,
	/// Merged link mode before managed and tool policy.
	pub effective_mode: Option<String>,
	/// Merged link reviewer before managed policy.
	pub effective_reviewer: Option<String>,
}
/// Native saved overrides, independently of pending tool requests.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentSavedAppSettingsResult {
	/// Complete saved configuration for the current task's native source.
	Available {
		/// Originating local task.
		work_id: String,
		/// Native thread whose directory selected this configuration.
		thread_id: String,
		/// Shared writable configuration file.
		config_file: String,
		/// Saved connections; empty means there are no explicit overrides.
		connections: Vec<AgentSavedAppConnection>,
		/// False while a shared edit remains unresolved.
		can_update: bool,
		/// Latest app or hook edit to this file.
		last_edit: Option<Box<AgentConfigEditReceipt>>,
	},
	/// No current source or complete readable configuration.
	Unavailable,
}

#[cfg(test)]
mod tests {
	use crate::AgentAppSettingEdit;

	#[test]
	fn setting_edits_preserve_inheritance_and_reject_unoffered_fields_and_values() {
		for wire in [
			serde_json::json!({"field":"approval_mode","value":"prompt"}),
			serde_json::json!({"field":"approval_mode","value":null}),
			serde_json::json!({"field":"reviewer","value":"auto_review"}),
			serde_json::json!({"field":"reviewer","value":null}),
		] {
			let edit: AgentAppSettingEdit = serde_json::from_value(wire.clone()).unwrap();

			assert_eq!(serde_json::to_value(edit).unwrap(), wire);
		}
		for wire in [
			serde_json::json!({"field":"model","value":"other"}),
			serde_json::json!({"field":"reviewer","value":"future"}),
			serde_json::json!({"field":"approval_mode","value":true}),
			serde_json::json!({"field":"reviewer","value":"user","extra":"unreviewed"}),
		] {
			assert!(serde_json::from_value::<AgentAppSettingEdit>(wire).is_err());
		}
	}
}
