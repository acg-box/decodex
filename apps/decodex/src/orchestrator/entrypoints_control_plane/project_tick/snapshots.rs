mod context_failure;
mod deferred;
mod live;
mod local;

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) use self::{
	context_failure::control_plane_tick_context_failed_tick,
	deferred::control_plane_project_deferred_snapshot, live::control_plane_project_snapshot,
	local::control_plane_project_local_snapshot,
};
