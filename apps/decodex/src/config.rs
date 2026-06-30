//! Service configuration for Decodex.

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;
use std::{
	env,
	ffi::OsString,
	fs,
	io::ErrorKind,
	path::{Component, Path, PathBuf},
	process::Command,
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

/// Canonical repository root for the current Git checkout.
pub fn canonical_repo_root_for_checkout(cwd: &Path) -> Result<Option<PathBuf>> {
	let worktree_root = git_absolute_rev_parse(cwd, "show-toplevel")?
		.map(|path| canonicalize_path_best_effort(&path));

	if let Some(shared_repo_root) = shared_repo_root_for_checkout(cwd, worktree_root.as_deref())? {
		return Ok(Some(shared_repo_root));
	}

	Ok(worktree_root)
}

/// Absolute Git administrative directory for the current checkout.
pub fn git_dir_for_checkout(cwd: &Path) -> Result<Option<PathBuf>> {
	Ok(git_absolute_rev_parse(cwd, "git-dir")?.map(|path| canonicalize_path_best_effort(&path)))
}

/// Whether two Git checkouts belong to the same shared repository.
pub fn checkouts_share_repository(a: &Path, b: &Path) -> Result<bool> {
	let a_common_dir = git_absolute_rev_parse(a, "git-common-dir")?
		.map(|path| canonicalize_path_best_effort(&path));
	let b_common_dir = git_absolute_rev_parse(b, "git-common-dir")?
		.map(|path| canonicalize_path_best_effort(&path));

	Ok(a_common_dir.is_some() && a_common_dir == b_common_dir)
}

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

fn shared_repo_root_for_checkout(
	cwd: &Path,
	worktree_root: Option<&Path>,
) -> Result<Option<PathBuf>> {
	let git_dir =
		git_absolute_rev_parse(cwd, "git-dir")?.map(|path| canonicalize_path_best_effort(&path));
	let common_dir = git_absolute_rev_parse(cwd, "git-common-dir")?
		.map(|path| canonicalize_path_best_effort(&path));
	let prefers_shared_repo_root = git_dir.is_some() && git_dir != common_dir;

	if prefers_shared_repo_root {
		return shared_repo_root_for_linked_worktree(cwd, worktree_root, common_dir.as_deref());
	}

	Ok(None)
}

fn shared_repo_root_for_linked_worktree(
	cwd: &Path,
	worktree_root: Option<&Path>,
	common_dir: Option<&Path>,
) -> Result<Option<PathBuf>> {
	let Some(worktree_root) = worktree_root else {
		return Ok(None);
	};
	let Some(common_dir) = common_dir else {
		return Ok(None);
	};

	if let Some(shared_repo_root) =
		repo_root_from_git_worktree_list(cwd, common_dir, worktree_root)?
	{
		return Ok(Some(shared_repo_root));
	}
	if let Some(shared_repo_root) =
		repo_root_from_gitdir_reference_search(common_dir, worktree_root)?
	{
		return Ok(Some(shared_repo_root));
	}

	Ok(None)
}

fn repo_root_from_git_worktree_list(
	cwd: &Path,
	common_dir: &Path,
	worktree_root: &Path,
) -> Result<Option<PathBuf>> {
	for path in git_worktree_roots(cwd)? {
		let path = canonicalize_path_best_effort(&path);

		if path == worktree_root || path == common_dir {
			continue;
		}
		if git_absolute_rev_parse(&path, "git-common-dir")?
			.map(|path| canonicalize_path_best_effort(&path))
			.as_deref()
			!= Some(common_dir)
		{
			continue;
		}
		if git_absolute_rev_parse(&path, "git-dir")?
			.map(|path| canonicalize_path_best_effort(&path))
			.as_deref()
			== Some(common_dir)
		{
			return Ok(Some(path));
		}
	}

	Ok(None)
}

fn repo_root_from_gitdir_reference_search(
	common_dir: &Path,
	worktree_root: &Path,
) -> Result<Option<PathBuf>> {
	let Some(search_root) = nearest_shared_ancestor(common_dir, worktree_root) else {
		return Ok(None);
	};

	find_checkout_root_referencing_common_dir(&search_root, common_dir, worktree_root)
}

fn git_worktree_roots(cwd: &Path) -> Result<Vec<PathBuf>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["worktree", "list", "--porcelain", "-z"])
		.output()?;

	if !output.status.success() {
		return Ok(Vec::new());
	}

	parse_git_worktree_list(&output.stdout)
}

