//! Service configuration for Decodex.

mod autonomy;
mod codex;
mod document;
mod git_paths;
mod github;
mod paths;
mod privacy;
mod review;
mod tracker;
mod validation;

pub use self::{
	autonomy::{ProjectAutonomyConfig, ProjectAutonomyRuntimePolicyConfig},
	codex::{ProjectCodexAccountsConfig, ProjectCodexConfig},
	github::ProjectGitHubConfig,
	paths::ProjectPathsConfig,
	privacy::ProjectPrivacyClassifierConfig,
	review::ReviewLevel,
	tracker::ProjectTrackerConfig,
};
pub use git_paths::{
	canonical_repo_root_for_checkout, checkouts_share_repository, git_dir_for_checkout,
};

use std::{
	env, fs,
	path::{Component, Path, PathBuf},
};

use self::document::ServiceConfigDocument;
use crate::prelude::{Result, eyre};
#[cfg(test)]
use git_paths::path_buf_from_git_line_output;

const WORKFLOW_FILE_NAME: &str = "WORKFLOW.md";
const PROJECT_CONFIG_FILE_NAME: &str = "project.toml";

/// Top-level service configuration for one target repository and tracker integration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceConfig {
	service_id: String,
	config_path: PathBuf,
	repo_root: PathBuf,
	worktree_root: PathBuf,
	workflow_path: PathBuf,
	tracker: ProjectTrackerConfig,
	github: ProjectGitHubConfig,
	codex: ProjectCodexConfig,
	autonomy: ProjectAutonomyConfig,
	privacy_classifier: ProjectPrivacyClassifierConfig,
}
impl ServiceConfig {
	/// Parse service configuration from TOML text.
	pub fn parse_toml(input: &str) -> Result<Self> {
		let config_dir = canonicalize_path_best_effort(&env::current_dir()?);
		let document = toml::from_str::<ServiceConfigDocument>(input)?;
		let config_path = config_dir.join(PROJECT_CONFIG_FILE_NAME);

		Self::from_document(document, &config_dir, config_path)
	}

	/// Resolve the canonical `project.toml` path for a Decodex project directory.
	pub fn resolve_project_config_path(path: impl AsRef<Path>) -> Result<PathBuf> {
		resolve_project_config_file_path(path.as_ref())
	}

	/// Load service configuration from a project directory or its `project.toml` file.
	pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
		let path = Self::resolve_project_config_path(path)?;
		let config_dir = config_parent_dir(&path)?;
		let input = fs::read_to_string(&path)?;
		let document = toml::from_str::<ServiceConfigDocument>(&input)?;

		Self::from_document(document, &config_dir, path)
	}

	/// Stable identifier for this target service config.
	pub fn service_id(&self) -> &str {
		&self.service_id
	}

	/// Absolute path to this project's `project.toml`.
	pub fn config_path(&self) -> &Path {
		&self.config_path
	}

	/// Absolute repository root used for the target checkout.
	pub fn repo_root(&self) -> &Path {
		&self.repo_root
	}

	/// Worktree root where `decodex` creates issue lanes.
	pub fn worktree_root(&self) -> &Path {
		&self.worktree_root
	}

	/// Absolute path to the project-owned `WORKFLOW.md`.
	pub fn workflow_path(&self) -> &Path {
		&self.workflow_path
	}

	/// Tracker configuration for this project.
	pub fn tracker(&self) -> &ProjectTrackerConfig {
		&self.tracker
	}

	/// GitHub configuration for this project.
	pub fn github(&self) -> &ProjectGitHubConfig {
		&self.github
	}

	/// Codex defaults scoped to this project.
	pub fn codex(&self) -> &ProjectCodexConfig {
		&self.codex
	}

	/// Objective-autonomy references scoped to this project.
	pub fn autonomy(&self) -> &ProjectAutonomyConfig {
		&self.autonomy
	}

	/// Optional local classifier for Linear public projection text.
	pub fn privacy_classifier(&self) -> &ProjectPrivacyClassifierConfig {
		&self.privacy_classifier
	}

	fn from_document(
		document: ServiceConfigDocument,
		config_dir: &Path,
		config_path: PathBuf,
	) -> Result<Self> {
		document.validate()?;

		let repo_root = document.paths.resolve_repo_root(config_dir)?;

		validation::validate_nonempty_path("repo_root", &repo_root)?;

		Ok(Self {
			service_id: document.service_id,
			config_path: canonicalize_path_best_effort(&config_path),
			repo_root: repo_root.to_path_buf(),
			worktree_root: document.paths.resolve_worktree_root(&repo_root)?,
			workflow_path: config_dir.join(WORKFLOW_FILE_NAME),
			tracker: document.tracker,
			github: document.github.resolve_paths(config_dir)?,
			codex: document.codex.resolve_paths(config_dir)?,
			autonomy: document.autonomy,
			privacy_classifier: document.privacy_classifier,
		})
	}
}

fn canonicalize_path_best_effort(path: &Path) -> PathBuf {
	fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn resolve_project_config_file_path(path: &Path) -> Result<PathBuf> {
	let metadata = fs::metadata(path).map_err(|error| {
		eyre::eyre!("Failed to inspect Decodex project config path `{}`: {error}", path.display())
	})?;

	if metadata.is_dir() {
		return Ok(path.join(PROJECT_CONFIG_FILE_NAME));
	}
	if path.file_name().and_then(|name| name.to_str()) == Some(PROJECT_CONFIG_FILE_NAME) {
		return Ok(path.to_path_buf());
	}

	eyre::bail!(
		"Decodex project config must be a project directory or `{PROJECT_CONFIG_FILE_NAME}` file: `{}`.",
		path.display()
	);
}

fn config_parent_dir(config_path: &Path) -> Result<PathBuf> {
	let canonical_path = fs::canonicalize(config_path)?;
	let Some(parent) = canonical_path.parent() else {
		eyre::bail!("Config path `{}` must have a parent directory.", config_path.display());
	};

	Ok(parent.to_path_buf())
}

fn resolve_relative_path(base: &Path, path: &Path) -> PathBuf {
	let resolved = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };

	normalize_path(&resolved)
}

fn normalize_path(path: &Path) -> PathBuf {
	let mut normalized = PathBuf::new();

	for component in path.components() {
		match component {
			Component::CurDir => {},
			Component::ParentDir => match normalized.components().next_back() {
				Some(Component::Normal(_)) => {
					normalized.pop();
				},
				Some(Component::RootDir | Component::Prefix(_)) => {},
				Some(Component::ParentDir) | None => normalized.push(component.as_os_str()),
				Some(Component::CurDir) => {},
			},
			_ => normalized.push(component.as_os_str()),
		}
	}

	if normalized.as_os_str().is_empty() { PathBuf::from(".") } else { normalized }
}

#[cfg(test)]
mod tests;
