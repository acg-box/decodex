//! Shared Radar filesystem, schema, and repository helpers.

use std::{
	fs::{File, OpenOptions},
	os::{fd::AsRawFd as _, unix::fs::OpenOptionsExt as _},
};

use crate::{
	BTreeSet, CONFIG_FEATURE_CATALOG_PATH, GitHubApi, Path, PathBuf, RadarRefreshQueueRequest,
	RefreshKind, Value, eyre, fs,
};

pub(crate) fn validate_expected_schema(
	value: &Value,
	schema: &str,
	label: &str,
) -> crate::prelude::Result<()> {
	let validation = crate::validate_artifact(value);

	if validation.schema.as_deref() != Some(schema) {
		return Err(eyre::eyre!("{label} schema must be {schema}"));
	}
	if !validation.errors.is_empty() {
		return Err(eyre::eyre!(
			"{label} validation failed:\n- {}",
			validation.errors.join("\n- ")
		));
	}

	Ok(())
}

pub(crate) fn repo_default_branch(api: &GitHubApi, repo: &str) -> crate::prelude::Result<String> {
	let payload = api.get(&format!("https://api.github.com/repos/{repo}"))?.payload;

	crate::required_value_string(&payload, "default_branch")
		.map_err(|error| eyre::eyre!("Unable to resolve default branch for {repo}: {error}"))
}

pub(crate) fn absolute_repo_path(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

pub(crate) fn ledger_path(root: &Path, request: &RadarRefreshQueueRequest) -> Option<PathBuf> {
	(!request.no_ledger).then(|| absolute_repo_path(root, &request.ledger))
}

pub(crate) fn sorted_json_files(path: &Path) -> crate::prelude::Result<Vec<PathBuf>> {
	if crate::is_radar_cache_path(path) {
		return crate::collect_private_json_files_if_present(path);
	}
	if !path.exists() {
		return Ok(Vec::new());
	}

	let mut files = fs::read_dir(path)?
		.map(|entry| entry.map(|entry| entry.path()))
		.collect::<std::result::Result<Vec<_>, _>>()?;

	files.retain(|path| {
		path.is_file() && path.extension().is_some_and(|extension| extension == "json")
	});
	files.sort();

	Ok(files)
}

pub(crate) fn collect_bundle_json_files(paths: &[PathBuf]) -> crate::prelude::Result<Vec<PathBuf>> {
	if paths.is_empty() {
		eyre::bail!("at least one bundle JSON file or directory is required");
	}

	let mut files = Vec::new();

	for path in paths {
		if crate::is_radar_cache_path(path) {
			if path.extension().is_some_and(|extension| extension == "json") {
				if crate::private_file_exists(path)? {
					files.push(path.clone());
				} else {
					eyre::bail!("Bundle validation path does not exist");
				}
			} else {
				files.extend(crate::collect_private_json_files(path)?);
			}

			continue;
		}

		if path.is_dir() {
			files.extend(sorted_json_files(path)?);
		} else if path.is_file() {
			files.push(path.clone());
		} else {
			eyre::bail!("Bundle validation path does not exist: {}", path.display());
		}
	}

	files.sort();

	Ok(files)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RefreshWriteReport {
	pub(crate) material_changed: bool,
	pub(crate) written: bool,
	pub(crate) refreshed_at: String,
}

pub(crate) fn refresh_json(
	path: &Path,
	payload: &Value,
	kind: RefreshKind,
) -> crate::prelude::Result<RefreshWriteReport> {
	refresh_json_with_write(path, payload, kind, true)
}

pub(crate) fn inspect_json_refresh(
	path: &Path,
	payload: &Value,
	kind: RefreshKind,
) -> crate::prelude::Result<RefreshWriteReport> {
	refresh_json_with_write(path, payload, kind, false)
}

fn refresh_json_with_write(
	path: &Path,
	payload: &Value,
	kind: RefreshKind,
	write: bool,
) -> crate::prelude::Result<RefreshWriteReport> {
	refresh_json_with_write_and_hook(path, payload, kind, write, || {})
}

fn refresh_json_with_write_and_hook(
	path: &Path,
	payload: &Value,
	kind: RefreshKind,
	write: bool,
	after_comparison: impl FnOnce(),
) -> crate::prelude::Result<RefreshWriteReport> {
	let refreshed_at = payload
		.get("generated_at")
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("refreshed artifact must contain generated_at"))?
		.to_owned();

	if crate::is_radar_cache_path(path) {
		return refresh_private_json(path, payload, kind, write, refreshed_at, after_comparison);
	}
	let _directory_lock = acquire_external_refresh_lock(path, write)?;
	let existing = match crate::load_json(path) {
		Ok(existing) => Some(existing),
		Err(error) if is_not_found(&error) => None,
		Err(error) => return Err(error),
	};
	let material_changed = compare_refresh(existing.as_ref(), payload, &kind)?;

	after_comparison();
	if write {
		crate::write_json(path, payload)?;
	}
	Ok(RefreshWriteReport { material_changed, written: write, refreshed_at })
}

fn acquire_external_refresh_lock(
	path: &Path,
	create_parent: bool,
) -> crate::prelude::Result<Option<ExternalRefreshLock>> {
	let parent =
		path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));

	if create_parent {
		fs::create_dir_all(parent)?;
	} else if !parent.exists() {
		return Ok(None);
	}
	let directory = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
		.open(parent)?;

	if unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_EX) } == -1 {
		return Err(std::io::Error::last_os_error().into());
	}

	Ok(Some(ExternalRefreshLock(directory)))
}