fn parse_git_worktree_list(output: &[u8]) -> Result<Vec<PathBuf>> {
	let mut roots = Vec::new();

	for entry in output.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
		let Some(path_bytes) = entry.strip_prefix(b"worktree ") else {
			continue;
		};
		let Some(path) = path_buf_from_git_bytes(path_bytes)? else {
			continue;
		};

		roots.push(path);
	}

	Ok(roots)
}

fn nearest_shared_ancestor(a: &Path, b: &Path) -> Option<PathBuf> {
	a.ancestors().find(|ancestor| b.starts_with(ancestor)).map(Path::to_path_buf)
}

fn find_checkout_root_referencing_common_dir(
	search_root: &Path,
	common_dir: &Path,
	worktree_root: &Path,
) -> Result<Option<PathBuf>> {
	const MAX_DIRS_TO_SCAN: usize = 4_096;

	let mut stack = vec![search_root.to_path_buf()];
	let mut scanned_dirs = 0_usize;

	while let Some(path) = stack.pop() {
		if scanned_dirs >= MAX_DIRS_TO_SCAN {
			return Ok(None);
		}

		scanned_dirs += 1;

		if path != worktree_root
			&& path != common_dir
			&& git_dir_reference_matches_common_dir_best_effort(&path.join(".git"), common_dir)
		{
			return Ok(Some(path));
		}

		let entries = match fs::read_dir(&path) {
			Ok(entries) => entries,
			Err(error) if error.kind() == ErrorKind::NotFound => continue,
			Err(error) => return Err(error.into()),
		};

		for entry in entries {
			let entry = entry?;
			let child = entry.path();

			if !child.is_dir()
				|| child == common_dir
				|| child.starts_with(common_dir)
				|| child == worktree_root
				|| child.starts_with(worktree_root)
			{
				continue;
			}

			stack.push(child);
		}
	}

	Ok(None)
}

fn git_dir_reference_matches_common_dir_best_effort(dot_git: &Path, common_dir: &Path) -> bool {
	git_dir_reference_matches_common_dir(dot_git, common_dir).unwrap_or_default()
}

fn git_dir_reference_matches_common_dir(dot_git: &Path, common_dir: &Path) -> Result<bool> {
	if dot_git.is_dir() {
		return Ok(fs::canonicalize(dot_git)? == common_dir);
	}
	if !dot_git.is_file() {
		return Ok(false);
	}

	let gitdir = parse_gitdir_file(dot_git)?;

	Ok(fs::canonicalize(gitdir)? == common_dir)
}

fn parse_gitdir_file(dot_git: &Path) -> Result<PathBuf> {
	let contents = fs::read_to_string(dot_git)?;
	let prefix = "gitdir:";
	let Some(gitdir) = contents.lines().find_map(|line| line.strip_prefix(prefix)) else {
		eyre::bail!("Git dir file `{}` is missing a `gitdir:` entry.", dot_git.display());
	};
	let gitdir = gitdir.trim();

	if gitdir.is_empty() {
		eyre::bail!("Git dir file `{}` has an empty `gitdir:` entry.", dot_git.display());
	}

	let gitdir = PathBuf::from(gitdir);

	if gitdir.is_absolute() {
		return Ok(gitdir);
	}

	let Some(parent) = dot_git.parent() else {
		eyre::bail!("Git dir file `{}` must have a parent directory.", dot_git.display());
	};

	Ok(parent.join(gitdir))
}

fn git_absolute_rev_parse(cwd: &Path, mode: &str) -> Result<Option<PathBuf>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["rev-parse", "--path-format=absolute", &format!("--{mode}")])
		.output()?;

	if !output.status.success() {
		return Ok(None);
	}

	path_buf_from_git_line_output(&output.stdout)
}

