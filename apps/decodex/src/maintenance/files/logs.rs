use std::{fs, time::SystemTime};

use time::OffsetDateTime;

use crate::{
	maintenance::{
		files::path_utils,
		policy::{DEFAULT_LOG_RETENTION_DAYS, MaintenanceMode, MaintenancePolicy},
		reports::{FileMaintenanceAction, FileMaintenanceReport},
	},
	prelude::Result,
	runtime,
};

pub(in crate::maintenance) fn maintain_logs(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
	generated_at: OffsetDateTime,
) -> Result<FileMaintenanceReport> {
	let root = runtime::log_dir()?;
	let mut report = FileMaintenanceReport {
		root: root.display().to_string(),
		..FileMaintenanceReport::default()
	};

	if !root.exists() {
		return Ok(report);
	}

	for entry in fs::read_dir(&root)? {
		let entry = entry?;
		let path = entry.path();

		if !path.is_file()
			|| path.extension().and_then(|extension| extension.to_str()) != Some("log")
		{
			continue;
		}

		let metadata = entry.metadata()?;
		let size = metadata.len();

		if path_utils::is_rotated_log_file(&path)
			&& path_utils::file_is_older_than(&metadata, system_now, policy.log_retention)
		{
			report.delete_candidates += 1;
			report.delete_bytes = report.delete_bytes.saturating_add(size);

			report.actions.push(FileMaintenanceAction {
				action: "delete",
				path: path.display().to_string(),
				bytes: size,
				target: None,
				reason: format!("log is older than {DEFAULT_LOG_RETENTION_DAYS} days"),
			});

			if mode.applies() {
				fs::remove_file(&path)?;

				report.deleted_files += 1;
			}

			continue;
		}
		if size > policy.log_rotate_bytes {
			let rotated_path = path_utils::rotated_path(&path, generated_at)?;

			report.rotate_candidates += 1;
			report.rotate_bytes = report.rotate_bytes.saturating_add(size);

			report.actions.push(FileMaintenanceAction {
				action: "rotate",
				path: path.display().to_string(),
				bytes: size,
				target: Some(rotated_path.display().to_string()),
				reason: format!(
					"size {} exceeds {} byte log rotation threshold",
					size, policy.log_rotate_bytes
				),
			});

			if mode.applies() {
				path_utils::copy_truncate(&path, &rotated_path)?;

				report.rotated_files += 1;
			}
		}
	}

	Ok(report)
}
