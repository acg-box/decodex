use std::{cmp::Reverse, collections::BTreeMap, fs, path::Path, time::SystemTime};

use crate::{
	maintenance::{
		policy::{DEFAULT_BACKUP_RETENTION_DAYS, MaintenanceMode, MaintenancePolicy},
		reports::{BackupCandidate, BackupMaintenanceReport, FileMaintenanceAction},
	},
	prelude::Result,
	runtime,
};

pub(in crate::maintenance) fn maintain_backups(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
) -> Result<BackupMaintenanceReport> {
	let root = runtime::decodex_home_dir()?;
	let mut report = BackupMaintenanceReport {
		root: root.display().to_string(),
		..BackupMaintenanceReport::default()
	};

	if !root.exists() {
		return Ok(report);
	}

	let mut groups = BTreeMap::<String, Vec<BackupCandidate>>::new();

	collect_backup_candidates(&root, &mut groups)?;

	for candidates in groups.values_mut() {
		candidates.sort_by_key(|candidate| Reverse(candidate.modified));

		for (index, candidate) in candidates.iter().enumerate() {
			let young_enough = system_now
				.duration_since(candidate.modified)
				.map(|age| age <= policy.backup_retention)
				.unwrap_or(true);
			let recent_enough = index < policy.backup_keep_recent;

			if recent_enough || young_enough {
				continue;
			}

			report.delete_candidates += 1;
			report.delete_bytes = report.delete_bytes.saturating_add(candidate.bytes);

			report.actions.push(FileMaintenanceAction {
				action: "delete",
				path: candidate.path.display().to_string(),
				bytes: candidate.bytes,
				target: None,
				reason: format!(
					"backup is outside the latest {} files and older than {DEFAULT_BACKUP_RETENTION_DAYS} days",
					policy.backup_keep_recent
				),
			});

			if mode.applies() {
				fs::remove_file(&candidate.path)?;

				report.deleted_files += 1;
			}
		}
	}

	Ok(report)
}

fn collect_backup_candidates(
	root: &Path,
	groups: &mut BTreeMap<String, Vec<BackupCandidate>>,
) -> Result<()> {
	for entry in fs::read_dir(root)? {
		let entry = entry?;
		let path = entry.path();
		let file_type = entry.file_type()?;

		if file_type.is_dir() {
			collect_backup_candidates(&path, groups)?;

			continue;
		}
		if !file_type.is_file() {
			continue;
		}

		let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
			continue;
		};
		let Some((backup_prefix, _suffix)) = file_name.split_once(".bak-") else {
			continue;
		};
		let metadata = entry.metadata()?;
		let group_key = path
			.parent()
			.map(|parent| parent.join(backup_prefix).display().to_string())
			.unwrap_or_else(|| backup_prefix.to_owned());

		groups.entry(group_key).or_default().push(BackupCandidate {
			path,
			bytes: metadata.len(),
			modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
		});
	}

	Ok(())
}
