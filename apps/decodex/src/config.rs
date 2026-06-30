//! Service configuration for Decodex.

use std::{
	env, fs,
	path::{Component, Path, PathBuf},
};

use reqwest::Url;
use serde::Deserialize;

use crate::prelude::{Result, eyre};

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

		validate_nonempty_path("repo_root", &repo_root)?;

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

/// Tracker-specific settings for a target project.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrackerConfig {
	api_key_env_var: String,
}
impl ProjectTrackerConfig {
	/// Name of the environment variable that stores the tracker API key.
	pub fn api_key_env_var(&self) -> &str {
		&self.api_key_env_var
	}

	/// Resolve the configured tracker API key env-var name into a concrete token string.
	pub fn resolve_api_key(&self) -> Result<String> {
		resolve_secret_env_var("tracker.api_key_env_var", self.api_key_env_var())
	}

	fn validate(&self) -> Result<()> {
		validate_env_var_name("tracker.api_key_env_var", self.api_key_env_var())?;

		Ok(())
	}
}

/// GitHub settings for a target project.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectGitHubConfig {
	token_env_var: String,
	command_path: Option<PathBuf>,
}
impl ProjectGitHubConfig {
	/// Name of the environment variable that stores the GitHub token.
	pub fn token_env_var(&self) -> &str {
		&self.token_env_var
	}

	/// Optional configured GitHub CLI command path.
	pub fn command_path(&self) -> Option<&Path> {
		self.command_path.as_deref()
	}

	/// Resolve the configured GitHub token env-var name into a concrete token string.
	pub fn resolve_token(&self) -> Result<String> {
		resolve_secret_env_var("github.token_env_var", self.token_env_var())
	}

	fn resolve_paths(mut self, config_dir: &Path) -> Result<Self> {
		if let Some(command_path) = self.command_path.take() {
			validate_nonempty_path("github.command_path", &command_path)?;

			self.command_path = Some(resolve_relative_path(config_dir, &command_path));
		}

		Ok(self)
	}

	fn validate(&self) -> Result<()> {
		validate_env_var_name("github.token_env_var", self.token_env_var())?;

		if let Some(command_path) = self.command_path.as_deref() {
			validate_nonempty_path("github.command_path", command_path)?;
		}

		Ok(())
	}
}

/// Project-level Codex defaults from service configuration.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct ProjectCodexConfig {
	#[serde(default = "default_review_level")]
	review: ReviewLevel,
	accounts: Option<ProjectCodexAccountsConfig>,
}
impl ProjectCodexConfig {
	/// Review level Decodex should apply for agent runs.
	pub fn review_level(&self) -> ReviewLevel {
		self.review
	}

	/// Optional ChatGPT accounts used to seed Codex app-server auth.
	pub fn accounts(&self) -> Option<&ProjectCodexAccountsConfig> {
		self.accounts.as_ref()
	}

	fn resolve_paths(mut self, _config_dir: &Path) -> Result<Self> {
		if let Some(accounts) = self.accounts.take() {
			accounts.validate()?;

			self.accounts = Some(accounts);
		}

		Ok(self)
	}

	fn validate(&self) -> Result<()> {
		if let Some(accounts) = &self.accounts {
			accounts.validate()?;
		}

		Ok(())
	}
}

/// Optional local-only classifier for public Linear projection text.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectPrivacyClassifierConfig {
	endpoint: Option<String>,
	#[serde(default = "default_privacy_classifier_timeout_ms")]
	timeout_ms: u64,
}
impl ProjectPrivacyClassifierConfig {
	/// Loopback HTTP endpoint for an operator-managed local classifier runtime.
	pub fn endpoint(&self) -> Option<&str> {
		self.endpoint.as_deref()
	}

	/// Per-field local classifier request timeout.
	pub fn timeout_ms(&self) -> u64 {
		self.timeout_ms
	}

	fn validate(&self) -> Result<()> {
		if self.timeout_ms == 0 {
			eyre::bail!("`privacy_classifier.timeout_ms` must be greater than zero.");
		}
		if self.timeout_ms > 30_000 {
			eyre::bail!("`privacy_classifier.timeout_ms` must be 30000 or less.");
		}

		if let Some(endpoint) = self.endpoint.as_deref() {
			validate_local_privacy_classifier_endpoint(endpoint)?;
		}

		Ok(())
	}
}

impl Default for ProjectPrivacyClassifierConfig {
	fn default() -> Self {
		Self { endpoint: None, timeout_ms: default_privacy_classifier_timeout_ms() }
	}
}

/// Project-autonomy references from service configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAutonomyConfig {
	#[serde(default)]
	auto_promote: bool,
	#[serde(default)]
	auto_intake: bool,
	runtime_policy: Option<ProjectAutonomyRuntimePolicyConfig>,
}
impl ProjectAutonomyConfig {
	/// Whether accepted runtime policy may promote proposals without another chat turn.
	pub fn auto_promote(&self) -> bool {
		self.auto_promote
	}

