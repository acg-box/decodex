use serde::Deserialize;

/// Review level for agent runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ReviewLevel {
	/// Disable review gates.
	Off,
	/// Require implementation self-check only.
	Basic,
	/// Require self-check plus the Decodex Review checkpoint gate.
	Standard,
	/// Require standard review plus the GitHub Review path.
	#[default]
	Strict,
}
impl ReviewLevel {
	/// Config string for this level.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Off => "off",
			Self::Basic => "basic",
			Self::Standard => "standard",
			Self::Strict => "strict",
		}
	}

	/// Whether this level prompts the implementation self-check.
	pub const fn uses_self_check(self) -> bool {
		!matches!(self, Self::Off)
	}

	/// Whether this level uses the structured Decodex Review checkpoint gate.
	pub const fn requires_review_checkpoint(self) -> bool {
		matches!(self, Self::Standard | Self::Strict)
	}

	/// Whether this level uses the GitHub `@codex review` path.
	pub const fn uses_github_review(self) -> bool {
		matches!(self, Self::Strict)
	}
}
