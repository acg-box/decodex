use super::*;

mod autonomy_lineage;
mod ghost_lane;
mod lifecycle;
mod liveness;
mod recovery_lineage;

use lifecycle::{
	assert_terminal_pending_interrupt_rejects_force, assert_terminal_pending_lane_inspect,
	assert_terminal_pending_status_projection,
};

#[derive(Clone, Copy)]
struct ReviewCheckpointSeed<'a> {
	issue_id: &'a str,
	run_id: &'a str,
	phase: &'a str,
	status: &'a str,
	head_sha: &'a str,
	nonclean_rounds: i64,
	details_json: &'a str,
}
