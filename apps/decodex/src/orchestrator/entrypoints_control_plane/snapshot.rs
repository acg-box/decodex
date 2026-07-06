mod aggregate;
mod disabled;
mod local;
mod project_status;

pub(crate) use self::{
	aggregate::{append_control_plane_project_snapshot, collect_control_plane_snapshot},
	disabled::control_plane_disabled_project_observer_tick,
	local::build_operator_state_snapshot_without_live_observers,
	project_status::{
		complete_project_status, hydrate_project_status_from_local_snapshot,
		hydrate_project_status_from_registered_status,
	},
};

use crate::orchestrator::{
	self, AgentEvidenceSource, OperatorProjectStatus, OperatorStatusSnapshot,
};

pub(crate) struct ControlPlaneProjectTick {
	pub(crate) snapshot: Option<OperatorStatusSnapshot>,
	pub(crate) project_status: Option<OperatorProjectStatus>,
}

pub(crate) fn write_snapshot_evidence(snapshot: &OperatorStatusSnapshot) {
	orchestrator::write_agent_evidence_best_effort(snapshot, AgentEvidenceSource::ServeTick);
}
