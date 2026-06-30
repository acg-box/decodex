//! Downstream `WORKFLOW.md` parsing and validation.

use std::{
	collections::{BTreeSet, HashMap},
	fs,
	path::{Component, Path},
};

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::prelude::eyre;

const FRONTMATTER_DELIMITER: &str = "+++";

/// Parsed downstream workflow document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowDocument {
	frontmatter: WorkflowFrontmatter,
	body: String,
}
impl WorkflowDocument {
	/// Parse a workflow document from Markdown text.
	pub fn parse_markdown(input: &str) -> crate::prelude::Result<Self> {
		let (frontmatter_input, body) = split_frontmatter(input)?;
		let frontmatter = toml::from_str::<WorkflowFrontmatter>(&frontmatter_input)?;

		frontmatter.validate()?;

		Ok(Self { frontmatter, body })
	}

	/// Load a workflow document from the repository root.
	pub fn from_path(path: impl AsRef<Path>) -> crate::prelude::Result<Self> {
		let input = fs::read_to_string(path)?;

		Self::parse_markdown(&input)
	}

	/// Machine-readable frontmatter for orchestration behavior.
	pub fn frontmatter(&self) -> &WorkflowFrontmatter {
		&self.frontmatter
	}

	/// Human-readable Markdown policy body.
	pub fn body(&self) -> &str {
		&self.body
	}

	/// Render the workflow back to Markdown for process-to-process handoff.
	pub fn to_markdown(&self) -> crate::prelude::Result<String> {
		let frontmatter = toml::to_string(&self.frontmatter)?;
		let mut markdown = format!("{FRONTMATTER_DELIMITER}\n{frontmatter}{FRONTMATTER_DELIMITER}");

		if !self.body.is_empty() {
			markdown.push_str("\n\n");
			markdown.push_str(&self.body);
		}

		Ok(markdown)
	}
}

/// Typed TOML frontmatter for a downstream workflow document.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowFrontmatter {
	version: u8,
	tracker: WorkflowTracker,
	agent: WorkflowAgent,
	execution: WorkflowExecution,
	context: WorkflowContext,
}
impl WorkflowFrontmatter {
	/// Contract version.
	pub fn version(&self) -> u8 {
		self.version
	}

	/// Tracker policy for this repository.
	pub fn tracker(&self) -> &WorkflowTracker {
		&self.tracker
	}

	/// Agent defaults for this repository.
	pub fn agent(&self) -> &WorkflowAgent {
		&self.agent
	}

	/// Execution policy for this repository.
	pub fn execution(&self) -> &WorkflowExecution {
		&self.execution
	}

	/// Extra early-load context paths for this repository.
	pub fn context(&self) -> &WorkflowContext {
		&self.context
	}

	fn validate(&self) -> crate::prelude::Result<()> {
		if self.version != 1 {
			eyre::bail!("Unsupported WORKFLOW.md version: {}", self.version);
		}

		validate_non_empty_string_list("tracker.startable_states", &self.tracker.startable_states)?;
		validate_non_empty_string_list("tracker.terminal_states", &self.tracker.terminal_states)?;
		validate_trimmed_non_empty("tracker.in_progress_state", &self.tracker.in_progress_state)?;
		validate_trimmed_non_empty("tracker.success_state", &self.tracker.success_state)?;
		validate_trimmed_non_empty("tracker.failure_state", &self.tracker.failure_state)?;
		validate_trimmed_non_empty("tracker.opt_out_label", &self.tracker.opt_out_label)?;
		validate_trimmed_non_empty(
			"tracker.needs_attention_label",
			&self.tracker.needs_attention_label,
		)?;
		validate_trimmed_non_empty("agent.transport", &self.agent.transport)?;

		if self.execution.max_attempts == 0 {
			eyre::bail!("`execution.max_attempts` must be greater than zero.");
		}
		if self.execution.max_turns == 0 {
			eyre::bail!("`execution.max_turns` must be greater than zero.");
		}
		if self.execution.max_retry_backoff_ms == 0 {
			eyre::bail!("`execution.max_retry_backoff_ms` must be greater than zero.");
		}

		validate_trimmed_non_empty("tracker.completed_state", &self.tracker.completed_state)?;

		if !self.tracker.terminal_states.iter().any(|state| state == &self.tracker.completed_state)
		{
			eyre::bail!("`tracker.completed_state` must be one of `tracker.terminal_states`.");
		}

		self.execution.validate()?;
		self.context.validate()?;

		Ok(())
	}
}

