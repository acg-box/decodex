use std::{fs, time::SystemTime};

use time::OffsetDateTime;

use crate::{
	maintenance::{
		files::path_utils,
		policy::{DEFAULT_EVIDENCE_RETENTION_DAYS, MaintenanceMode, MaintenancePolicy},
		reports::{FileMaintenanceAction, FileMaintenanceReport},
	},
	prelude::Result,
	runtime,
};

pub(in crate::maintenance) fn maintain_agent_evidence(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
	generated_at: OffsetDateTime,
) -> Result<FileMaintenanceReport> {
	let root = runtime::agent_evidence_dir()?;
	let mut report = FileMaintenanceReport {
		root: root.display().to_string(),
		..FileMaintenanceReport::default()
	};

	if !root.exists() {
		return Ok(report);
	}

	for entry in fs::read_dir(&root)? {
		let entry = entry?;
		let service_root = entry.path();

		if !service_root.is_dir() {
			continue;
		}

		let events_path = service_root.join("events.jsonl");

		if events_path.is_file() {
			let metadata = fs::metadata(&events_path)?;
			let size = metadata.len();

			if size > policy.evidence_rotate_bytes {
				let rotated_path = path_utils::rotated_path(&events_path, generated_at)?;

				report.rotate_candidates += 1;
				report.rotate_bytes = report.rotate_bytes.saturating_add(size);

				report.actions.push(FileMaintenanceAction {
					action: "rotate",
					path: events_path.display().to_string(),
					bytes: size,
					target: Some(rotated_path.display().to_string()),
					reason: format!(
						"size {} exceeds {} byte agent-evidence event threshold",
						size, policy.evidence_rotate_bytes
					),
				});

				if mode.applies() {
					path_utils::copy_truncate(&events_path, &rotated_path)?;

					report.rotated_files += 1;
				}
			}
		}

		for event_entry in fs::read_dir(&service_root)? {
			let event_entry = event_entry?;
			let path = event_entry.path();
			let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
				continue;
			};

			if !path.is_file()
				|| file_name == "events.jsonl"
				|| !file_name.starts_with("events.")
				|| path.extension().and_then(|extension| extension.to_str()) != Some("jsonl")
			{
				continue;
			}

			let metadata = event_entry.metadata()?;

			if path_utils::file_is_older_than(&metadata, system_now, policy.evidence_retention) {
				let size = metadata.len();

				report.delete_candidates += 1;
				report.delete_bytes = report.delete_bytes.saturating_add(size);

				report.actions.push(FileMaintenanceAction {
					action: "delete",
					path: path.display().to_string(),
					bytes: size,
					target: None,
					reason: format!(
						"rotated agent-evidence event stream is older than {DEFAULT_EVIDENCE_RETENTION_DAYS} days"
					),
				});

				if mode.applies() {
					fs::remove_file(&path)?;

					report.deleted_files += 1;
				}
			}
		}
	}

	Ok(report)
}
