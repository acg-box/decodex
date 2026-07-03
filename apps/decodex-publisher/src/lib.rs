//! Decodex auxiliary publishing handoff tooling.

mod cli;
mod social_publish;
mod social_validation;
mod prelude {
	pub use color_eyre::{Result, eyre};
}

use std::{
	env,
	fs::{self, OpenOptions},
	io::Write,
	path::{Path, PathBuf},
	sync::OnceLock,
};

use clap::Parser as _;
use serde::Serialize;
use serde_json::{Map, Value};

use cli::Cli;
use prelude::{Result, eyre};
use social_validation::SocialValidationState;

pub(crate) const SOCIAL_CANDIDATE_SCHEMA: &str = "social_candidate/v1";
pub(crate) const SOCIAL_POST_SCHEMA: &str = "social_post/v1";
pub(crate) const SOCIAL_PUBLISH_RESERVATION_SCHEMA: &str = "social_publish_reservation/v1";
pub(crate) const DEFAULT_SOCIAL_CANDIDATES_DIR: &str =
	".agent/automations/decodex/cache/social/x/candidates";
pub(crate) const DEFAULT_SOCIAL_RESERVATIONS_DIR: &str =
	".agent/automations/decodex/cache/social/x/reservations";
pub(crate) const DEFAULT_SOCIAL_POSTS_DIR: &str = ".agent/automations/decodex/cache/social/x/posts";

#[derive(Debug)]
pub(crate) struct SocialReservePublishRequest {
	pub(crate) slug: String,
	pub(crate) mode: String,
	pub(crate) idempotency_key: String,
	pub(crate) reserved_at: String,
	pub(crate) expires_at: String,
	pub(crate) day: String,
	pub(crate) timezone: String,
	pub(crate) candidate_paths: Vec<PathBuf>,
	pub(crate) urls: Vec<String>,
	pub(crate) duplicate_keys: Vec<String>,
	pub(crate) out_dir: PathBuf,
	pub(crate) posts_dir: PathBuf,
	pub(crate) automation_id: Option<String>,
	pub(crate) run_id: Option<String>,
	pub(crate) branch: Option<String>,
	pub(crate) daily_limit: usize,
	pub(crate) dry_run: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SocialReservePublishReport {
	pub(crate) status: String,
	pub(crate) path: String,
	pub(crate) idempotency_key: String,
	pub(crate) daily_limit: usize,
	pub(crate) published_count: usize,
	pub(crate) active_reservation_count: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SocialValidationReport {
	pub(crate) checked_files: usize,
	pub(crate) errors: Vec<String>,
}

/// Run the Decodex Publisher CLI.
pub fn run() -> Result<()> {
	color_eyre::install()?;

	Cli::parse().run()
}

pub(crate) fn reserve_social_publish(
	request: &SocialReservePublishRequest,
) -> Result<SocialReservePublishReport> {
	social_publish::reserve_social_publish(request)
}

pub(crate) fn validate_social(paths: &[PathBuf]) -> Result<SocialValidationReport> {
	let root = repo_root()?;
	let paths = if paths.is_empty() {
		vec![
			PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
		]
	} else {
		paths.to_vec()
	};
	let files = collect_json_files(
		&paths.iter().map(|path| resolve_against(&root, path)).collect::<Vec<_>>(),
	)?;
	let mut state = SocialValidationState::new();
	let mut errors = Vec::new();

	for path in &files {
		let payload = load_json(path)?;
		let validation = social_validation::validate_social_artifact_for_path(path, &payload);

		for error in validation.errors {
			errors.push(format!("{}: {error}", path_arg(&root, path)));
		}

		social_validation::validate_social_cross_file_constraints(
			path,
			&payload,
			&mut state,
			&mut errors,
		);
	}

	if !errors.is_empty() {
		return Err(eyre::eyre!("Social artifact validation failed:\n- {}", errors.join("\n- ")));
	}

	Ok(SocialValidationReport { checked_files: files.len(), errors })
}

pub(crate) fn validate_generated_social_artifact(payload: &Value) -> Result<()> {
	let validation = social_validation::validate_social_artifact(payload);

	if !validation.errors.is_empty() {
		eyre::bail!("Social artifact validation failed:\n- {}", validation.errors.join("\n- "));
	}

	Ok(())
}

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

pub(crate) fn slugify(value: &str) -> String {
	let mut slug = String::new();
	let mut previous_dash = false;

	for byte in value.bytes() {
		let ch = byte as char;

		if ch.is_ascii_alphanumeric() {
			slug.push(ch.to_ascii_lowercase());

			previous_dash = false;
		} else if !previous_dash && !slug.is_empty() {
			slug.push('-');

			previous_dash = true;
		}
	}

	while slug.ends_with('-') {
		slug.pop();
	}

	if slug.is_empty() { "social-post".into() } else { slug }
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

	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}

	let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;

	file.write_all(serde_json::to_string_pretty(payload)?.as_bytes())?;
	file.write_all(b"\n")?;

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

#[cfg(test)] mod tests;
