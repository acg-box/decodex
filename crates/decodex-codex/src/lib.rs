//! Codex app-server adapter boundary.
//!
//! XY-1265 preserves the shared normal `~/.codex` contract without spawning a runner.

use decodex_core::{Availability, ConversationRuntime};

/// Stable unavailable reason while Codex process supervision remains outside XY-1265.
pub const NOT_IMPLEMENTED: &str = "Codex adapter is unavailable until XY-1270";

/// Continuation-home policy selected by this infrastructure owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexContinuity {
	/// The user's ordinary shared `~/.codex`, never a per-run home.
	SharedNormalHome,
}

/// The sole Codex adapter selected by the vNext composition root.
#[derive(Clone, Copy, Debug, Default)]
pub struct CodexAdapter;
impl CodexAdapter {
	/// Construct the explicit XY-1265 unavailable adapter.
	pub const fn unavailable() -> Self {
		Self
	}

	/// Report the continuation policy owned by this adapter.
	pub const fn continuity(self) -> CodexContinuity {
		CodexContinuity::SharedNormalHome
	}
}

impl ConversationRuntime for CodexAdapter {
	fn availability(&self) -> Availability {
		Availability::Unavailable { reason: NOT_IMPLEMENTED }
	}
}

#[cfg(test)]
mod tests {
	use crate::{CodexAdapter, CodexContinuity, NOT_IMPLEMENTED};
	use decodex_core::{Availability, ConversationRuntime};

	#[test]
	fn adapter_preserves_shared_home_and_is_explicitly_unavailable() {
		let adapter = CodexAdapter::unavailable();

		assert_eq!(adapter.continuity(), CodexContinuity::SharedNormalHome);
		assert_eq!(adapter.availability(), Availability::Unavailable { reason: NOT_IMPLEMENTED });
	}
}
