use crate::maintenance;

pub(crate) fn run_control_plane_maintenance(trigger: &'static str) {
	match maintenance::run_auto_safe_prune() {
		Ok(report) => {
			tracing::info!(
				trigger = trigger,
				log_rotated_files = report.logs.rotated_files,
				evidence_rotated_files = report.agent_evidence.rotated_files,
				backup_deleted_files = report.backups.deleted_files,
				wal_checkpoint_mode = report
					.wal_checkpoint
					.as_ref()
					.map(|checkpoint| checkpoint.mode)
					.unwrap_or("skipped"),
				"Completed Decodex auto-safe maintenance."
			);
		},
		Err(error) => {
			let _ = error;

			tracing::warn!(
				trigger = trigger,
				"Decodex auto-safe maintenance failed; sensitive runtime details were withheld from control-plane logs."
			);
		},
	}
}