/// Tracker-facing repository policy.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTracker {
	provider: TrackerProvider,
	startable_states: Vec<String>,
	terminal_states: Vec<String>,
	in_progress_state: String,
	success_state: String,
	completed_state: String,
	failure_state: String,
	opt_out_label: String,
	needs_attention_label: String,
}
impl WorkflowTracker {
	/// Tracker provider for this repository.
	pub fn provider(&self) -> TrackerProvider {
		self.provider
	}

	/// States that are eligible for automatic execution.
	pub fn startable_states(&self) -> &[String] {
		&self.startable_states
	}

	/// States that are considered terminal for automatic execution.
	pub fn terminal_states(&self) -> &[String] {
		&self.terminal_states
	}

	/// State used when `decodex` starts work on an issue.
	pub fn in_progress_state(&self) -> &str {
		&self.in_progress_state
	}

	/// State used after a successful run and validation pass.
	pub fn success_state(&self) -> &str {
		&self.success_state
	}

	/// Explicit state used after a successful post-merge closeout.
	pub fn completed_state(&self) -> &str {
		&self.completed_state
	}

	/// State used after a successful post-merge closeout.
	pub fn resolved_completed_state(&self) -> &str {
		&self.completed_state
	}

	/// State used when retries are exhausted.
	pub fn failure_state(&self) -> &str {
		&self.failure_state
	}

	/// Label that disables automation for an issue.
	pub fn opt_out_label(&self) -> &str {
		&self.opt_out_label
	}

	/// Label that marks failed runs needing human attention.
	pub fn needs_attention_label(&self) -> &str {
		&self.needs_attention_label
	}
}

/// Repo-local agent defaults.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowAgent {
	transport: String,
}
impl WorkflowAgent {
	/// App-server transport.
	pub fn transport(&self) -> &str {
		&self.transport
	}
}

/// Repo-local execution and repo-gate policy.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecution {
	max_attempts: u32,
	max_turns: u32,
	max_retry_backoff_ms: u64,
	canonicalize_commands: Vec<String>,
	verify_commands: Vec<String>,
	gate_profiles: HashMap<String, WorkflowGateProfile>,
	workspace_hooks: WorkflowWorkspaceHooks,
}
impl WorkflowExecution {
	/// Maximum automatic attempts before human attention is required.
	pub fn max_attempts(&self) -> u32 {
		self.max_attempts
	}

	/// Maximum same-thread turns per bounded run before Decodex yields cleanly.
	pub fn max_turns(&self) -> u32 {
		self.max_turns
	}

	/// Maximum failure-retry backoff in milliseconds.
	pub fn max_retry_backoff_ms(&self) -> u64 {
		self.max_retry_backoff_ms
	}

	/// Repo canonicalize commands that may rewrite the worktree before verification.
	pub fn canonicalize_commands(&self) -> &[String] {
		&self.canonicalize_commands
	}

	/// Repo verification commands that must pass after canonicalize commands complete.
	pub fn verify_commands(&self) -> &[String] {
		&self.verify_commands
	}

	/// Repo-owned named gate profiles for narrow path-scoped validation.
	pub fn gate_profiles(&self) -> &HashMap<String, WorkflowGateProfile> {
		&self.gate_profiles
	}

	/// Repo-owned workspace lifecycle hooks.
	pub fn workspace_hooks(&self) -> &WorkflowWorkspaceHooks {
		&self.workspace_hooks
	}

