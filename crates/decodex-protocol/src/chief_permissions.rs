//! Named native permission profiles for an exact saved task.
use crate::{EntityId, WireText};
use serde::{Deserialize, Serialize};

/// Native profile eligibility in the task's current directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefPermissionProfile {
	/// Native profile identifier.
	pub id: WireText,
	/// Native policy permits selection.
	pub allowed: bool,
	/// Policy and current task state permit this selection.
	pub can_select: bool,
	/// Native description, if supplied.
	pub description: Option<WireText>,
}
/// Durable outcome, separate from the effective native policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefPermissionOutcome {
	/// Reserved before the native write.
	Reserved,
	/// Native accepted the queue request; application is not confirmed.
	Queued,
	/// Delivery is uncertain and must not be replayed.
	Unknown,
	/// Native or source checks rejected the operation.
	Rejected,
	/// Later current native facts show the target profile.
	TargetObserved,
	/// A new native owner reports different permissions after the old process ended.
	Superseded,
}
/// Read-only review or pending operation for a saved task.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefPermissionState {
	/// Current native facts and the complete bounded profile catalog.
	Available {
		/// Exact local task.
		work_id: EntityId,
		/// Exact native thread.
		thread_id: EntityId,
		/// Opaque source, catalog and receipt identity.
		review_token: WireText,
		/// Native directory used to resolve profiles.
		cwd: WireText,
		/// Current profile; absent for unnamed native policy.
		profile_id: Option<WireText>,
		/// Native reviewer, independent of profile identity.
		approvals_reviewer: WireText,
		/// Native catalog, including profiles prohibited by policy.
		profiles: Vec<ChiefPermissionProfile>,
		/// The task state permits permission selection; each profile has its own eligibility.
		can_update: bool,
		/// Most recent settled local attempt, if any.
		last_outcome: Option<ChiefPermissionOutcome>,
	},
	/// An unresolved operation blocks another selection and task dispatch.
	Pending {
		/// Requested native profile; this is not an effective-policy claim.
		profile_id: WireText,
		/// Unresolved durable state.
		state: ChiefPermissionOutcome,
	},
	/// Installed native service does not support profile discovery.
	Unsupported,
	/// Current exact-source facts cannot be established.
	Unavailable,
}
