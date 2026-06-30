use std::{
	cmp::Reverse,
	collections::BTreeMap,
	fs::{self, Metadata, OpenOptions},
	path::{Path, PathBuf},
	time::{Duration, SystemTime},
};

use rusqlite::Connection;
use time::OffsetDateTime;

use crate::{
	prelude::{Result, eyre},
	runtime as control_runtime,
};

use super::{
	policy::{
		DEFAULT_BACKUP_RETENTION_DAYS, DEFAULT_EVIDENCE_RETENTION_DAYS,
		DEFAULT_GIT_ASKPASS_HELPER_RETENTION_DAYS, DEFAULT_LOG_RETENTION_DAYS,
		LEGACY_GIT_ASKPASS_PREFIX, LEGACY_GIT_ASKPASS_SUFFIX, MaintenanceMode, MaintenancePolicy,
		MaintenanceScope,
	},
	reports::{
		BackupCandidate, BackupMaintenanceReport, FileMaintenanceAction, FileMaintenanceReport,
	},
};

pub(super) fn maintain_logs(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
	generated_at: OffsetDateTime,
) -> Result<FileMaintenanceReport> {
	let root = control_runtime::log_dir()?;
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

		if is_rotated_log_file(&path)
			&& file_is_older_than(&metadata, system_now, policy.log_retention)
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
			let rotated_path = rotated_path(&path, generated_at)?;

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
				copy_truncate(&path, &rotated_path)?;

				report.rotated_files += 1;
			}
		}
	}

	Ok(report)
}

pub(super) fn maintain_agent_evidence(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
	generated_at: OffsetDateTime,
) -> Result<FileMaintenanceReport> {
	let root = control_runtime::agent_evidence_dir()?;
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
				let rotated_path = rotated_path(&events_path, generated_at)?;

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
					copy_truncate(&events_path, &rotated_path)?;

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

			if file_is_older_than(&metadata, system_now, policy.evidence_retention) {
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

pub(super) fn maintain_git_askpass_helpers_for_scope(
	mode: MaintenanceMode,
	scope: MaintenanceScope,
	policy: MaintenancePolicy,
	system_now: SystemTime,
) -> Result<FileMaintenanceReport> {
	match maintain_git_askpass_helpers(mode, policy, system_now) {
		Ok(report) => Ok(report),
		Err(error) if scope == MaintenanceScope::AutoSafe => {
			tracing::warn!(
				?error,
				"Skipped Decodex auto-safe legacy Git askpass helper cleanup; control-plane maintenance continued."
			);

			Ok(FileMaintenanceReport {
				root: control_runtime::runtime_db_path()?.display().to_string(),
				..FileMaintenanceReport::default()
			})
		},
		Err(error) => Err(error),
	}
}

fn maintain_git_askpass_helpers(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
) -> Result<FileMaintenanceReport> {
	let database_path = control_runtime::runtime_db_path()?;
	let mut report = FileMaintenanceReport {
		root: database_path.display().to_string(),
		..FileMaintenanceReport::default()
	};

	if !database_path.exists() {
		return Ok(report);
	}

	let connection = Connection::open(database_path)?;

	if !sqlite_table_exists(&connection, "projects")? {
		return Ok(report);
	}

	for worktree_root in registered_worktree_roots(&connection)? {
		if !worktree_root.exists() {
			continue;
		}

		for entry in fs::read_dir(&worktree_root)? {
			let entry = entry?;
			let path = entry.path();
			let file_type = entry.file_type()?;

			if !file_type.is_file() || !is_legacy_git_askpass_helper(&path) {
				continue;
			}

			let metadata = entry.metadata()?;

			if !file_is_older_than(&metadata, system_now, policy.git_askpass_helper_retention) {
				continue;
			}

			let size = metadata.len();

			report.delete_candidates += 1;
			report.delete_bytes = report.delete_bytes.saturating_add(size);

			report.actions.push(FileMaintenanceAction {
				action: "delete",
				path: path.display().to_string(),
				bytes: size,
				target: None,
				reason: format!(
					"legacy Git askpass helper is older than {DEFAULT_GIT_ASKPASS_HELPER_RETENTION_DAYS} day"
				),
			});

			if mode.applies() {
				fs::remove_file(&path)?;

				report.deleted_files += 1;
			}
		}
	}

	Ok(report)
}

pub(super) fn maintain_backups(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
) -> Result<BackupMaintenanceReport> {
	let root = control_runtime::decodex_home_dir()?;
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

fn registered_worktree_roots(connection: &Connection) -> Result<Vec<PathBuf>> {
	let mut statement =
		connection.prepare("SELECT DISTINCT worktree_root FROM projects ORDER BY worktree_root")?;
	let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
	let mut roots = Vec::new();

	for row in rows {
		roots.push(PathBuf::from(row?));
	}

	Ok(roots)
}

fn sqlite_table_exists(connection: &Connection, table: &str) -> Result<bool> {
	let count = connection.query_row(
		"SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
		rusqlite::params![table],
		|row| row.get::<_, i64>(0),
	)?;

	Ok(count > 0)
}

fn copy_truncate(path: &Path, rotated_path: &Path) -> Result<()> {
	if let Some(parent) = rotated_path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::copy(path, rotated_path)?;
	OpenOptions::new().write(true).truncate(true).open(path)?;

	Ok(())
}

fn rotated_path(path: &Path, generated_at: OffsetDateTime) -> Result<PathBuf> {
	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no parent directory.", path.display())
	})?;
	let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no UTF-8 file name.", path.display())
	})?;
	let Some((prefix, suffix)) = file_name.rsplit_once('.') else {
		return Ok(parent.join(format!("{file_name}.{}", generated_at.unix_timestamp())));
	};
	let candidate = parent.join(format!("{prefix}.{}.{suffix}", generated_at.unix_timestamp()));

	next_available_path(candidate)
}

fn next_available_path(path: PathBuf) -> Result<PathBuf> {
	if !path.exists() {
		return Ok(path);
	}

	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no parent directory.", path.display())
	})?;
	let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no UTF-8 file name.", path.display())
	})?;

	for index in 1..=999 {
		let candidate = parent.join(format!("{file_name}.{index}"));

		if !candidate.exists() {
			return Ok(candidate);
		}
	}

	eyre::bail!("Could not allocate a unique maintenance rotation path for `{}`.", path.display());
}

fn file_is_older_than(metadata: &Metadata, system_now: SystemTime, retention: Duration) -> bool {
	metadata
		.modified()
		.ok()
		.and_then(|modified| system_now.duration_since(modified).ok())
		.is_some_and(|age| age > retention)
}

fn is_rotated_log_file(path: &Path) -> bool {
	path.file_stem()
		.and_then(|stem| stem.to_str())
		.and_then(|stem| stem.rsplit_once('.').map(|(_, timestamp)| timestamp))
		.is_some_and(|timestamp| timestamp.parse::<i64>().is_ok())
}

fn is_legacy_git_askpass_helper(path: &Path) -> bool {
	path.file_name().and_then(|name| name.to_str()).is_some_and(|file_name| {
		file_name.starts_with(LEGACY_GIT_ASKPASS_PREFIX)
			&& file_name.ends_with(LEGACY_GIT_ASKPASS_SUFFIX)
	})
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
