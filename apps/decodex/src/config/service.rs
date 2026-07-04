use std::{
	env, fs,
	path::{Path, PathBuf},
};

use crate::{
	config::{
		ProjectAutonomyConfig, ProjectCodexConfig, ProjectGitHubConfig,
		ProjectPrivacyClassifierConfig, ProjectTrackerConfig, document::ServiceConfigDocument,
		path_resolution, path_resolution::PROJECT_CONFIG_FILE_NAME, validation,
	},
	prelude::Result,
};

const WORKFLOW_FILE_NAME: &str = "WORKFLOW.md";

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
		let config_dir = path_resolution::canonicalize_path_best_effort(&env::current_dir()?);
		let document = toml::from_str::<ServiceConfigDocument>(input)?;
		let config_path = config_dir.join(PROJECT_CONFIG_FILE_NAME);

		Self::from_document(document, &config_dir, config_path)
	}

	/// Resolve the canonical `project.toml` path for a Decodex project directory.
	pub fn resolve_project_config_path(path: impl AsRef<Path>) -> Result<PathBuf> {
		path_resolution::resolve_project_config_file_path(path.as_ref())
	}

	/// Load service configuration from a project directory or its `project.toml` file.
	pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
		let path = Self::resolve_project_config_path(path)?;
		let config_dir = path_resolution::config_parent_dir(&path)?;
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
			config_path: path_resolution::canonicalize_path_best_effort(&config_path),
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