struct ExternalRefreshLock(File);
impl Drop for ExternalRefreshLock {
	fn drop(&mut self) {
		unsafe {
			libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
		}
	}
}

fn refresh_private_json(
	path: &Path,
	payload: &Value,
	kind: RefreshKind,
	write: bool,
	refreshed_at: String,
	after_comparison: impl FnOnce(),
) -> crate::prelude::Result<RefreshWriteReport> {
	let (cache, relative) = crate::private_fs::private_cache_file(path)?;
	let lock = cache.lock()?;
	let original_identity = lock.cache().metadata(&relative)?;
	let existing = match original_identity.as_ref() {
		Some(_) => {
			let raw = lock.read(&relative)?;
			let value = serde_json::from_slice(&raw).map_err(|error| {
				eyre::eyre!("Failed to parse JSON from {}: {error}", path.display())
			})?;

			Some(value)
		},
		None => None,
	};
	let material_changed = compare_refresh(existing.as_ref(), payload, &kind)?;

	after_comparison();
	if write {
		let mut output = serde_json::to_string_pretty(payload)?;

		output.push('\n');
		lock.write_atomic_if_matches(&relative, original_identity.as_ref(), output.as_bytes())?;
	}

	Ok(RefreshWriteReport { material_changed, written: write, refreshed_at })
}

fn compare_refresh(
	existing: Option<&Value>,
	payload: &Value,
	kind: &RefreshKind,
) -> crate::prelude::Result<bool> {
	let incoming_at = generated_at(payload, "incoming")?;
	let Some(existing) = existing else {
		return Ok(true);
	};
	let existing_at = generated_at(existing, "existing")?;

	if incoming_at < existing_at {
		eyre::bail!("refreshed artifact is older than the currently stored observation");
	}

	Ok(material_json(existing, kind) != material_json(payload, kind))
}

fn generated_at(payload: &Value, label: &str) -> crate::prelude::Result<time::OffsetDateTime> {
	let value = payload
		.get("generated_at")
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("{label} refreshed artifact must contain generated_at"))?;

	time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
		.map_err(|error| eyre::eyre!("{label} refreshed artifact generated_at is invalid: {error}"))
}

fn is_not_found(error: &eyre::Report) -> bool {
	error
		.chain()
		.find_map(|cause| cause.downcast_ref::<std::io::Error>())
		.is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
pub(crate) fn refresh_json_after_comparison(
	path: &Path,
	payload: &Value,
	kind: RefreshKind,
	after_comparison: impl FnOnce(),
) -> crate::prelude::Result<RefreshWriteReport> {
	refresh_json_with_write_and_hook(path, payload, kind, true, after_comparison)
}

pub(crate) fn material_json(payload: &Value, kind: &RefreshKind) -> Value {
	let mut normalized = payload.clone();

	match kind {
		RefreshKind::Queue | RefreshKind::ReleaseDelta => {
			if let Some(object) = normalized.as_object_mut() {
				object.insert("generated_at".to_owned(), Value::String(String::new()));
			}
		},
	}

	normalized
}

pub(crate) fn load_known_feature_names(root: &Path) -> crate::prelude::Result<BTreeSet<String>> {
	let path = root.join(CONFIG_FEATURE_CATALOG_PATH);

	if !crate::private_file_exists(&path)? {
		return Ok(BTreeSet::new());
	}

	let payload = crate::load_json(&path)?;
	let names = payload
		.get("features")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|item| item.get("name").and_then(Value::as_str))
		.filter(|name| !name.is_empty())
		.map(str::to_owned)
		.collect();

	Ok(names)
}
