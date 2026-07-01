use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum ThreadGoalStatus {
	Active,
	Paused,
	Blocked,
	UsageLimited,
	BudgetLimited,
	Complete,
}
impl ThreadGoalStatus {
	pub(in crate::agent::app_server) const fn as_str(self) -> &'static str {
		match self {
			Self::Active => "active",
			Self::Paused => "paused",
			Self::Blocked => "blocked",
			Self::UsageLimited => "usageLimited",
			Self::BudgetLimited => "budgetLimited",
			Self::Complete => "complete",
		}
	}
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum CommandExecutionApprovalDecision {
	Decline,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum FileChangeApprovalDecision {
	Decline,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum McpServerElicitationAction {
	Decline,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) enum PermissionGrantScope {
	#[default]
	Turn,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(in crate::agent::app_server) enum UserInput {
	#[serde(rename = "text")]
	Text { text: String },
}
