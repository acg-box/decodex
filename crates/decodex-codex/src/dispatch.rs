/// Repository authority gate that owns all live routing enablement.
pub const LIVE_ROUTING_GATE: &str = "XY-1304";

/// Every app-server operation that can mutate or advance live execution.
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
	/// Reply to an app-server approval request.
	ApprovalResponse,
}

/// Typed fail-closed dispatch denial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchDenied {
	/// Operation denied before protocol construction.
	pub operation: DispatchOperation,
	/// Repository gate that remains failed.
	pub failed_gate: &'static str,
}

/// Zero-config guard. No enabled constructor exists in this gate.
#[derive(Clone, Copy, Debug, Default)]
pub struct DispatchGate;
impl DispatchGate {
	/// Construct the only XY-1270 dispatch state.
	pub const fn failed_xy_1304() -> Self {
		Self
	}

	/// Deny every live operation before a protocol request can be constructed.
	pub const fn authorize(self, operation: DispatchOperation) -> Result<(), DispatchDenied> {
		Err(DispatchDenied { operation, failed_gate: LIVE_ROUTING_GATE })
	}
}

#[cfg(test)]
mod tests {
	use crate::{DispatchDenied, DispatchGate, LIVE_ROUTING_GATE, dispatch::DispatchOperation};

	#[test]
	fn default_guard_denies_every_live_dispatch_operation() {
		for operation in [
			DispatchOperation::ThreadStart,
			DispatchOperation::ThreadResume,
			DispatchOperation::TurnStart,
			DispatchOperation::TurnSteer,
			DispatchOperation::TurnInterrupt,
			DispatchOperation::ApprovalResponse,
		] {
			assert_eq!(
				DispatchGate.authorize(operation),
				Err(DispatchDenied { operation, failed_gate: LIVE_ROUTING_GATE })
			);
		}
	}
}
