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
	let canonical_root = fs::canonicalize(temp_dir.path()).expect("temp dir should canonicalize");

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
	let canonical_root = fs::canonicalize(temp_dir.path()).expect("temp dir should canonicalize");

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

	let config = ServiceConfig::from_path(&config_path).expect("centralized config should load");
	let canonical_root = fs::canonicalize(&repo_root).expect("repo root should canonicalize");

	assert_eq!(config.service_id(), "rsnap");
	assert_eq!(config.repo_root(), canonical_root);
	assert_eq!(config.worktree_root(), canonical_root.join("lanes"));
	assert_eq!(
		config.workflow_path(),
		fs::canonicalize(&config_dir).expect("config dir should canonicalize").join("WORKFLOW.md")
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

	let error = ServiceConfig::from_path(&config_path).expect_err("repo_root should be required");

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
	let runtime_policy = autonomy.runtime_policy().expect("runtime policy references should parse");

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
		let config = ServiceConfig::from_path(&config_path).expect("service config should parse");
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
	let canonical_repo_root = fs::canonicalize(&repo_root).expect("repo root should canonicalize");

	assert_eq!(
		config::canonical_repo_root_for_checkout(&worktree.path)
			.expect("canonical repo root should resolve")
			.expect("linked worktree should expose a canonical repo root"),
		canonical_repo_root
	);
}