	/// Full default repo gate declared directly on `[execution]`.
	pub fn default_repo_gate(&self) -> ResolvedRepoGate<'_> {
		ResolvedRepoGate {
			profile_name: None,
			canonicalize_commands: &self.canonicalize_commands,
			verify_commands: &self.verify_commands,
		}
	}

	/// Resolve the repo gate for a concrete changed-file set.
	pub fn select_repo_gate_for_changed_files(
		&self,
		changed_files: &BTreeSet<String>,
	) -> ResolvedRepoGate<'_> {
		if changed_files.is_empty() {
			return self.default_repo_gate();
		}

		let mut matching_profiles = self
			.gate_profiles
			.iter()
			.filter_map(|(profile_name, profile)| {
				profile.matches_changed_files(changed_files).ok().and_then(|matches| {
					matches.then_some(ResolvedRepoGate {
						profile_name: Some(profile_name.as_str()),
						canonicalize_commands: profile.canonicalize_commands(),
						verify_commands: profile.verify_commands(),
					})
				})
			})
			.collect::<Vec<_>>();

		if matching_profiles.len() == 1 {
			return matching_profiles.remove(0);
		}

		self.default_repo_gate()
	}

	fn validate(&self) -> crate::prelude::Result<()> {
		validate_string_entries("execution.canonicalize_commands", &self.canonicalize_commands)?;
		validate_string_entries("execution.verify_commands", &self.verify_commands)?;

		for (profile_name, profile) in &self.gate_profiles {
			let trimmed = profile_name.trim();

			if trimmed.is_empty() {
				eyre::bail!("`execution.gate_profiles` keys must not be empty.");
			}
			if trimmed != profile_name {
				eyre::bail!(
					"`execution.gate_profiles.{profile_name}` must not include surrounding whitespace."
				);
			}

			profile.validate(profile_name)?;
		}

		self.workspace_hooks.validate()?;

		Ok(())
	}
}

/// Repo-owned workspace lifecycle hooks around linked worktree setup and cleanup.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowWorkspaceHooks {
	after_create_commands: Vec<String>,
	before_remove_commands: Vec<String>,
	timeout_seconds: u64,
}
impl WorkflowWorkspaceHooks {
	/// Commands that run after Decodex creates a new linked worktree for a lane.
	pub fn after_create_commands(&self) -> &[String] {
		&self.after_create_commands
	}

	/// Commands that run before Decodex removes a linked worktree for a lane.
	pub fn before_remove_commands(&self) -> &[String] {
		&self.before_remove_commands
	}

	/// Shared timeout budget, in seconds, for each workspace hook command.
	pub fn timeout_seconds(&self) -> u64 {
		self.timeout_seconds
	}

	fn validate(&self) -> crate::prelude::Result<()> {
		if self.timeout_seconds == 0 {
			eyre::bail!("`execution.workspace_hooks.timeout_seconds` must be greater than zero.");
		}

		validate_string_entries(
			"execution.workspace_hooks.after_create_commands",
			&self.after_create_commands,
		)?;
		validate_string_entries(
			"execution.workspace_hooks.before_remove_commands",
			&self.before_remove_commands,
		)?;

		Ok(())
	}
}

/// Narrow, repo-owned gate profile selected from changed tracked files.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowGateProfile {
	match_mode: WorkflowGateMatchMode,
	paths: Vec<String>,
	canonicalize_commands: Vec<String>,
	verify_commands: Vec<String>,
}
impl WorkflowGateProfile {
	/// Match mode for the profile.
	pub fn match_mode(&self) -> WorkflowGateMatchMode {
		self.match_mode
	}

	/// Repo-relative path patterns covered by this profile.
	pub fn paths(&self) -> &[String] {
		&self.paths
	}

	/// Canonicalize commands for this profile.
	pub fn canonicalize_commands(&self) -> &[String] {
		&self.canonicalize_commands
	}

	/// Verify commands for this profile.
	pub fn verify_commands(&self) -> &[String] {
		&self.verify_commands
	}

	fn validate(&self, profile_name: &str) -> crate::prelude::Result<()> {
		if self.paths.is_empty() {
			eyre::bail!("`execution.gate_profiles.{profile_name}.paths` must not be empty.");
		}
		if self.canonicalize_commands.is_empty() && self.verify_commands.is_empty() {
			eyre::bail!(
				"`execution.gate_profiles.{profile_name}` must declare at least one canonicalize or verify command."
			);
		}

		validate_repo_relative_paths(
			&format!("execution.gate_profiles.{profile_name}.paths"),
			&self.paths,
		)?;

		self.compile_path_set(profile_name)?;

		validate_string_entries(
			&format!("execution.gate_profiles.{profile_name}.canonicalize_commands"),
			&self.canonicalize_commands,
		)?;
		validate_string_entries(
			&format!("execution.gate_profiles.{profile_name}.verify_commands"),
			&self.verify_commands,
		)?;

		Ok(())
	}

