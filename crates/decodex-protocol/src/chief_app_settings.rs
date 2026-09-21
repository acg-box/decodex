//! Account approval configuration observed for one native pending request.
use serde::{Deserialize, Serialize};

/// An explicit edit to one account override. None restores inheritance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefAppSettingEdit {
	/// Native account approval mode override.
	ApprovalMode(Option<ChiefAppApprovalMode>),
	/// Native account reviewer override.
	Reviewer(Option<ChiefAppReviewer>),
}

/// Native account-level tool approval modes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefAppApprovalMode {
	/// Native automatic approval policy.
	Auto,
	/// Prompt for tool approval.
	Prompt,
	/// Native write-sensitive approval policy.
	Writes,
	/// Native explicit approval policy.
	Approve,
}

/// Native approval reviewer selection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefAppReviewer {
	/// Send approval requests to the user.
	User,
	/// Use native automatic review.
	AutoReview,
}

impl ChiefAppSettingEdit {
	/// Exact native leaf name and value; None removes this override only.
	pub fn native_value(&self) -> (&'static str, Option<&'static str>) {
		match self {
			Self::ApprovalMode(mode) => (
				"default_tools_approval_mode",
				mode.map(|mode| match mode {
					ChiefAppApprovalMode::Auto => "auto",
					ChiefAppApprovalMode::Prompt => "prompt",
					ChiefAppApprovalMode::Writes => "writes",
					ChiefAppApprovalMode::Approve => "approve",
				}),
			),
			Self::Reviewer(reviewer) => (
				"approvals_reviewer",
				reviewer.map(|reviewer| match reviewer {
					ChiefAppReviewer::User => "user",
					ChiefAppReviewer::AutoReview => "auto_review",
				}),
			),
		}
	}
}

/// Configuration readback, distinct from effective tool policy or live approval readiness.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefAppSettingsResult {
	/// Native account identity and configuration remained bound to the same request and source.
	Available {
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

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	#[test]
	fn setting_edits_preserve_inheritance_and_reject_unoffered_fields_and_values() {
		for (wire, expected) in [
			(
				json!({"field":"approval_mode","value":"prompt"}),
				("default_tools_approval_mode", Some("prompt")),
			),
			(json!({"field":"approval_mode","value":null}), ("default_tools_approval_mode", None)),
			(
				json!({"field":"reviewer","value":"auto_review"}),
				("approvals_reviewer", Some("auto_review")),
			),
			(json!({"field":"reviewer","value":null}), ("approvals_reviewer", None)),
		] {
			let edit: ChiefAppSettingEdit = serde_json::from_value(wire.clone()).unwrap();
			assert_eq!(edit.native_value(), expected);
			assert_eq!(serde_json::to_value(edit).unwrap(), wire);
		}
		for wire in [
			json!({"field":"model","value":"other"}),
			json!({"field":"reviewer","value":"future"}),
			json!({"field":"approval_mode","value":true}),
			json!({"field":"reviewer","value":"user","extra":"unreviewed"}),
		] {
			assert!(serde_json::from_value::<ChiefAppSettingEdit>(wire).is_err());
		}
	}
}
