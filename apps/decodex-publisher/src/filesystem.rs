use std::{
	env,
	fs::{self, OpenOptions},
	io::Write as _,
	path::{Path, PathBuf},
	sync::OnceLock,
};

use serde_json::Value;

use crate::prelude::{Result, eyre};

pub(crate) fn repo_root() -> Result<PathBuf> {
	static ROOT: OnceLock<PathBuf> = OnceLock::new();

	if let Some(root) = ROOT.get() {
		return Ok(root.clone());
	}

	let current = env::current_dir()?;
	let root = current
		.ancestors()
		.find(|candidate| {
			candidate.join("automations/decodex/skills/x-post-publisher/SKILL.md").is_file()
				&& candidate.join("apps/decodex-publisher/src/lib.rs").is_file()
		})
		.map(Path::to_path_buf)
		.ok_or_else(|| eyre::eyre!("could not locate repository root"))?;
	let _ = ROOT.set(root.clone());

	Ok(root)
}

pub(crate) fn resolve_against(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

pub(crate) fn path_arg(root: &Path, path: &Path) -> String {
	path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

pub(crate) fn load_json(path: &Path) -> Result<Value> {
	let payload = fs::read_to_string(path)
		.map_err(|error| eyre::eyre!("failed to read {}: {error}", path.display()))?;

	serde_json::from_str(&payload)
		.map_err(|error| eyre::eyre!("failed to parse {} as JSON: {error}", path.display()))
}

pub(crate) fn write_new_json(path: &Path, payload: &Value) -> Result<()> {
	if path.exists() {
		return Err(eyre::eyre!("refusing to overwrite existing file: {}", path.display()));
	}

	let parent = path.parent().ok_or_else(|| eyre::eyre!("output path must have a parent"))?;
	fs::create_dir_all(parent)?;
	let mut random = [0_u8; 16];
	getrandom::fill(&mut random).map_err(|_| eyre::eyre!("secure randomness is unavailable"))?;
	let suffix = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
	let file_name = path
		.file_name()
		.and_then(|value| value.to_str())
		.ok_or_else(|| eyre::eyre!("output filename must be UTF-8"))?;
	let temporary_path = parent.join(format!(".{file_name}.{suffix}.tmp"));
	let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary_path)?;
	file.write_all(serde_json::to_string_pretty(payload)?.as_bytes())?;
	file.write_all(b"\n")?;
	file.sync_all()?;
	drop(file);

	let linked = fs::hard_link(&temporary_path, path);
	let cleanup = fs::remove_file(&temporary_path);
	if let Err(error) = linked {
		if cleanup.is_err() {
			return Err(eyre::eyre!(
				"failed to publish and clean temporary JSON file {}: {error}",
				path.display()
			));
		}
		if error.kind() == std::io::ErrorKind::AlreadyExists {
			return Err(eyre::eyre!("refusing to overwrite existing file: {}", path.display()));
		}

		return Err(eyre::eyre!("failed to publish {}: {error}", path.display()));
	}
	cleanup?;

	Ok(())
}

pub(crate) fn collect_json_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
	let mut files = Vec::new();

	for path in paths {
		if !path.exists() {
			continue;
		}

		collect_json_files_inner(path, &mut files)?;
	}

	files.sort();

	Ok(files)
}

fn collect_json_files_inner(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
	if path.is_file() {
		if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
			files.push(path.to_path_buf());
		}

		return Ok(());
	}
	if path.is_dir() {
		for entry in fs::read_dir(path)? {
			collect_json_files_inner(&entry?.path(), files)?;
		}
	}

	Ok(())
}
