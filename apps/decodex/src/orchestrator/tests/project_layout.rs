mod service_config;
mod workflow_markdown;

pub(super) use self::{
	service_config::{
		load_service_config, sample_service_config_toml,
		sample_service_config_toml_with_github_command_path, service_config_dir,
		service_config_path, service_config_toml_for_config,
		service_config_toml_for_config_with_github_command_path,
		service_config_with_github_token_env_var,
		service_config_with_github_token_env_var_and_command_path,
		service_config_with_review_level, service_workflow_path, write_service_config,
	},
	workflow_markdown::{profile_scoped_workflow_markdown, sample_workflow_markdown},
};

use crate::orchestrator::tests::{self, ReviewLevel, ServiceConfig, TempDir, WorkflowDocument, fs};

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