	/// Whether accepted runtime policy may enter Program Intake after promotion.
	pub fn auto_intake(&self) -> bool {
		self.auto_intake
	}

	/// References to accepted runtime authority records, when configured.
	pub fn runtime_policy(&self) -> Option<&ProjectAutonomyRuntimePolicyConfig> {
		self.runtime_policy.as_ref()
	}

	fn validate(&self) -> Result<()> {
		if self.auto_intake && !self.auto_promote {
			eyre::bail!("`autonomy.auto_intake = true` requires `autonomy.auto_promote = true`.");
		}
		if self.auto_promote && self.runtime_policy.is_none() {
			eyre::bail!(
				"`autonomy.auto_promote = true` requires `[autonomy.runtime_policy]` references."
			);
		}

		if let Some(runtime_policy) = &self.runtime_policy {
			runtime_policy.validate()?;

			if self.auto_intake && runtime_policy.team_issue_identifier().is_none() {
				eyre::bail!(
					"`autonomy.auto_intake = true` requires `autonomy.runtime_policy.team_issue_identifier`."
				);
			}
		}

		Ok(())
	}
}

/// References to accepted Objective Contract and project-policy authority records.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAutonomyRuntimePolicyConfig {
	accepted_objective_id: String,
	accepted_objective_version: String,
	accepted_policy_id: String,
	accepted_policy_version: String,
	policy_authority_ref: String,
	team_issue_identifier: Option<String>,
}
impl ProjectAutonomyRuntimePolicyConfig {
	/// Accepted runtime Objective Contract id.
	pub fn accepted_objective_id(&self) -> &str {
		&self.accepted_objective_id
	}

	/// Accepted runtime Objective Contract version.
	pub fn accepted_objective_version(&self) -> &str {
		&self.accepted_objective_version
	}

	/// Accepted runtime project-policy id.
	pub fn accepted_policy_id(&self) -> &str {
		&self.accepted_policy_id
	}

	/// Accepted runtime project-policy version.
	pub fn accepted_policy_version(&self) -> &str {
		&self.accepted_policy_version
	}

	/// Runtime authority reference for the accepted project policy record.
	pub fn policy_authority_ref(&self) -> &str {
		&self.policy_authority_ref
	}

	/// Optional tracker anchor required before automatic intake may create issues.
	pub fn team_issue_identifier(&self) -> Option<&str> {
		self.team_issue_identifier.as_deref()
	}

	fn validate(&self) -> Result<()> {
		validate_required_config_string(
			"autonomy.runtime_policy.accepted_objective_id",
			&self.accepted_objective_id,
		)?;
		validate_required_config_string(
			"autonomy.runtime_policy.accepted_objective_version",
			&self.accepted_objective_version,
		)?;
		validate_required_config_string(
			"autonomy.runtime_policy.accepted_policy_id",
			&self.accepted_policy_id,
		)?;
		validate_required_config_string(
			"autonomy.runtime_policy.accepted_policy_version",
			&self.accepted_policy_version,
		)?;
		validate_required_config_string(
			"autonomy.runtime_policy.policy_authority_ref",
			&self.policy_authority_ref,
		)?;

		validate_optional_nonempty_string(
			"autonomy.runtime_policy.team_issue_identifier",
			self.team_issue_identifier.as_deref(),
		)
	}
}

/// Optional JSONL ChatGPT accounts for Codex app-server runs.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCodexAccountsConfig {
	usage_endpoint: Option<String>,
	profile_endpoint: Option<String>,
	refresh_endpoint: Option<String>,
}
impl ProjectCodexAccountsConfig {
	/// Override for ChatGPT usage probes. Defaults to the Codex `/wham/usage` endpoint.
	pub fn usage_endpoint(&self) -> Option<&str> {
		self.usage_endpoint.as_deref()
	}

	/// Override for ChatGPT profile-stat probes. Defaults to Codex `/wham/profiles/me`.
	pub fn profile_endpoint(&self) -> Option<&str> {
		self.profile_endpoint.as_deref()
	}

	/// Override for ChatGPT OAuth refresh. Defaults to the Codex auth token endpoint.
	pub fn refresh_endpoint(&self) -> Option<&str> {
		self.refresh_endpoint.as_deref()
	}

	fn validate(&self) -> Result<()> {
		validate_optional_nonempty_string(
			"codex.accounts.usage_endpoint",
			self.usage_endpoint.as_deref(),
		)?;
		validate_optional_nonempty_string(
			"codex.accounts.profile_endpoint",
			self.profile_endpoint.as_deref(),
		)?;
		validate_optional_nonempty_string(
			"codex.accounts.refresh_endpoint",
			self.refresh_endpoint.as_deref(),
		)?;

		Ok(())
	}
}