fn path_buf_from_git_line_output(output: &[u8]) -> Result<Option<PathBuf>> {
	let resolved = output.strip_suffix(b"\n").unwrap_or(output);
	let resolved = resolved.strip_suffix(b"\r").unwrap_or(resolved);

	path_buf_from_git_bytes(resolved)
}

fn canonicalize_path_best_effort(path: &Path) -> PathBuf {
	fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(unix)]
fn path_buf_from_git_bytes(path: &[u8]) -> Result<Option<PathBuf>> {
	if path.is_empty() {
		return Ok(None);
	}

	Ok(Some(PathBuf::from(OsString::from_vec(path.to_vec()))))
}

#[cfg(not(unix))]
fn path_buf_from_git_bytes(path: &[u8]) -> Result<Option<PathBuf>> {
	let resolved = String::from_utf8(path.to_vec())?;

	if resolved.is_empty() {
		return Ok(None);
	}

	Ok(Some(PathBuf::from(resolved)))
}

fn validate_nonempty_path(field_name: &str, value: &Path) -> Result<()> {
	if value.as_os_str().is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}

	Ok(())
}

fn validate_optional_nonempty_string(field_name: &str, value: Option<&str>) -> Result<()> {
	let Some(value) = value else {
		return Ok(());
	};

	if value.trim().is_empty() {
		eyre::bail!("`{field_name}` must not be empty when configured.");
	}

	Ok(())
}

fn validate_required_config_string(field_name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}

	Ok(())
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

fn validate_service_id(field_name: &str, value: &str) -> Result<()> {
	let trimmed = value.trim();

	if trimmed.is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}
	if trimmed != value {
		eyre::bail!("`{field_name}` must not include surrounding whitespace.");
	}

	let mut chars = trimmed.chars();
	let Some(first) = chars.next() else {
		eyre::bail!("`{field_name}` must not be empty.");
	};

	if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
		eyre::bail!("`{field_name}` must start with a lowercase ASCII letter or digit.");
	}
	if chars.any(|character| {
		!(character.is_ascii_lowercase()
			|| character.is_ascii_digit()
			|| matches!(character, '-' | '_'))
	}) {
		eyre::bail!(
			"`{field_name}` must contain only lowercase ASCII letters, digits, hyphens, or underscores."
		);
	}

	Ok(())
}

fn validate_env_var_name(field_name: &str, value: &str) -> Result<()> {
	let trimmed = value.trim();

	if trimmed.is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}
	if trimmed != value {
		eyre::bail!("`{field_name}` must not include surrounding whitespace.");
	}
	if trimmed.starts_with('$') {
		eyre::bail!(
			"`{field_name}` must name the environment variable directly, without a `$` prefix."
		);
	}

	let mut chars = trimmed.chars();
	let Some(first) = chars.next() else {
		eyre::bail!("`{field_name}` must not be empty.");
	};

	if !(first == '_' || first.is_ascii_alphabetic()) {
		eyre::bail!(
			"`{field_name}` must start with an ASCII letter or underscore and contain only ASCII letters, digits, or underscores."
		);
	}
	if chars.any(|character| !(character == '_' || character.is_ascii_alphanumeric())) {
		eyre::bail!("`{field_name}` must contain only ASCII letters, digits, or underscores.");
	}

	Ok(())
}

fn resolve_secret_env_var(field_name: &str, env_var: &str) -> Result<String> {
	validate_env_var_name(field_name, env_var)?;

	let value = match env::var(env_var) {
		Ok(value) if !value.trim().is_empty() => value,
		Ok(_) => {
			if let Some(value) = resolve_secret_launchd_env_var(env_var) {
				value
			} else {
				eyre::bail!(
					"Environment variable `{env_var}` referenced by `{field_name}` must not be blank."
				);
			}
		},
		Err(error) => {
			if let Some(value) = resolve_secret_launchd_env_var(env_var) {
				value
			} else {
				return Err(eyre::eyre!(
					"Failed to read environment variable `{env_var}` referenced by `{field_name}`: {error}"
				));
			}
		},
	};

	if value.trim().is_empty() {
		eyre::bail!(
			"Environment variable `{env_var}` referenced by `{field_name}` must not be blank."
		);
	}

	Ok(value)
}

