// Runtime activity-marker filesystem helpers.

mod accounts;
mod identity;
mod progress;
mod read;
mod record;
mod retry;
mod storage;
mod write;

#[cfg(test)] pub(crate) use self::identity::current_process_start_identity;
pub(crate) use self::{
	identity::{current_host_boot_id, process_start_identity},
	progress::protocol_event_counts_as_work_progress,
	read::{
		read_run_activity_marker, read_run_activity_marker_snapshot,
		read_run_protocol_activity_marker,
	},
	retry::{
		clear_run_retry_schedule, read_run_retry_budget_attempt_count,
		write_run_retry_budget_attempt_count, write_run_retry_schedule,
	},
	write::{
		write_run_account_marker, write_run_effective_runtime_marker, write_run_operation_marker,
		write_run_operation_marker_for_process, write_run_operation_marker_preserving_activity,
		write_run_protocol_activity_marker, write_run_thread_marker,
		write_run_thread_status_marker, write_run_turn_marker,
	},
};
#[cfg(test)]
pub(crate) use self::{
	storage::{read_run_activity_marker_record, write_run_activity_marker_record},
	write::{
		write_run_activity_marker, write_run_activity_marker_at,
		write_run_activity_marker_for_process,
	},
};