/// Optional service-level path overrides.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectPathsConfig {
	repo_root: Option<PathBuf>,
	worktree_root: Option<PathBuf>,
}
impl ProjectPathsConfig {
	fn validate(&self) -> Result<()> {
		if self.repo_root.is_none() {
			eyre::bail!("`paths.repo_root` is required for every Decodex project config.");
		}

		if let Some(repo_root) = self.repo_root.as_deref() {
			validate_nonempty_path("paths.repo_root", repo_root)?;
		}
		if let Some(worktree_root) = self.worktree_root.as_deref() {
			validate_nonempty_path("paths.worktree_root", worktree_root)?;
		}

		Ok(())
	}

	fn resolve_repo_root(&self, config_dir: &Path) -> Result<PathBuf> {
		let Some(path) = self.repo_root.as_deref() else {
			eyre::bail!("`paths.repo_root` is required for every Decodex project config.");
		};
		let repo_root = resolve_relative_path(config_dir, path);
		let repo_root = canonicalize_path_best_effort(&repo_root);

		validate_nonempty_path("paths.repo_root", &repo_root)?;

		Ok(repo_root)
	}

	fn resolve_worktree_root(&self, repo_root: &Path) -> Result<PathBuf> {
		let worktree_root = self.worktree_root.as_deref().map_or_else(
			|| repo_root.join(".worktrees"),
			|path| resolve_relative_path(repo_root, path),
		);

		validate_nonempty_path("paths.worktree_root", &worktree_root)?;

		Ok(worktree_root)
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceConfigDocument {
	service_id: String,
	tracker: ProjectTrackerConfig,
	github: ProjectGitHubConfig,
	#[serde(default)]
	codex: ProjectCodexConfig,
	#[serde(default)]
	autonomy: ProjectAutonomyConfig,
	#[serde(default)]
	privacy_classifier: ProjectPrivacyClassifierConfig,
	#[serde(default)]
	paths: ProjectPathsConfig,
}
impl ServiceConfigDocument {
	fn validate(&self) -> Result<()> {
		validate_service_id("service_id", &self.service_id)?;

		self.tracker.validate()?;
		self.github.validate()?;
		self.codex.validate()?;
		self.autonomy.validate()?;
		self.privacy_classifier.validate()?;
		self.paths.validate()?;

		Ok(())
	}
}

/// Review level for agent runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLevel {
	/// Disable review gates.
	Off,
	/// Require implementation self-check only.
	Basic,
	/// Require self-check plus the Decodex Review checkpoint gate.
	Standard,
	/// Require standard review plus the GitHub Review path.
	Strict,
}
impl ReviewLevel {
	/// Config string for this level.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Off => "off",
			Self::Basic => "basic",
			Self::Standard => "standard",
			Self::Strict => "strict",
		}
	}

	/// Whether this level prompts the implementation self-check.
	pub const fn uses_self_check(self) -> bool {
		!matches!(self, Self::Off)
	}

	/// Whether this level uses the structured Decodex Review checkpoint gate.
	pub const fn requires_review_checkpoint(self) -> bool {
		matches!(self, Self::Standard | Self::Strict)
	}

	/// Whether this level uses the GitHub `@codex review` path.
	pub const fn uses_github_review(self) -> bool {
		matches!(self, Self::Strict)
	}
}

impl Default for ReviewLevel {
	fn default() -> Self {
		default_review_level()
	}
}

mod git_paths;
mod validation;
#[cfg(test)]
use git_paths::path_buf_from_git_line_output;
pub use git_paths::{
	canonical_repo_root_for_checkout, checkouts_share_repository, git_dir_for_checkout,
};
use validation::{
	resolve_secret_env_var, validate_env_var_name, validate_nonempty_path,
	validate_optional_nonempty_string, validate_required_config_string, validate_service_id,
};

const fn default_review_level() -> ReviewLevel {
	ReviewLevel::Strict
}

const fn default_privacy_classifier_timeout_ms() -> u64 {
	1_000
}

fn validate_local_privacy_classifier_endpoint(endpoint: &str) -> Result<()> {
	let url = Url::parse(endpoint)
		.map_err(|error| eyre::eyre!("`privacy_classifier.endpoint` must be a URL: {error}"))?;

	if url.scheme() != "http" {
		eyre::bail!("`privacy_classifier.endpoint` must use `http` on a loopback host.");
	}
	if !url.username().is_empty() || url.password().is_some() {
		eyre::bail!("`privacy_classifier.endpoint` must not contain credentials.");
	}

	let Some(host) = url.host_str() else {
		eyre::bail!("`privacy_classifier.endpoint` must include a loopback host.");
	};

	if !matches!(host, "localhost" | "127.0.0.1" | "::1") {
		eyre::bail!("`privacy_classifier.endpoint` must point to a loopback host, not `{host}`.");
	}

	Ok(())
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