#[cfg(target_os = "macos")]
fn resolve_secret_launchd_env_var(env_var: &str) -> Option<String> {
	let output = Command::new("/bin/launchctl").args(["getenv", env_var]).output().ok()?;

	if !output.status.success() {
		return None;
	}

	let value = String::from_utf8(output.stdout).ok()?.trim().to_owned();

	if value.is_empty() { None } else { Some(value) }
}

#[cfg(not(target_os = "macos"))]
fn resolve_secret_launchd_env_var(_env_var: &str) -> Option<String> {
	None
}

#[cfg(test)]
mod tests {
	use std::{
		env,
		ffi::OsString,
		fs,
		path::{Path, PathBuf},
		sync::{Mutex, MutexGuard, OnceLock},
	};

	use tempfile::TempDir;

	use crate::{
		config::{self, ReviewLevel, ServiceConfig},
		test_support::hermetic_git_command,
		worktree::WorktreeManager,
	};

	struct TestEnvVarGuard {
		key: String,
		previous: Option<OsString>,
	}
	impl TestEnvVarGuard {
		fn lock() -> MutexGuard<'static, ()> {
			static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

			ENV_LOCK
				.get_or_init(|| Mutex::new(()))
				.lock()
				.expect("env var mutex should not be poisoned")
		}