	fn matches_changed_files(
		&self,
		changed_files: &BTreeSet<String>,
	) -> crate::prelude::Result<bool> {
		let path_set = self.compile_path_set("runtime")?;

		match self.match_mode {
			WorkflowGateMatchMode::Only => {
				Ok(changed_files.iter().all(|path| path_set.is_match(Path::new(path))))
			},
		}
	}

	fn compile_path_set(&self, profile_name: &str) -> crate::prelude::Result<GlobSet> {
		let mut builder = GlobSetBuilder::new();

		for path in &self.paths {
			let glob = Glob::new(path).map_err(|error| {
				eyre::eyre!(
					"Invalid glob pattern in `execution.gate_profiles.{profile_name}.paths`: `{path}` ({error})"
				)
			})?;

			builder.add(glob);
		}

		builder.build().map_err(|error| {
			eyre::eyre!("Failed to compile `execution.gate_profiles.{profile_name}.paths`: {error}")
		})
	}
}

/// A resolved repo gate ready to execute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedRepoGate<'a> {
	profile_name: Option<&'a str>,
	canonicalize_commands: &'a [String],
	verify_commands: &'a [String],
}
impl<'a> ResolvedRepoGate<'a> {
	/// Optional selected profile name; `None` means the default full gate.
	pub fn profile_name(&self) -> Option<&'a str> {
		self.profile_name
	}

	/// Canonicalize commands selected for this gate run.
	pub fn canonicalize_commands(&self) -> &'a [String] {
		self.canonicalize_commands
	}

	/// Verification commands selected for this gate run.
	pub fn verify_commands(&self) -> &'a [String] {
		self.verify_commands
	}
}

/// Repo-local early-load context.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowContext {
	read_first: Vec<String>,
}
impl WorkflowContext {
	/// Repository-relative files to load before the broader prompt body.
	pub fn read_first(&self) -> &[String] {
		&self.read_first
	}

	fn validate(&self) -> crate::prelude::Result<()> {
		validate_repo_relative_paths("context.read_first", &self.read_first)
	}
}

/// Supported tracker providers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackerProvider {
	/// Linear issue tracking.
	Linear,
}

/// Match semantics for a repo-owned gate profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowGateMatchMode {
	/// The profile applies only when every changed tracked file is covered by its path set.
	Only,
}

fn validate_string_entries(field_name: &str, values: &[String]) -> crate::prelude::Result<()> {
	for value in values {
		let trimmed = value.trim();

		if trimmed.is_empty() {
			eyre::bail!("`{field_name}` entries must not be empty.");
		}
		if trimmed != value {
			eyre::bail!("`{field_name}` entries must not include surrounding whitespace.");
		}
	}

	Ok(())
}

fn validate_repo_relative_paths(field_name: &str, values: &[String]) -> crate::prelude::Result<()> {
	validate_string_entries(field_name, values)?;

	for value in values {
		let path = Path::new(value);

		if path.is_absolute() {
			eyre::bail!("`{field_name}` entries must be repository-relative paths.");
		}
		if !path.components().all(|component| matches!(component, Component::Normal(_))) {
			eyre::bail!(
				"`{field_name}` entries must not contain `.`, `..`, root, or prefix components."
			);
		}
	}

	Ok(())
}

fn split_frontmatter(input: &str) -> crate::prelude::Result<(String, String)> {
	let input = input.trim_start_matches(['\u{feff}', '\n', '\r']);
	let mut lines = input.lines();

	if lines.next() != Some(FRONTMATTER_DELIMITER) {
		eyre::bail!("WORKFLOW.md must begin with TOML frontmatter delimited by `+++`.");
	}

	let mut frontmatter_lines = Vec::new();
	let mut body_lines = Vec::new();
	let mut found_end = false;

	for line in lines {
		if !found_end && line == FRONTMATTER_DELIMITER {
			found_end = true;

			continue;
		}
		if found_end {
			body_lines.push(line);
		} else {
			frontmatter_lines.push(line);
		}
	}

	if !found_end {
		eyre::bail!("WORKFLOW.md frontmatter is missing the closing `+++` delimiter.");
	}

	let body = body_lines.join("\n").trim().to_string();

	Ok((frontmatter_lines.join("\n"), body))
}

