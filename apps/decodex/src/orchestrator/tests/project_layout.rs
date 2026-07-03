use crate::orchestrator::tests::{
	self, Path, PathBuf, ReviewLevel, ServiceConfig, TEST_PROJECT_CONFIG_FILE, TempDir,
	WorkflowDocument, fs,
};

pub(super) fn temp_project_layout() -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_and_read_first(
		"pubfi",
		&[],
		"Follow the repository policy.\n",
	)
}

pub(super) fn sample_workflow() -> WorkflowDocument {
	temp_project_layout().2
}

pub(super) fn write_service_config(repo_root: &Path, contents: &str) {
	fs::create_dir_all(service_config_dir(repo_root)).expect("service config dir should exist");

	let contents =
		contents.replace("repo_root = \".\"", &format!("repo_root = \"{}\"", repo_root.display()));

	fs::write(service_config_path(repo_root), contents).expect("service config should write");
}

pub(super) fn load_service_config(repo_root: &Path) -> ServiceConfig {
	ServiceConfig::from_path(service_config_path(repo_root)).expect("service config should load")
}

pub(super) fn service_config_path(repo_root: &Path) -> PathBuf {
	service_config_dir(repo_root).join(TEST_PROJECT_CONFIG_FILE)
}

pub(super) fn service_config_dir(repo_root: &Path) -> PathBuf {
	repo_root
		.parent()
		.expect("repo root should have temp parent")
		.join(".codex/decodex/projects/project")
}

pub(super) fn service_workflow_path(repo_root: &Path) -> PathBuf {
	service_config_dir(repo_root).join("WORKFLOW.md")
}

pub(super) fn sample_service_config_toml(
	service_id: &str,
	tracker_api_key_env_var: &str,
	github_token_env_var: &str,
	worktree_root: Option<&Path>,
	review_level: ReviewLevel,
) -> String {
	sample_service_config_toml_with_github_command_path(
		service_id,
		tracker_api_key_env_var,
		github_token_env_var,
		worktree_root,
		review_level,
		None,
	)
}

pub(super) fn sample_service_config_toml_with_github_command_path(
	service_id: &str,
	tracker_api_key_env_var: &str,
	github_token_env_var: &str,
	worktree_root: Option<&Path>,
	review_level: ReviewLevel,
	github_command_path: Option<&Path>,
) -> String {
	let mut toml = format!(
		r#"service_id = "{service_id}"

[tracker]
api_key_env_var = "{tracker_api_key_env_var}"

[github]
token_env_var = "{github_token_env_var}"
"#
	);

	if let Some(github_command_path) = github_command_path {
		toml.push_str(&format!("command_path = \"{}\"\n", github_command_path.display()));
	}

	if review_level != ReviewLevel::Strict {
		toml.push_str("\n\n[codex]\n");
		toml.push_str(&format!("review = \"{}\"\n", review_level.as_str()));
	}

	toml.push_str(
		r#"

[paths]
repo_root = "."
"#,
	);

	if let Some(worktree_root) = worktree_root {
		toml.push_str(&format!("worktree_root = \"{}\"\n", worktree_root.display()));
	}

	toml
}

pub(super) fn service_config_toml_for_config(
	config: &ServiceConfig,
	github_token_env_var: &str,
	review_level: ReviewLevel,
) -> String {
	service_config_toml_for_config_with_github_command_path(
		config,
		github_token_env_var,
		review_level,
		config.github().command_path(),
	)
}

pub(super) fn service_config_toml_for_config_with_github_command_path(
	config: &ServiceConfig,
	github_token_env_var: &str,
	review_level: ReviewLevel,
	github_command_path: Option<&Path>,
) -> String {
	let default_worktree_root = config.repo_root().join(".worktrees");
	let worktree_root =
		(config.worktree_root() != default_worktree_root).then_some(config.worktree_root());

	sample_service_config_toml_with_github_command_path(
		config.service_id(),
		config.tracker().api_key_env_var(),
		github_token_env_var,
		worktree_root,
		review_level,
		github_command_path,
	)
}

pub(super) fn service_config_with_github_token_env_var(
	config: &ServiceConfig,
	token_env_var: &str,
) -> ServiceConfig {
	write_service_config(
		config.repo_root(),
		&service_config_toml_for_config(config, token_env_var, config.codex().review_level()),
	);

	load_service_config(config.repo_root())
}

pub(super) fn service_config_with_github_token_env_var_and_command_path(
	config: &ServiceConfig,
	token_env_var: &str,
	github_command_path: &Path,
) -> ServiceConfig {
	write_service_config(
		config.repo_root(),
		&service_config_toml_for_config_with_github_command_path(
			config,
			token_env_var,
			config.codex().review_level(),
			Some(github_command_path),
		),
	);

	load_service_config(config.repo_root())
}

pub(super) fn service_config_with_review_level(
	config: &ServiceConfig,
	review_level: ReviewLevel,
) -> ServiceConfig {
	write_service_config(
		config.repo_root(),
		&service_config_toml_for_config_with_github_command_path(
			config,
			config.github().token_env_var(),
			review_level,
			config.github().command_path(),
		),
	);

	load_service_config(config.repo_root())
}