		fn set(key: &str, value: &str) -> Self {
			let _guard = Self::lock();
			let previous = env::var_os(key);

			unsafe { env::set_var(key, value) };

			Self { key: key.to_owned(), previous }
		}
	}

	impl Drop for TestEnvVarGuard {
		fn drop(&mut self) {
			match self.previous.take() {
				Some(previous) => unsafe { env::set_var(&self.key, previous) },
				None => unsafe { env::remove_var(&self.key) },
			}
		}
	}

	fn write_config_file(dir: &Path, body: &str) -> PathBuf {
		let config_path = dir.join("project.toml");
		let body = body_with_explicit_repo_root(body);

		fs::write(&config_path, body).expect("config should write");

		config_path
	}

	#[test]
	fn loads_service_config_from_project_file_with_explicit_repo_root() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
				command_path = "bin/gh"
			"#,
		);
		let config =
			ServiceConfig::from_path(&config_path).expect("service config should load from disk");
		let canonical_root =
			fs::canonicalize(temp_dir.path()).expect("temp dir should canonicalize");

		assert_eq!(config.service_id(), "pubfi");
		assert_eq!(config.repo_root(), canonical_root);
		assert_eq!(config.worktree_root(), canonical_root.join(".worktrees"));
		assert_eq!(config.workflow_path(), canonical_root.join("WORKFLOW.md"));
		assert_eq!(config.github().token_env_var(), "HOME");
		assert_eq!(config.github().command_path(), Some(canonical_root.join("bin/gh").as_path()));
		assert_eq!(config.codex().review_level(), ReviewLevel::Strict);
	}

	#[test]
	fn loads_service_config_from_project_directory() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#,
		);
		let config = ServiceConfig::from_path(temp_dir.path())
			.expect("service config should load from project directory");

		assert_eq!(config.service_id(), "pubfi");
		assert_eq!(
			ServiceConfig::resolve_project_config_path(temp_dir.path())
				.expect("project directory should resolve"),
			config_path
		);
	}

	fn body_with_explicit_repo_root(body: &str) -> String {
		if body.contains("repo_root") {
			return body.to_owned();
		}
		if body.contains("[paths]") {
			return body.replacen("[paths]", "[paths]\nrepo_root = \".\"", 1);
		}

		format!("{body}\n\n[paths]\nrepo_root = \".\"\n")
	}

	#[test]
	fn loads_service_config_with_relative_worktree_override() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[paths]
				worktree_root = "var/worktrees"
			"#,
		);
		let config =
			ServiceConfig::from_path(&config_path).expect("service config should load from disk");
		let canonical_root =
			fs::canonicalize(temp_dir.path()).expect("temp dir should canonicalize");

		assert_eq!(config.worktree_root(), canonical_root.join("var/worktrees"));
	}

	#[test]
	fn loads_service_config_from_external_project_file_with_explicit_repo_root() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let repo_root = temp_dir.path().join("target-repo");
		let config_dir = temp_dir.path().join("codex/decodex/projects/rsnap");
		let config_path = config_dir.join("project.toml");

		fs::create_dir_all(&repo_root).expect("repo root should exist");
		fs::create_dir_all(&config_dir).expect("config dir should exist");
		fs::write(
			&config_path,
			r#"
				service_id = "rsnap"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[paths]
				repo_root = "../../../../target-repo"
				worktree_root = "lanes"
			"#,
		)
		.expect("centralized config should write");

		let config =
			ServiceConfig::from_path(&config_path).expect("centralized config should load");
		let canonical_root = fs::canonicalize(&repo_root).expect("repo root should canonicalize");

		assert_eq!(config.service_id(), "rsnap");
		assert_eq!(config.repo_root(), canonical_root);
		assert_eq!(config.worktree_root(), canonical_root.join("lanes"));
		assert_eq!(
			config.workflow_path(),
			fs::canonicalize(&config_dir)
				.expect("config dir should canonicalize")
				.join("WORKFLOW.md")
		);
	}

	#[test]
	fn rejects_project_config_with_nonstandard_file_name() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = temp_dir.path().join("rsnap.toml");

		fs::write(&config_path, "").expect("config should write");

		let error = ServiceConfig::from_path(&config_path)
			.expect_err("nonstandard config file name should fail");

		assert!(
			error.to_string().contains("project.toml"),
			"error should explain the fixed config file name: {error:?}"
		);
	}

	#[test]
	fn external_project_config_requires_explicit_repo_root() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = temp_dir.path().join("project.toml");

		fs::write(
			&config_path,
			r#"
				service_id = "rsnap"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#,
		)
		.expect("centralized config should write");

		let error =
			ServiceConfig::from_path(&config_path).expect_err("repo_root should be required");

		assert!(
			error.to_string().contains("paths.repo_root"),
			"error should explain the missing explicit repo root: {error:?}"
		);
	}

	#[test]
	fn parses_codex_review_levels() {
		for (case_name, codex_body, expected_level) in [
			("default strict level", "", ReviewLevel::Strict),
			("explicit off level", r#"review = "off""#, ReviewLevel::Off),
			("explicit basic level", r#"review = "basic""#, ReviewLevel::Basic),
			("explicit standard level", r#"review = "standard""#, ReviewLevel::Standard),
			("explicit strict level", r#"review = "strict""#, ReviewLevel::Strict),
		] {
			let temp_dir = TempDir::new().expect("temp dir should exist");
			let config_path = write_config_file(
				temp_dir.path(),
				&format!(
					r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[codex]
				{codex_body}
			"#
				),
			);
			let config = ServiceConfig::from_path(&config_path).expect(case_name);

			assert_eq!(config.codex().review_level(), expected_level);
		}
	}

	#[test]
	fn parses_autonomy_objective_and_policy_references() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[autonomy]
				auto_promote = true
				auto_intake = true

				[autonomy.runtime_policy]
				accepted_objective_id = "quality-autonomy"
				accepted_objective_version = "1"
				accepted_policy_id = "pubfi-autonomy-policy"
				accepted_policy_version = "7"
				policy_authority_ref = "decodex.runtime_policy:pubfi-autonomy-policy@7"
				team_issue_identifier = "PUB-1000"
			"#,
		);
		let config =
			ServiceConfig::from_path(&config_path).expect("service config should load from disk");
		let autonomy = config.autonomy();
		let runtime_policy =
			autonomy.runtime_policy().expect("runtime policy references should parse");

		assert!(autonomy.auto_promote());
		assert!(autonomy.auto_intake());
		assert_eq!(runtime_policy.accepted_objective_id(), "quality-autonomy");
		assert_eq!(runtime_policy.accepted_objective_version(), "1");
		assert_eq!(runtime_policy.accepted_policy_id(), "pubfi-autonomy-policy");
		assert_eq!(runtime_policy.accepted_policy_version(), "7");
		assert_eq!(
			runtime_policy.policy_authority_ref(),
			"decodex.runtime_policy:pubfi-autonomy-policy@7"
		);
		assert_eq!(runtime_policy.team_issue_identifier(), Some("PUB-1000"));
	}

	#[test]
	fn autonomy_config_defaults_to_latent_only() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#,
		);
		let config =
			ServiceConfig::from_path(&config_path).expect("service config should load from disk");

		assert!(!config.autonomy().auto_promote());
		assert!(!config.autonomy().auto_intake());
		assert!(config.autonomy().runtime_policy().is_none());
	}

	#[test]
	fn rejects_autonomy_execution_flags_without_required_authority_references() {
		for (case_name, autonomy_body, expected_error) in [
			(
				"auto promote needs runtime policy refs",
				r#"
				[autonomy]
				auto_promote = true
				"#,
				"runtime_policy",
			),
			(
				"auto intake needs auto promote",
				r#"
				[autonomy]
				auto_intake = true
				"#,
				"auto_promote",
			),
			(
				"auto intake needs tracker anchor",
				r#"
				[autonomy]
				auto_promote = true
				auto_intake = true

				[autonomy.runtime_policy]
				accepted_objective_id = "quality-autonomy"
				accepted_objective_version = "1"
				accepted_policy_id = "pubfi-autonomy-policy"
				accepted_policy_version = "7"
				policy_authority_ref = "decodex.runtime_policy:pubfi-autonomy-policy@7"
				"#,
				"team_issue_identifier",
			),
		] {
			let temp_dir = TempDir::new().expect("temp dir should exist");
			let config_path = write_config_file(
				temp_dir.path(),
				&format!(
					r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				{autonomy_body}
			"#
				),
			);
			let error = ServiceConfig::from_path(&config_path).expect_err(case_name);

			assert!(
				error.to_string().contains(expected_error),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_autonomy_embedded_policy_bodies_and_execution_budgets() {
		for removed_field in [
			"objective_body",
			"policy_body",
			"allowed_signal_kinds",
			"allowed_surfaces",
			"validation_gates",
			"cooldown_seconds",
			"write_budget",
		] {
			let temp_dir = TempDir::new().expect("temp dir should exist");
			let config_path = write_config_file(
				temp_dir.path(),
				&format!(
					r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[autonomy]
				auto_promote = false

				[autonomy.runtime_policy]
				accepted_objective_id = "quality-autonomy"
				accepted_objective_version = "1"
				accepted_policy_id = "pubfi-autonomy-policy"
				accepted_policy_version = "7"
				policy_authority_ref = "decodex.runtime_policy:pubfi-autonomy-policy@7"
				{removed_field} = "must-live-in-runtime-authority"
			"#
				),
			);
			let error = ServiceConfig::from_path(&config_path)
				.expect_err("embedded autonomy authority should be rejected");

			assert!(
				error.to_string().contains(removed_field),
				"error should identify rejected field {removed_field}: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_legacy_codex_review_fields() {
		for (removed_field, removed_value) in
			[("external_review_enabled", "false"), ("internal_review_mode", "\"prompt\"")]
		{
			let temp_dir = TempDir::new().expect("temp dir should exist");
			let config_path = write_config_file(
				temp_dir.path(),
				&format!(
					r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[codex]
				{removed_field} = {removed_value}
			"#
				),
			);
			let error = ServiceConfig::from_path(&config_path)
				.expect_err("legacy codex review field should be rejected");

			assert!(
				error.to_string().contains(removed_field),
				"error should identify removed field {removed_field}: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_removed_codex_goal_field() {
		let removed_field = ["goal", "support"].join("_");

		for removed_value in ["auto", "required", "off"] {
			let temp_dir = TempDir::new().expect("temp dir should exist");
			let config_path = write_config_file(
				temp_dir.path(),
				&format!(
					r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[codex]
				{removed_field} = "{removed_value}"
			"#
				),
			);
			let error = ServiceConfig::from_path(&config_path)
				.expect_err("removed goal field should be rejected");

			assert!(
				error.to_string().contains(&removed_field),
				"unexpected error for removed value `{removed_value}`: {error:?}"
			);
		}
	}

	#[test]
	fn project_privacy_classifier_defaults_to_disabled() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#,
		);
		let config =
			ServiceConfig::from_path(&config_path).expect("service config should load from disk");

		assert_eq!(config.privacy_classifier().endpoint(), None);
		assert_eq!(config.privacy_classifier().timeout_ms(), 1_000);
	}

	#[test]
	fn parses_loopback_privacy_classifier_endpoint() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[privacy_classifier]
				endpoint = "http://127.0.0.1:9123/classify"
				timeout_ms = 250
			"#,
		);
		let config =
			ServiceConfig::from_path(&config_path).expect("service config should load from disk");

		assert_eq!(config.privacy_classifier().endpoint(), Some("http://127.0.0.1:9123/classify"));
		assert_eq!(config.privacy_classifier().timeout_ms(), 250);
	}

	#[test]
	fn rejects_remote_privacy_classifier_endpoint() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[privacy_classifier]
				endpoint = "https://example.com/classify"
			"#,
		);
		let error = ServiceConfig::from_path(&config_path)
			.expect_err("remote classifier endpoints should be rejected");

		assert!(
			error.to_string().contains("loopback"),
			"error should explain local-only classifier routing: {error:?}"
		);
	}

	#[test]
	fn parses_codex_accounts_settings() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
					api_key_env_var = "HOME"

					[github]
					token_env_var = "HOME"

						[codex.accounts]
						usage_endpoint = "http://127.0.0.1:1234/wham/usage"
						profile_endpoint = "http://127.0.0.1:1234/wham/profiles/me"
						refresh_endpoint = "http://127.0.0.1:1234/oauth/token"
					"#,
		);
		let config = ServiceConfig::from_path(&config_path).expect("accounts should parse");
		let accounts = config.codex().accounts().expect("accounts should be configured");

		assert_eq!(accounts.usage_endpoint(), Some("http://127.0.0.1:1234/wham/usage"));
		assert_eq!(accounts.profile_endpoint(), Some("http://127.0.0.1:1234/wham/profiles/me"));
		assert_eq!(accounts.refresh_endpoint(), Some("http://127.0.0.1:1234/oauth/token"));
	}

	#[test]
	fn rejects_removed_project_scoped_codex_account_fields() {
		for (case_name, removed_field) in [
			("project-scoped account selection", r#"fixed_account = "primary@example.com""#),
			("legacy account path override", r#"path = "accounts/codex-auth.jsonl""#),
		] {
			let temp_dir = TempDir::new().expect("temp dir should exist");
			let config_path = write_config_file(
				temp_dir.path(),
				&format!(
					r#"
				service_id = "pubfi"

				[tracker]
					api_key_env_var = "HOME"

					[github]
					token_env_var = "HOME"

					[codex.accounts]
					{removed_field}
				"#
				),
			);
			let error = ServiceConfig::from_path(&config_path).expect_err(case_name);

			assert!(
				error.to_string().contains(
					removed_field
						.split_once(" = ")
						.expect("removed field assignment should include a separator")
						.0
				),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_unknown_codex_review_level() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[codex]
				review = "prompt_only"
			"#,
		);
		let error =
			ServiceConfig::from_path(&config_path).expect_err("unknown review level should fail");

		assert!(error.to_string().contains("prompt_only"));
	}

	#[test]
	fn rejects_empty_github_token_env_var() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = write_config_file(
			temp_dir.path(),
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = ""
			"#,
		);
		let error = ServiceConfig::from_path(&config_path)
			.expect_err("empty github token env-var should be rejected");

		assert!(error.to_string().contains("github.token_env_var"));
	}

	#[test]
	fn rejects_blank_secret_env_var_values_when_resolving() {
		#[derive(Clone, Copy)]
		enum SecretTarget {
			Github,
			Tracker,
		}

		for (case_name, env_var, env_value, target) in [
			(
				"empty github token env-var value",
				"DECODEX_TEST_EMPTY_GITHUB_TOKEN",
				"",
				SecretTarget::Github,
			),
			(
				"whitespace-only github token env-var value",
				"DECODEX_TEST_BLANK_GITHUB_TOKEN",
				"   ",
				SecretTarget::Github,
			),
			(
				"whitespace-only tracker api key env-var value",
				"DECODEX_TEST_BLANK_TRACKER_API_KEY",
				"   ",
				SecretTarget::Tracker,
			),
		] {
			let _guard = TestEnvVarGuard::set(env_var, env_value);
			let temp_dir = TempDir::new().expect("temp dir should exist");
			let config_path = write_config_file(
				temp_dir.path(),
				&format!(
					r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "{}"

				[github]
				token_env_var = "{}"
			"#,
					match target {
						SecretTarget::Github => "HOME",
						SecretTarget::Tracker => env_var,
					},
					match target {
						SecretTarget::Github => env_var,
						SecretTarget::Tracker => "HOME",
					},
				),
			);
			let config =
				ServiceConfig::from_path(&config_path).expect("service config should parse");
			let error = match target {
				SecretTarget::Github => config.github().resolve_token(),
				SecretTarget::Tracker => config.tracker().resolve_api_key(),
			}
			.expect_err(case_name);

			assert!(
				error.to_string().contains("must not be blank"),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_invalid_service_ids() {
		for (case_name, service_id, expected) in [
			("empty service_id", "", "service_id"),
			(
				"service_id with non-slug characters",
				"pub:fi",
				"lowercase ASCII letters, digits, hyphens, or underscores",
			),
		] {
			let temp_dir = TempDir::new().expect("temp dir should exist");
			let config_path = write_config_file(
				temp_dir.path(),
				&format!(
					r#"
				service_id = "{service_id}"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#
				),
			);
			let error = ServiceConfig::from_path(&config_path).expect_err(case_name);

			assert!(
				error.to_string().contains(expected),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[cfg(unix)]
	#[test]
	fn git_path_output_preserves_non_utf8_bytes() {
		let path = super::path_buf_from_git_line_output(b"/tmp/\xFFlane\n")
			.expect("git path output should parse")
			.expect("git path output should not be empty");

		assert_eq!(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()), b"/tmp/\xFFlane");
	}

	#[test]
	fn canonical_repo_root_for_checkout_prefers_shared_repo_root_for_linked_worktree() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let repo_root = temp_dir.path().join("target-repo");
		let worktree_root = repo_root.join(".worktrees");

		fs::create_dir_all(&repo_root).expect("repo root should exist");
		fs::create_dir_all(&worktree_root).expect("worktree root should exist");

		assert!(
			hermetic_git_command()
				.args(["init", "-b", "main"])
				.current_dir(temp_dir.path())
				.arg(&repo_root)
				.status()
				.expect("git init should run")
				.success()
		);
		assert!(
			hermetic_git_command()
				.args(["config", "user.name", "Decodex Tests"])
				.current_dir(&repo_root)
				.status()
				.expect("git config should run")
				.success()
		);
		assert!(
			hermetic_git_command()
				.args(["config", "user.email", "decodex-tests@example.com"])
				.current_dir(&repo_root)
				.status()
				.expect("git config should run")
				.success()
		);
		assert!(
			hermetic_git_command()
				.args(["config", "commit.gpgsign", "false"])
				.current_dir(&repo_root)
				.status()
				.expect("git config should run")
				.success()
		);

		fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");

		assert!(
			hermetic_git_command()
				.args(["add", "README.md"])
				.current_dir(&repo_root)
				.status()
				.expect("git add should run")
				.success()
		);
		assert!(
			hermetic_git_command()
				.args(["commit", "-m", "seed repo"])
				.current_dir(&repo_root)
				.status()
				.expect("git commit should run")
				.success()
		);

		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let worktree = manager.ensure_worktree("XY-251", false).expect("worktree should create");
		let canonical_repo_root =
			fs::canonicalize(&repo_root).expect("repo root should canonicalize");

		assert_eq!(
			config::canonical_repo_root_for_checkout(&worktree.path)
				.expect("canonical repo root should resolve")
				.expect("linked worktree should expose a canonical repo root"),
			canonical_repo_root
		);
	}
}
