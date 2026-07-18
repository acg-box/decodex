//! Inert ManagedRun identities, execution assignments, and fail-closed safety algebra.

use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use serde::{Deserialize, Serialize};

use crate::{ProjectId, RuntimeSessionId, TurnId, WorkItemId};

macro_rules! managed_run_id {
	($name:ident, $label:literal, $error:ident) => {
		#[doc = concat!("Canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			#[doc = concat!("Parse one lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl Into<String>) -> Result<Self, ManagedRunError> {
				let value = value.into();
				if !is_canonical_uuid_v4(&value) {
					return Err(ManagedRunError::$error);
				}
				Ok(Self(value))
			}

			/// Borrow the canonical identity text.
			pub fn as_str(&self) -> &str {
				&self.0
			}
		}
		impl Display for $name {
			fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
				formatter.write_str(&self.0)
			}
		}
	};
}

managed_run_id!(ManagedRunId, "ManagedRun", InvalidManagedRunId);
managed_run_id!(EffectId, "ManagedRun effect", InvalidEffectId);
managed_run_id!(SubmittedTurnReceiptId, "submitted-turn receipt", InvalidReceiptId);
managed_run_id!(SafetyObservationId, "safety observation", InvalidObservationId);

/// Closed ManagedRun validation error without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRunError {
	/// ManagedRun identity was not canonical UUID-v4 text.
	InvalidManagedRunId,
	/// Effect identity was not canonical UUID-v4 text.
	InvalidEffectId,
	/// Submitted-turn receipt identity was not canonical UUID-v4 text.
	InvalidReceiptId,
	/// Safety observation identity was not canonical UUID-v4 text.
	InvalidObservationId,
	/// Lifecycle, phase, and wait reason did not form a legal state.
	InvalidState,
	/// An optimistic revision was not positive.
	InvalidRevision,
}
impl Error for ManagedRunError {}
impl Display for ManagedRunError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidManagedRunId => "invalid ManagedRun identity",
			Self::InvalidEffectId => "invalid ManagedRun effect identity",
			Self::InvalidReceiptId => "invalid submitted-turn receipt identity",
			Self::InvalidObservationId => "invalid safety observation identity",
			Self::InvalidState => "invalid ManagedRun lifecycle, phase, and wait combination",
			Self::InvalidRevision => "invalid ManagedRun revision",
		})
	}
}

/// Complete lifecycle vocabulary; persistence in this slice accepts only `waiting`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedRunLifecycle {
	/// Execution has not been acquired.
	Queued,
	/// A future owner is actively advancing the run.
	Active,
	/// Progress is explicitly blocked.
	Waiting,
	/// A future owner ended the run.
	Terminal,
}

/// Closed phase vocabulary independent of lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedRunPhase {
	/// Prepare exact execution inputs.
	Prepare,
	/// Perform implementation work.
	Execute,
	/// Validate owned output.
	Validate,
	/// Obtain independent review.
	Review,
	/// Repair accepted findings.
	Repair,
	/// Land accepted work.
	Land,
	/// Close the execution record.
	Close,
}

/// Typed reason that keeps a ManagedRun inert.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedRunWaitReason {
	/// Usage capacity is unavailable or unproven.
	Usage,
	/// Authentication is unavailable or unproven.
	Auth,
	/// Required plugin readiness is unavailable or unproven.
	Plugin,
	/// A declared dependency prevents progress.
	Dependency,
	/// Required approval is absent.
	Approval,
	/// Explicit user input is required.
	User,
	/// An external authority or readback remains unresolved.
	External,
	/// No independent reviewer is available.
	ReviewerUnavailable,
	/// Independent review failed without accepted completion.
	ReviewerFailed,
}

/// Pure, validated lifecycle algebra.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRunState {
	/// The only legal queued state.
	Queued,
	/// Active state for a non-close phase.
	Active(ManagedRunPhase),
	/// Inert state with an exact phase and typed reason.
	Waiting(ManagedRunPhase, ManagedRunWaitReason),
	/// The only legal terminal state.
	Terminal,
}
impl ManagedRunState {
	/// Validate raw persistence parts and reject every non-canonical combination.
	pub const fn from_parts(
		lifecycle: ManagedRunLifecycle,
		phase: ManagedRunPhase,
		wait_reason: Option<ManagedRunWaitReason>,
	) -> Result<Self, ManagedRunError> {
		match (lifecycle, phase, wait_reason) {
			(ManagedRunLifecycle::Queued, ManagedRunPhase::Prepare, None) => Ok(Self::Queued),
			(ManagedRunLifecycle::Active, active_phase, None)
				if !matches!(active_phase, ManagedRunPhase::Close) =>
				Ok(Self::Active(active_phase)),
			(ManagedRunLifecycle::Waiting, waiting_phase, Some(reason)) =>
				Ok(Self::Waiting(waiting_phase, reason)),
			(ManagedRunLifecycle::Terminal, ManagedRunPhase::Close, None) => Ok(Self::Terminal),
			_ => Err(ManagedRunError::InvalidState),
		}
	}