#[allow(dead_code)]
pub(super) fn temp_project_layout_with_tracker_project_slug(
	_project_slug: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_and_read_first(
		"pubfi",
		&[],
		"Follow the repository policy.\n",
	)
}

pub(super) fn temp_project_layout_with_read_first(
	read_first_files: &[(&str, &str)],
	workflow_body: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_and_read_first(
		"pubfi",
		read_first_files,
		workflow_body,
	)
}

pub(super) fn temp_project_layout_with_max_turns(
	max_turns: u32,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_max_turns_and_read_first(
		"pubfi",
		max_turns,
		&[],
		"Follow the repository policy.\n",
	)
}

pub(super) fn temp_project_layout_with_tracker_project_slug_and_read_first(
	_project_slug: &str,
	read_first_files: &[(&str, &str)],
	workflow_body: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_max_turns_and_read_first(
		"pubfi",
		1,
		read_first_files,
		workflow_body,
	)
}

pub(super) fn temp_project_layout_with_tracker_project_slug_max_turns_and_read_first(
	_project_slug: &str,
	max_turns: u32,
	read_first_files: &[(&str, &str)],
	workflow_body: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let repo_root = temp_dir.path().join("target-repo");
	let read_first_paths = read_first_files.iter().map(|(path, _)| *path).collect::<Vec<_>>();

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(repo_root.join(".worktrees")).expect("worktree root should exist");
	fs::create_dir_all(service_config_dir(&repo_root)).expect("service config dir should exist");

	for (relative_path, contents) in read_first_files {
		let absolute_path = repo_root.join(relative_path);

		if let Some(parent) = absolute_path.parent() {
			fs::create_dir_all(parent).expect("read_first parent should exist");
		}

		fs::write(absolute_path, contents).expect("read_first file should exist");
	}

	fs::write(
		service_workflow_path(&repo_root),
		sample_workflow_markdown("pubfi", &read_first_paths, workflow_body, max_turns),
	)
	.expect("workflow should exist");
	fs::write(repo_root.join("README.md"), "test repo\n").expect("tracked repo file should exist");

	write_service_config(
		&repo_root,
		&sample_service_config_toml("pubfi", "HOME", "HOME", None, ReviewLevel::Strict),
	);

	tests::git_status_success(&repo_root, &["init", "-b", "main"]);
	tests::git_status_success(&repo_root, &["config", "user.name", "Decodex Tests"]);
	tests::git_status_success(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	tests::git_status_success(&repo_root, &["config", "commit.gpgsign", "false"]);
	tests::git_status_success(&repo_root, &["add", "."]);
	tests::git_status_success(&repo_root, &["commit", "-m", "bootstrap repo"]);

	let config = load_service_config(&repo_root);
	let workflow =
		WorkflowDocument::from_path(config.workflow_path()).expect("workflow should load");

	(temp_dir, config, workflow)
}

pub(super) fn temp_project_layout_with_workflow_markdown(
	workflow_markdown: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let repo_root = temp_dir.path().join("target-repo");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(repo_root.join(".worktrees")).expect("worktree root should exist");
	fs::create_dir_all(service_config_dir(&repo_root)).expect("service config dir should exist");
	fs::write(service_workflow_path(&repo_root), workflow_markdown).expect("workflow should exist");
	fs::write(repo_root.join("README.md"), "test repo\n").expect("tracked repo file should exist");

	write_service_config(
		&repo_root,
		&sample_service_config_toml("pubfi", "HOME", "HOME", None, ReviewLevel::Strict),
	);

	tests::git_status_success(&repo_root, &["init", "-b", "main"]);
	tests::git_status_success(&repo_root, &["config", "user.name", "Decodex Tests"]);
	tests::git_status_success(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	tests::git_status_success(&repo_root, &["config", "commit.gpgsign", "false"]);
	tests::git_status_success(&repo_root, &["add", "."]);
	tests::git_status_success(&repo_root, &["commit", "-m", "bootstrap repo"]);

	let config = load_service_config(&repo_root);
	let workflow =
		WorkflowDocument::from_path(config.workflow_path()).expect("workflow should load");

	(temp_dir, config, workflow)
}

pub(super) fn profile_scoped_workflow_markdown(project_slug: &str) -> String {
	let _ = project_slug;
	let markdown = r#"
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
canonicalize_commands = ["cargo make fmt", "cargo make lint-fix"]
verify_commands = ["cargo make check"]

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

Follow the repository policy.
"#;

	markdown.to_string()
}

pub(super) fn sample_workflow_markdown(
	_project_slug: &str,
	read_first: &[&str],
	workflow_body: &str,
	max_turns: u32,
) -> String {
	let read_first =
		read_first.iter().map(|path| format!("\"{path}\"")).collect::<Vec<_>>().join(", ");
	let context = format!("[context]\nread_first = [{read_first}]");
	let markdown = format!(
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
max_turns = {max_turns}
max_retry_backoff_ms = 300000
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

{context}
+++

{workflow_body}"#
	);

	markdown
}
