/// Production composition gate that remains closed for every app-server mutation.
pub const LIVE_ROUTING_GATE: &str = "production-app-server-io-unavailable";
/// Later acceptance gate for automatic cross-account fallback and all-depleted wake.
pub const AUTOMATIC_FALLBACK_WAKE_GATE: &str = "XY-1304";

/// Closed dispatch-path classification. This is descriptive and grants no I/O authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchPath {
	/// Initial account selection or a new task after explicit manual recovery.
	OrdinaryConversation,
	/// Automatic continuation of one conversation on another account.
	AutomaticCrossAccountFallback,
	/// Automatic retry after every eligible account was depleted.
	AllDepletedWake,
}
impl DispatchPath {
	/// Report whether this path remains gated by XY-1304.
	pub const fn requires_xy_1304(self) -> bool {
		matches!(self, Self::AutomaticCrossAccountFallback | Self::AllDepletedWake)
	}
}

/// App-server mutations known to the adapter. None are wired to production I/O here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchOperation {
	/// Create a Codex thread.
	ThreadStart,
	/// Resume a Codex thread under a process.
	ThreadResume,
	/// Start model execution for a turn.
	TurnStart,
	/// Add input to an active turn.
	TurnSteer,
	/// Interrupt an active turn.
	TurnInterrupt,
	/// Explicitly archive an exact thread.
	ThreadArchive,
	/// Reply to an app-server approval request.
	ApprovalResponse,
}

/// Typed fail-closed dispatch denial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchDenied {
	/// Operation denied before any production app-server I/O.
	pub operation: DispatchOperation,
	/// Production composition gate that remains failed.
	pub failed_gate: &'static str,
}

/// Zero-config production I/O guard. No enabled constructor exists in this crate.
#[derive(Clone, Copy, Debug, Default)]
pub struct DispatchGate;
impl DispatchGate {
	/// Construct the current uncomposed production I/O state.
	pub const fn production_io_unavailable() -> Self {
		Self
	}

	/// Deny production I/O without treating XY-1304 as an ordinary Conversation prerequisite.
	pub const fn authorize(self, operation: DispatchOperation) -> Result<(), DispatchDenied> {
		Err(DispatchDenied { operation, failed_gate: LIVE_ROUTING_GATE })
	}
}

#[cfg(test)]
mod tests {
	use crate::{
		AUTOMATIC_FALLBACK_WAKE_GATE, DispatchDenied, DispatchGate, DispatchPath,
		LIVE_ROUTING_GATE, dispatch::DispatchOperation,
	};

	#[test]
	fn dispatch_stays_closed_and_xy_1304_applies_only_to_fallback_and_wake() {
		for operation in [
			DispatchOperation::ThreadStart,
			DispatchOperation::ThreadResume,
			DispatchOperation::TurnStart,
			DispatchOperation::TurnSteer,
			DispatchOperation::TurnInterrupt,
			DispatchOperation::ThreadArchive,
			DispatchOperation::ApprovalResponse,
		] {
			assert_eq!(
				DispatchGate.authorize(operation),
				Err(DispatchDenied { operation, failed_gate: LIVE_ROUTING_GATE })
			);
		}

		assert_eq!(AUTOMATIC_FALLBACK_WAKE_GATE, "XY-1304");
		for (path, expected) in [
			(DispatchPath::OrdinaryConversation, false),
			(DispatchPath::AutomaticCrossAccountFallback, true),
			(DispatchPath::AllDepletedWake, true),
		] {
			assert_eq!(path.requires_xy_1304(), expected);
		}
	}
}
