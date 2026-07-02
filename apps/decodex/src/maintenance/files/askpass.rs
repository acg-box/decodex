use std::{
	fs,
	path::{Path, PathBuf},
	time::SystemTime,
};

use rusqlite::Connection;

use crate::{
	maintenance::{
		files::path_utils,
		policy::{
			DEFAULT_GIT_ASKPASS_HELPER_RETENTION_DAYS, LEGACY_GIT_ASKPASS_PREFIX,
			LEGACY_GIT_ASKPASS_SUFFIX, MaintenanceMode, MaintenancePolicy, MaintenanceScope,
		},
		reports::{FileMaintenanceAction, FileMaintenanceReport},
	},
	prelude::Result,
	runtime,
};

pub(in crate::maintenance) fn maintain_git_askpass_helpers_for_scope(
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
				root: runtime::runtime_db_path()?.display().to_string(),
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
	let database_path = runtime::runtime_db_path()?;
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

			if !path_utils::file_is_older_than(
				&metadata,
				system_now,
				policy.git_askpass_helper_retention,
			) {
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

fn is_legacy_git_askpass_helper(path: &Path) -> bool {
	path.file_name().and_then(|name| name.to_str()).is_some_and(|file_name| {
		file_name.starts_with(LEGACY_GIT_ASKPASS_PREFIX)
			&& file_name.ends_with(LEGACY_GIT_ASKPASS_SUFFIX)
	})
}
