//! Shared Radar filesystem, schema, and repository helpers.

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

pub(crate) fn write_json_if_material_changed(
	path: &Path,
	payload: &Value,
	kind: RefreshKind,
) -> crate::prelude::Result<bool> {
	if let Ok(existing) = crate::load_json(path)
		&& material_json(&existing, &kind) == material_json(payload, &kind)
	{
		return Ok(false);
	}
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::write(path, format!("{}\n", crate::pretty_json(payload)?))?;

	Ok(true)
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

	if !path.exists() {
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
