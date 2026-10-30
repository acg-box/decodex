//! User-facing presets over native permission settings, not a second policy engine.
use serde::{Deserialize, Serialize};

/// Common native permission configurations shown in the desktop menu.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
	#[default]
	#[serde(alias = "workspace_write")]
	/// Work within the workspace and ask before escalation.
	AskForApproval,
	/// Work within the workspace and route escalation to native review.
	ApproveForMe,
	/// Remove the local execution sandbox.
	FullAccess,
	/// Inspect files and ask before edits.
	ReadOnly,
}
impl PermissionMode {
	/// Menu order, independent of the native profile catalog.
	pub const ALL: [Self; 4] =
		[Self::AskForApproval, Self::ApproveForMe, Self::FullAccess, Self::ReadOnly];

	/// Stable user-facing name.
	pub const fn label(self) -> &'static str {
		match self {
			Self::AskForApproval => "Ask for approval",
			Self::ApproveForMe => "Approve for me",
			Self::FullAccess => "Full access",
			Self::ReadOnly => "Read-only",
		}
	}

	/// Short description of the preset's boundary.
	pub const fn description(self) -> &'static str {
		match self {
			Self::AskForApproval => "External edits and restricted network.",
			Self::ApproveForMe => "Codex reviews restricted actions.",
			Self::FullAccess => "Unrestricted file and network access.",
			Self::ReadOnly => "Read files; ask before changes.",
		}
	}

	/// Corresponding built-in native permission profile.
	pub const fn profile(self) -> &'static str {
		match self {
			Self::AskForApproval | Self::ApproveForMe => ":workspace",
			Self::FullAccess => ":danger-full-access",
			Self::ReadOnly => ":read-only",
		}
	}

	/// Corresponding native creation sandbox.
	pub const fn sandbox(self) -> &'static str {
		match self {
			Self::AskForApproval | Self::ApproveForMe => "workspace-write",
			Self::FullAccess => "danger-full-access",
			Self::ReadOnly => "read-only",
		}
	}

	/// Native approval policy for the preset.
	pub const fn approval_policy(self) -> &'static str {
		match self {
			Self::FullAccess => "never",
			_ => "on-request",
		}
	}

	/// Native approval reviewer for the preset.
	pub const fn reviewer(self) -> &'static str {
		match self {
			Self::ApproveForMe => "auto_review",
			_ => "user",
		}
	}
}