fn validate_trimmed_non_empty(field_name: &str, value: &str) -> crate::prelude::Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}
	if value != value.trim() {
		eyre::bail!("`{field_name}` must not include surrounding whitespace.");
	}

	Ok(())
}

fn validate_non_empty_string_list(
	field_name: &str,
	values: &[String],
) -> crate::prelude::Result<()> {
	if values.is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}

	for value in values {
		validate_trimmed_non_empty(&format!("{field_name} entries"), value)?;
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::{collections::BTreeSet, fs};

	use Edit::{Remove, Replace};
	use tempfile::NamedTempFile;

	use crate::{
		prelude::Result,
		workflow::{TrackerProvider, WorkflowDocument, WorkflowGateMatchMode},
	};

	enum Edit<'a> {
		Remove(&'a str),
		Replace(&'a str, &'a str),
	}
	impl Edit<'_> {
		fn apply(&self, markdown: &mut String) {
			match self {
				Self::Remove(needle) => *markdown = markdown.replace(needle, ""),
				Self::Replace(needle, replacement) => {
					*markdown = markdown.replace(needle, replacement);
				},
			}
		}
	}

	#[test]
	fn parses_workflow_document() {
		let document = WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 4
max_retry_backoff_ms = 300000
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]
gate_profiles = {}

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Start with the repo's documented routing entrypoint when one exists.
Use `cargo make`.
			"#,
		)
		.expect("workflow document should parse");

		assert_eq!(document.frontmatter().version(), 1);
		assert_eq!(document.frontmatter().tracker().provider(), TrackerProvider::Linear);
		assert_eq!(document.frontmatter().tracker().completed_state(), "Done");
		assert_eq!(document.frontmatter().execution().max_attempts(), 3);
		assert_eq!(document.frontmatter().execution().max_turns(), 4);
		assert_eq!(document.frontmatter().execution().max_retry_backoff_ms(), 300_000);
		assert_eq!(document.frontmatter().execution().canonicalize_commands(), ["cargo make fmt"]);
		assert_eq!(document.frontmatter().execution().verify_commands(), ["cargo make test"]);
		assert_eq!(
			document.body(),
			"Start with the repo's documented routing entrypoint when one exists.\nUse `cargo make`."
		);
	}

	#[test]
	fn parses_workspace_hooks() {
		let document = WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = ["./scripts/bootstrap-worktree.sh"]
before_remove_commands = ["./scripts/cleanup-worktree.sh"]
timeout_seconds = 45

[context]
read_first = []
+++
			"#,
		)
		.expect("workflow with workspace hooks should parse");
		let hooks = document.frontmatter().execution().workspace_hooks();

		assert_eq!(hooks.after_create_commands(), ["./scripts/bootstrap-worktree.sh"]);
		assert_eq!(hooks.before_remove_commands(), ["./scripts/cleanup-worktree.sh"]);
		assert_eq!(hooks.timeout_seconds(), 45);
	}

	#[test]
	fn rejects_invalid_workspace_hook_config() {
		for (case_name, needle, replacement, expected) in [
			(
				"zero timeout",
				"timeout_seconds = 60",
				"timeout_seconds = 0",
				"`execution.workspace_hooks.timeout_seconds` must be greater than zero",
			),
			(
				"surrounding whitespace",
				"after_create_commands = []",
				r#"after_create_commands = ["  ./scripts/bootstrap-worktree.sh  "]"#,
				"`execution.workspace_hooks.after_create_commands` entries must not include surrounding whitespace",
			),
		] {
			let result = parse_valid_workflow_with(|markdown| {
				*markdown = markdown.replace(needle, replacement);
			});
			let error = result.expect_err(case_name);

			assert!(
				error.to_string().contains(expected),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[test]
	fn parses_named_gate_profile() {
		let document = WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
			"#,
		)
		.expect("workflow with gate profile should parse");
		let profile = document
			.frontmatter()
			.execution()
			.gate_profiles()
			.get("config_subset")
			.expect("config_subset profile should exist");

		assert_eq!(profile.match_mode(), WorkflowGateMatchMode::Only);
		assert_eq!(profile.paths(), ["config/**"]);
		assert_eq!(profile.verify_commands(), ["python3 -c 'print(\"ok\")'"]);
	}

	#[test]
	fn selects_matching_gate_profile_when_all_changed_files_match_profile_paths() {
		let document = WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
			"#,
		)
		.expect("workflow should parse");
		let changed_files = ["config/base.toml", "config/service.toml"]
			.into_iter()
			.map(str::to_owned)
			.collect::<BTreeSet<_>>();
		let selection =
			document.frontmatter().execution().select_repo_gate_for_changed_files(&changed_files);

		assert_eq!(selection.profile_name(), Some("config_subset"));
		assert!(selection.canonicalize_commands().is_empty());
		assert_eq!(selection.verify_commands(), ["python3 -c 'print(\"ok\")'"]);
	}

	#[test]
	fn falls_back_to_full_gate_for_mixed_docs_and_runtime_changes() {
		let document = WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
			"#,
		)
		.expect("workflow should parse");
		let changed_files = ["config/base.toml", "src/orchestrator/git_ops.rs"]
			.into_iter()
			.map(str::to_owned)
			.collect::<BTreeSet<_>>();
		let selection =
			document.frontmatter().execution().select_repo_gate_for_changed_files(&changed_files);

		assert_eq!(selection.profile_name(), None);
		assert_eq!(selection.canonicalize_commands(), ["cargo make fmt"]);
		assert_eq!(selection.verify_commands(), ["cargo make test"]);
	}

	#[test]
	fn falls_back_to_full_gate_for_ambiguous_profile_matches() {
		let document = WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

[execution.gate_profiles.config_prod]
match_mode = "only"
paths = ["config/prod.toml"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
			"#,
		)
		.expect("workflow should parse");
		let changed_files =
			["config/prod.toml"].into_iter().map(str::to_owned).collect::<BTreeSet<_>>();
		let selection =
			document.frontmatter().execution().select_repo_gate_for_changed_files(&changed_files);

		assert_eq!(selection.profile_name(), None);
		assert_eq!(selection.verify_commands(), ["cargo make test"]);
	}

	#[test]
	fn rejects_incomplete_gate_profiles() {
		for (case_name, paths, commands, expected) in [
			(
				"missing paths",
				"[]",
				r#"verify_commands = ["python3 -c 'print(\"ok\")'"]"#,
				"`execution.gate_profiles.config_subset.paths` must not be empty",
			),
			(
				"missing commands",
				r#"["config/**"]"#,
				"verify_commands = []",
				"`execution.gate_profiles.config_subset` must declare at least one canonicalize or verify command",
			),
		] {
			let result = parse_valid_workflow_with(|markdown| {
				*markdown = markdown.replace(
					r#"gate_profiles = {}
canonicalize_commands = []
verify_commands = []
"#,
					&format!(
						r#"
canonicalize_commands = []
verify_commands = []

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = {paths}
canonicalize_commands = []
{commands}

"#,
					),
				);
			});
			let error = result.expect_err(case_name);

			assert!(
				error.to_string().contains(expected),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_gate_profile_paths_that_escape_repo() {
		for (path, expected) in [
			("../config/**", "must not contain `.`, `..`, root, or prefix components"),
			("/tmp/config/**", "must be repository-relative paths"),
		] {
			let result = parse_valid_workflow_with(|markdown| {
				*markdown = markdown.replace(
					r#"gate_profiles = {}
canonicalize_commands = []
verify_commands = []
"#,
					&format!(
						r#"
canonicalize_commands = []
verify_commands = []

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["{path}"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

"#
					),
				);
			});
			let error = result.expect_err("escaping gate profile path should fail");

			assert!(
				error.to_string().contains(expected),
				"unexpected error for `{path}`: {error:?}"
			);
		}
	}

	#[test]
	fn loads_workflow_document_from_path() {
		let file = NamedTempFile::new().expect("temp file should exist");

		fs::write(
			file.path(),
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Read the repo policy first.
			"#,
		)
		.expect("workflow document should be written");

		let document =
			WorkflowDocument::from_path(file.path()).expect("workflow should load from path");

		assert_eq!(document.frontmatter().tracker().completed_state(), "Done");
	}

	#[test]
	fn parses_explicit_completed_state() {
		let document = WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Released", "Canceled"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Released"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Read the repo policy first.
			"#,
		)
		.expect("workflow document should parse");

		assert_eq!(document.frontmatter().tracker().completed_state(), "Released");
		assert_eq!(document.frontmatter().tracker().resolved_completed_state(), "Released");
	}

	#[test]
	fn rejects_invalid_completed_state_contract() {
		for (case_name, edit, expected) in [
			("missing completed_state", Remove("completed_state = \"Done\"\n"), "completed_state"),
			(
				"completed_state outside terminal_states",
				Replace(
					r#"terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done""#,
					r#"terminal_states = ["Released", "Canceled"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done""#,
				),
				"`tracker.completed_state` must be one of `tracker.terminal_states`",
			),
		] {
			let result = parse_valid_workflow_with(|markdown| edit.apply(markdown));
			let error = result.expect_err(case_name);

			assert!(
				error.to_string().contains(expected),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_unknown_workflow_fields() {
		for (case_name, edit, field) in [
			(
				"nested tracker field",
				Replace(
					"needs_attention_label = \"decodex:needs-attention\"",
					"needs_attention_label = \"decodex:needs-attention\"\nunexpected_tracker_key = \"pubfi\"",
				),
				"unexpected_tracker_key",
			),
			(
				"execution field",
				Replace(
					"verify_commands = []",
					"verify_commands = []\nunexpected_execution_field = [\"cargo make test\"]",
				),
				"unexpected_execution_field",
			),
			(
				"top-level table",
				Replace(
					"[context]\nread_first = []",
					"[context]\nread_first = []\n\n[unexpected]\nenabled = true",
				),
				"unexpected",
			),
		] {
			let result = parse_valid_workflow_with(|markdown| edit.apply(markdown));
			let error = result.expect_err(case_name);

			assert!(error.to_string().contains(&format!("unknown field `{field}`")));
		}
	}

	#[test]
	fn rejects_missing_frontmatter() {
		let result = WorkflowDocument::parse_markdown("Read the repo policy first.");

		assert!(result.is_err());
	}

	#[test]
	fn rejects_missing_or_empty_required_workflow_contract() {
		for (case_name, edit, expected) in [
			(
				"missing agent block",
				Remove(
					r#"[agent]
transport = "stdio://"

"#,
				),
				"agent",
			),
			("missing max_attempts", Remove("max_attempts = 3\n"), "max_attempts"),
			(
				"empty terminal states",
				Replace(
					r#"terminal_states = ["Done", "Canceled", "Duplicate"]"#,
					"terminal_states = []",
				),
				"`tracker.terminal_states` must not be empty",
			),
			(
				"blank agent transport",
				Replace(r#"transport = "stdio://""#, r#"transport = """#),
				"`agent.transport` must not be empty",
			),
		] {
			let result = parse_valid_workflow_with(|markdown| edit.apply(markdown));
			let error = result.expect_err(case_name);

			assert!(
				error.to_string().contains(expected),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_blank_required_tracker_policy_values() {
		for (
			field,
			in_progress_state,
			success_state,
			failure_state,
			opt_out_label,
			needs_attention_label,
		) in [
			(
				"in_progress_state",
				"\"\"",
				"\"In Review\"",
				"\"Todo\"",
				"\"decodex:manual-only\"",
				"\"decodex:needs-attention\"",
			),
			(
				"success_state",
				"\"In Progress\"",
				"\"\"",
				"\"Todo\"",
				"\"decodex:manual-only\"",
				"\"decodex:needs-attention\"",
			),
			(
				"failure_state",
				"\"In Progress\"",
				"\"In Review\"",
				"\"\"",
				"\"decodex:manual-only\"",
				"\"decodex:needs-attention\"",
			),
			(
				"opt_out_label",
				"\"In Progress\"",
				"\"In Review\"",
				"\"Todo\"",
				"\"\"",
				"\"decodex:needs-attention\"",
			),
			(
				"needs_attention_label",
				"\"In Progress\"",
				"\"In Review\"",
				"\"Todo\"",
				"\"decodex:manual-only\"",
				"\"\"",
			),
		] {
			let result = WorkflowDocument::parse_markdown(&format!(
				r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = {in_progress_state}
success_state = {success_state}
completed_state = "Done"
failure_state = {failure_state}
opt_out_label = {opt_out_label}
needs_attention_label = {needs_attention_label}

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
				"#,
			));

			assert!(
				result
					.expect_err("blank required tracker value should fail")
					.to_string()
					.contains(&format!("`tracker.{field}` must not be empty"))
			);
		}
	}

	#[test]
	fn rejects_blank_required_policy_entries() {
		for (field, startable_states, terminal_states) in [
			("startable_states", "[\"\"]", "[\"Done\", \"Canceled\", \"Duplicate\"]"),
			("terminal_states", "[\"Todo\"]", "[\"Done\", \"\"]"),
		] {
			let result = WorkflowDocument::parse_markdown(&format!(
				r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = {startable_states}
terminal_states = {terminal_states}
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
				"#,
			));

			assert!(
				result
					.expect_err("blank required tracker entry should fail")
					.to_string()
					.contains(&format!("`tracker.{field} entries` must not be empty"))
			);
		}
	}

	#[test]
	fn rejects_missing_required_workflow_sections_and_fields() {
		for (needle, expected) in [
			("gate_profiles = {}\n", "gate_profiles"),
			(
				r#"[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

"#,
				"workspace_hooks",
			),
			(
				r#"[context]
read_first = []
"#,
				"context",
			),
		] {
			let result = parse_valid_workflow_with(|markdown| {
				*markdown = markdown.replace(needle, "");
			});
			let error = result.expect_err("missing required workflow sections should fail");

			assert!(
				error.to_string().contains(expected),
				"unexpected error for `{expected}`: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_invalid_gate_command_entries() {
		for (case_name, edit, expected) in [
			(
				"blank canonicalize command",
				Replace("canonicalize_commands = []", "canonicalize_commands = [\"\"]"),
				"`execution.canonicalize_commands` entries",
			),
			(
				"untrimmed verify command",
				Replace("verify_commands = []", "verify_commands = [\"  cargo make test  \"]"),
				"`execution.verify_commands` entries",
			),
			(
				"blank profile canonicalize command",
				Replace(
					r#"gate_profiles = {}
canonicalize_commands = []
verify_commands = []
"#,
					r#"
canonicalize_commands = []
verify_commands = []

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = [" "]
verify_commands = ["python3 -c 'print(\"ok\")'"]

"#,
				),
				"`execution.gate_profiles.config_subset.canonicalize_commands` entries",
			),
		] {
			let result = parse_valid_workflow_with(|markdown| edit.apply(markdown));
			let error = result.expect_err(case_name);

			assert!(
				error.to_string().contains(expected),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[test]
	fn rejects_invalid_context_read_first_entries() {
		for (case_name, replacement, expected) in [
			(
				"blank read_first entry",
				"read_first = [\"\"]",
				"`context.read_first` entries must not be empty",
			),
			(
				"parent traversal read_first path",
				"read_first = [\"../secret.md\"]",
				"must not contain `.`, `..`, root, or prefix components",
			),
			(
				"absolute read_first path",
				"read_first = [\"/tmp/secret.md\"]",
				"must be repository-relative paths",
			),
		] {
			let result = parse_valid_workflow_with(|markdown| {
				*markdown = markdown.replace("read_first = []", replacement);
			});
			let error = result.expect_err(case_name);

			assert!(
				error.to_string().contains(expected),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
	}

	#[test]
	fn workflow_document_markdown_round_trips() {
		let document = WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 5
max_turns = 6
max_retry_backoff_ms = 120000
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]
gate_profiles = {}

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = ["docs/index.md", "README.md"]
+++

Read the repo policy first.
Then validate the lane.
			"#,
		)
		.expect("workflow document should parse");
		let reparsed = WorkflowDocument::parse_markdown(
			&document.to_markdown().expect("workflow markdown should render"),
		)
		.expect("rendered workflow should parse");

		assert_eq!(reparsed, document);
	}

	fn parse_valid_workflow_with(rewrite: impl FnOnce(&mut String)) -> Result<WorkflowDocument> {
		let mut markdown = valid_workflow_markdown();

		rewrite(&mut markdown);

		WorkflowDocument::parse_markdown(&markdown)
	}

	fn valid_workflow_markdown() -> String {
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Read the repo policy first.
		"#
		.to_string()
	}
}