	/// Return canonical raw parts for persistence or readback.
	pub const fn parts(
		self,
	) -> (ManagedRunLifecycle, ManagedRunPhase, Option<ManagedRunWaitReason>) {
		match self {
			Self::Queued => (ManagedRunLifecycle::Queued, ManagedRunPhase::Prepare, None),
			Self::Active(phase) => (ManagedRunLifecycle::Active, phase, None),
			Self::Waiting(phase, reason) => (ManagedRunLifecycle::Waiting, phase, Some(reason)),
			Self::Terminal => (ManagedRunLifecycle::Terminal, ManagedRunPhase::Close, None),
		}
	}
}

/// Execution-only role that cannot represent Advisor or Lead authority.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionAssignmentRole {
	/// Owning implementation Task for this exact run.
	Task,
	/// Independent Reviewer for this exact run.
	Reviewer,
}

/// Exact-run execution identity; it is not a durable Agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionAssignment {
	/// Owning ManagedRun identity.
	pub managed_run_id: ManagedRunId,
	/// Exact RuntimeSession bound to this assignment.
	pub runtime_session_id: RuntimeSessionId,
	/// Execution-only role.
	pub role: ExecutionAssignmentRole,
}

/// Structural inert ManagedRun readback identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRunIdentity {
	/// Canonical run identity.
	pub managed_run_id: ManagedRunId,
	/// Exact owning Project.
	pub project_id: ProjectId,
	/// Exact canonical WorkItem.
	pub work_item_id: WorkItemId,
	/// Authoritative RuntimeSession for safety transitions.
	pub runtime_session_id: RuntimeSessionId,
	/// Positive optimistic revision.
	pub revision: u64,
}
impl ManagedRunIdentity {
	/// Reject a non-positive stored revision.
	pub const fn validate(&self) -> Result<(), ManagedRunError> {
		if self.revision == 0 { Err(ManagedRunError::InvalidRevision) } else { Ok(()) }
	}
}

/// Monotonic positive or explicitly inconclusive input accepted by the safety transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedRunSafetyInput {
	/// A positively observed exact turn not owned by a matching submitted-turn receipt.
	PositivelyObservedUnknownTurn {
		/// Durable observation identity.
		observation_id: SafetyObservationId,
		/// Exact RuntimeSession observed.
		runtime_session_id: RuntimeSessionId,
		/// Positively observed exact turn identity.
		turn_id: TurnId,
	},
	/// A Decodex-owned submitted-turn receipt consumed without authorizing progress.
	SubmittedTurnReceipt {
		/// Durable receipt identity.
		receipt_id: SubmittedTurnReceiptId,
		/// Exact RuntimeSession recorded by the receipt.
		runtime_session_id: RuntimeSessionId,
		/// Exact submitted turn identity.
		turn_id: TurnId,
	},
	/// Explicitly inconclusive observation; absence is never synthesized into this value.
	InconclusiveObservation {
		/// Durable observation identity.
		observation_id: SafetyObservationId,
		/// Exact RuntimeSession observed.
		runtime_session_id: RuntimeSessionId,
	},
}

fn is_canonical_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();
	if bytes.len() != 36
		|| bytes[8] != b'-'
		|| bytes[13] != b'-'
		|| bytes[18] != b'-'
		|| bytes[23] != b'-'
		|| bytes[14] != b'4'
		|| !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
	{
		return false;
	}
	bytes.iter().enumerate().all(|(index, byte)| {
		matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
	})
}

#[cfg(test)]
mod tests {
	use super::{ManagedRunLifecycle, ManagedRunPhase, ManagedRunState, ManagedRunWaitReason};

	#[test]
	fn state_algebra_accepts_only_canonical_lifecycle_phase_wait_combinations() {
		let lifecycles = [
			ManagedRunLifecycle::Queued,
			ManagedRunLifecycle::Active,
			ManagedRunLifecycle::Waiting,
			ManagedRunLifecycle::Terminal,
		];
		let phases = [
			ManagedRunPhase::Prepare,
			ManagedRunPhase::Execute,
			ManagedRunPhase::Validate,
			ManagedRunPhase::Review,
			ManagedRunPhase::Repair,
			ManagedRunPhase::Land,
			ManagedRunPhase::Close,
		];
		let reasons =
			[None, Some(ManagedRunWaitReason::Usage), Some(ManagedRunWaitReason::ReviewerFailed)];

		for lifecycle in lifecycles {
			for phase in phases {
				for reason in reasons {
					let expected = matches!(
						(lifecycle, phase, reason),
						(ManagedRunLifecycle::Queued, ManagedRunPhase::Prepare, None)
							| (
								ManagedRunLifecycle::Active,
								ManagedRunPhase::Prepare
									| ManagedRunPhase::Execute | ManagedRunPhase::Validate
									| ManagedRunPhase::Review | ManagedRunPhase::Repair
									| ManagedRunPhase::Land,
								None
							) | (ManagedRunLifecycle::Waiting, _, Some(_))
							| (ManagedRunLifecycle::Terminal, ManagedRunPhase::Close, None)
					);
					assert_eq!(
						ManagedRunState::from_parts(lifecycle, phase, reason).is_ok(),
						expected
					);
				}
			}
		}
	}
}
