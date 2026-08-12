#![cfg(unix)]
//! Process-level proof for local exact-base landing authority.

use std::{
	env, fs,
	os::unix::fs::PermissionsExt as _,
	path::{Path, PathBuf},
	process::{Command, Output},
};

use clap as _;
use decodex_cli as _;
use decodex_protocol as _;
use serde as _;
use serde_json as _;
use tempfile::TempDir;
use tokio as _;
use toml_edit as _;

const PR_URL: &str = "https://github.com/acg-box/decodex/pull/123";
const PR_BRANCH: &str = "xv/exact-land";

#[test]
fn local_land_binary_merges_syncs_and_cleans_the_exact_lane() {
	let fixture = Fixture::new();
	let merge = fixture.run_land();

	assert_eq!(merge.len(), 40);
	assert_eq!(git(&fixture.primary, &["rev-parse", "HEAD"]), merge);
	assert_eq!(bare_git(&fixture.origin, &["rev-parse", "refs/heads/main"]), merge);
	assert_eq!(
		git(&fixture.primary, &["show", "-s", "--format=%P", &merge]),
		format!("{} {}", fixture.base, fixture.head)
	);
	assert_eq!(
		git(&fixture.primary, &["rev-parse", &format!("{merge}^{{tree}}")]),
		git(&fixture.primary, &["rev-parse", &format!("{}^{{tree}}", fixture.head)])
	);
	run_checked(Command::new("git").arg("-C").arg(&fixture.primary).args([
		"verify-commit",
		"--raw",
		&merge,
	]));
	assert!(!fixture.checkout.exists());
	assert_bare_ref_absent(&fixture.origin, &format!("refs/heads/{PR_BRANCH}"));
	assert_repository_ref_absent(&fixture.primary, &format!("refs/heads/{PR_BRANCH}"));
	assert!(fixture.hook_marker.is_file(), "pre-push hook should execute");
}

#[test]
fn local_land_recovers_when_remote_main_advanced_after_the_exact_merge() {
	let fixture = Fixture::new();
	let tree = git(&fixture.checkout, &["rev-parse", &format!("{}^{{tree}}", fixture.head)]);
	let record = r#"{"schema":"decodex/commit/2","change":"Land Exact integration candidate","authority":"manual","impact":"compatible"}"#;
	let merge = git(
		&fixture.checkout,
		&["commit-tree", &tree, "-p", &fixture.base, "-p", &fixture.head, "-S", "-m", record],
	);
	git_checked(
		&fixture.checkout,
		&[
			"push",
			&format!("--force-with-lease=refs/heads/main:{}", fixture.base),
			"origin",
			&format!("{merge}:refs/heads/main"),
		],
	);
	let descendant = git(
		&fixture.checkout,
		&[
			"commit-tree",
			&tree,
			"-p",
			&merge,
			"-m",
			r#"{"schema":"decodex/commit/2","change":"authorized descendant","authority":"manual","impact":"compatible"}"#,
		],
	);
	git_checked(&fixture.checkout, &["push", "origin", &format!("{descendant}:refs/heads/main")]);
	fixture.report_merge(&merge);
	fixture.reset_hook_marker();

	assert_eq!(fixture.run_land(), merge);
	assert_eq!(git(&fixture.primary, &["rev-parse", "HEAD"]), descendant);
	assert_eq!(bare_git(&fixture.origin, &["rev-parse", "refs/heads/main"]), descendant);
	run_checked(Command::new("git").arg("-C").arg(&fixture.primary).args([
		"merge-base",
		"--is-ancestor",
		&merge,
		&descendant,
	]));
	assert!(!fixture.checkout.exists());
	assert_bare_ref_absent(&fixture.origin, &format!("refs/heads/{PR_BRANCH}"));
	assert_repository_ref_absent(&fixture.primary, &format!("refs/heads/{PR_BRANCH}"));
	assert!(fixture.hook_marker.is_file(), "pre-push hook should execute");
}

struct Fixture {
	_temp: TempDir,
	origin: PathBuf,
	primary: PathBuf,
	checkout: PathBuf,
	fake_bin: PathBuf,
	reported_merge: PathBuf,
	hook_marker: PathBuf,
	base: String,
	head: String,
}
impl Fixture {
	fn new() -> Self {
		let temp = TempDir::new().expect("temporary directory should create");
		let origin = temp.path().join("origin.git");
		let primary = temp.path().join("repo");
		let checkout = primary.join(".worktrees/exact-land");
		let fake_bin = temp.path().join("bin");
		let reported_merge = temp.path().join("reported-merge");
		let hooks = temp.path().join("hooks");
		let hook_marker = temp.path().join("pre-push-ran");

		fs::create_dir_all(&hooks).expect("hooks directory should create");
		run_checked(Command::new("git").args([
			"init",
			"--bare",
			"--initial-branch=main",
			origin.to_str().expect("origin path should be UTF-8"),
		]));
		run_checked(Command::new("git").args([
			"clone",
			origin.to_str().expect("origin path should be UTF-8"),
			primary.to_str().expect("primary path should be UTF-8"),
		]));
		git_checked(&primary, &["config", "user.name", "Decodex Tests"]);
		git_checked(&primary, &["config", "user.email", "decodex-tests@example.com"]);
		git_checked(
			&primary,
			&["config", "core.hooksPath", hooks.to_str().expect("hooks path should be UTF-8")],
		);
		fs::write(primary.join("README.md"), "base\n").expect("base file should write");
		fs::write(primary.join(".gitignore"), ".worktrees/\n")
			.expect("worktree ignore should write");
		git_checked(&primary, &["add", "README.md", ".gitignore"]);
		git_checked(&primary, &["commit", "-m", "base"]);
		git_checked(&primary, &["push", "-u", "origin", "main"]);
		let base = git(&primary, &["rev-parse", "HEAD"]);

		configure_signing(temp.path(), &primary);
		git_checked(
			&primary,
			&[
				"config",
				&format!("url.{}.insteadOf", origin.to_str().expect("origin path should be UTF-8")),
				"git@github.com:acg-box/decodex.git",
			],
		);
		git_checked(
			&primary,
			&["remote", "set-url", "origin", "git@github.com:acg-box/decodex.git"],
		);
		git_checked(
			&primary,
			&[
				"worktree",
				"add",
				"-b",
				PR_BRANCH,
				checkout.to_str().expect("checkout path should be UTF-8"),
			],
		);
		fs::write(checkout.join("feature.txt"), "feature\n").expect("feature file should write");
		git_checked(&checkout, &["add", "feature.txt"]);
		git_checked(&checkout, &["commit", "-m", "feature"]);
		git_checked(&checkout, &["push", "-u", "origin", PR_BRANCH]);
		let head = git(&checkout, &["rev-parse", "HEAD"]);

		write_pre_push_hook(&hooks, &hook_marker);
		write_fake_gh(&fake_bin, &origin, &reported_merge, &base, &head);

		Self {
			_temp: temp,
			origin,
			primary,
			checkout,
			fake_bin,
			reported_merge,
			hook_marker,
			base,
			head,
		}
	}

	fn command_path(&self) -> std::ffi::OsString {
		let mut paths = vec![self.fake_bin.clone()];

		paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
		env::join_paths(paths).expect("command PATH should join")
	}

	fn run_land(&self) -> String {
		self.reset_hook_marker();
		let output = Command::new(env!("CARGO_BIN_EXE_decodex"))
			.current_dir(&self.checkout)
			.env("PATH", self.command_path())
			.args([
				"land",
				"Exact integration candidate",
				"--manual-authority",
				"--pr",
				PR_URL,
				"--expected-base-oid",
				&self.base,
				"--expected-head-oid",
				&self.head,
			])
			.output()
			.expect("Decodex binary should start");

		assert_success(&output);
		let stdout = String::from_utf8(output.stdout).expect("Decodex output should be UTF-8");
		let prefix = format!("land ok: pr={PR_URL} merge_commit=");
		let suffix = " default_branch=main local_default_branch_synced=true\n";

		stdout
			.strip_prefix(&prefix)
			.and_then(|value| value.strip_suffix(suffix))
			.expect("Decodex should emit the stable landing receipt")
			.to_owned()
	}

	fn reset_hook_marker(&self) {
		if self.hook_marker.exists() {
			fs::remove_file(&self.hook_marker).expect("hook marker should reset");
		}
	}

	fn report_merge(&self, merge: &str) {
		fs::write(&self.reported_merge, merge).expect("reported merge marker should write");
	}
}

fn write_pre_push_hook(hooks: &Path, marker: &Path) {
	let hook = hooks.join("pre-push");
	let body = format!(
		"#!/bin/sh\nset -eu\n{} git-hook pre-push \"${{1:-}}\" \"${{2:-}}\"\n: > {}\n",
		shell_quote(env!("CARGO_BIN_EXE_decodex")),
		shell_quote(marker.to_str().expect("hook marker path should be UTF-8")),
	);

	fs::write(&hook, body).expect("pre-push hook should write");
	let mut permissions = fs::metadata(&hook).expect("hook metadata should read").permissions();

	permissions.set_mode(0o700);
	fs::set_permissions(&hook, permissions).expect("pre-push hook should be executable");
}

fn configure_signing(root: &Path, repository: &Path) {
	let key = root.join("signing-key");

	run_checked(Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-f"]).arg(&key));
	let public_key = fs::read_to_string(key.with_extension("pub")).expect("public key should read");
	let allowed_signers = root.join("allowed-signers");

	fs::write(&allowed_signers, format!("decodex-tests@example.com {}", public_key.trim()))
		.expect("allowed signers should write");
	git_checked(repository, &["config", "gpg.format", "ssh"]);
	git_checked(
		repository,
		&["config", "user.signingkey", key.to_str().expect("key path should be UTF-8")],
	);
	git_checked(
		repository,
		&[
			"config",
			"gpg.ssh.allowedSignersFile",
			allowed_signers.to_str().expect("allowed signers path should be UTF-8"),
		],
	);
}

fn write_fake_gh(fake_bin: &Path, origin: &Path, reported_merge: &Path, base: &str, head: &str) {
	fs::create_dir_all(fake_bin).expect("fake binary directory should create");
	let script = fake_bin.join("gh");
	let origin = shell_quote(origin.to_str().expect("origin path should be UTF-8"));
	let reported_merge =
		shell_quote(reported_merge.to_str().expect("reported merge path should be UTF-8"));
	let git = shell_quote(
		env::var_os("PATH")
			.and_then(|path| {
				env::split_paths(&path)
					.map(|directory| directory.join("git"))
					.find(|candidate| candidate.is_file())
			})
			.and_then(|path| path.to_str().map(str::to_owned))
			.as_deref()
			.expect("Git executable path should be UTF-8"),
	);
	let body = format!(
		"#!/bin/sh\nset -eu\nremote=$({git} --git-dir={origin} rev-parse refs/heads/main)\n\
		 if [ -f {reported_merge} ]; then\n\
		 merge=$(cat {reported_merge})\n\
		 printf '{{\"url\":\"{PR_URL}\",\"state\":\"MERGED\",\"isDraft\":false,\
		 \"isCrossRepository\":false,\"baseRefName\":\"main\",\"baseRefOid\":\"{base}\",\
		 \"headRefName\":\"{PR_BRANCH}\",\"headRefOid\":\"{head}\",\
		 \"mergeCommit\":{{\"oid\":\"%s\"}}}}\\n' \"$merge\"\n\
		 elif [ \"$remote\" = \"{base}\" ]; then\n\
		 printf '%s\\n' '{{\"url\":\"{PR_URL}\",\"state\":\"OPEN\",\"isDraft\":false,\
		 \"isCrossRepository\":false,\"baseRefName\":\"main\",\"baseRefOid\":\"{base}\",\
		 \"headRefName\":\"{PR_BRANCH}\",\"headRefOid\":\"{head}\",\"mergeCommit\":null}}'\n\
		 else\n\
		 printf '{{\"url\":\"{PR_URL}\",\"state\":\"MERGED\",\"isDraft\":false,\
		 \"isCrossRepository\":false,\"baseRefName\":\"main\",\"baseRefOid\":\"{base}\",\
		 \"headRefName\":\"{PR_BRANCH}\",\"headRefOid\":\"{head}\",\
		 \"mergeCommit\":{{\"oid\":\"%s\"}}}}\\n' \"$remote\"\n\
		 fi\n"
	);

	fs::write(&script, body).expect("fake gh should write");
	let mut permissions =
		fs::metadata(&script).expect("fake gh metadata should read").permissions();

	permissions.set_mode(0o700);
	fs::set_permissions(&script, permissions).expect("fake gh should be executable");
}

fn shell_quote(value: &str) -> String {
	format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn git(cwd: &Path, args: &[&str]) -> String {
	let output =
		Command::new("git").arg("-C").arg(cwd).args(args).output().expect("Git should start");

	assert_success(&output);
	String::from_utf8(output.stdout).expect("Git output should be UTF-8").trim().to_owned()
}

fn bare_git(git_dir: &Path, args: &[&str]) -> String {
	let output = Command::new("git")
		.arg("--git-dir")
		.arg(git_dir)
		.args(args)
		.output()
		.expect("bare Git should start");

	assert_success(&output);
	String::from_utf8(output.stdout).expect("bare Git output should be UTF-8").trim().to_owned()
}

fn git_checked(cwd: &Path, args: &[&str]) {
	run_checked(Command::new("git").arg("-C").arg(cwd).args(args));
}

fn assert_bare_ref_absent(git_dir: &Path, reference: &str) {
	let output = Command::new("git")
		.arg("--git-dir")
		.arg(git_dir)
		.args(["show-ref", "--verify", "--quiet", reference])
		.output()
		.expect("Git ref readback should start");

	assert_eq!(
		output.status.code(),
		Some(1),
		"unexpected Git ref readback: {}",
		String::from_utf8_lossy(&output.stderr)
	);
}

fn assert_repository_ref_absent(repository: &Path, reference: &str) {
	let output = Command::new("git")
		.arg("-C")
		.arg(repository)
		.args(["show-ref", "--verify", "--quiet", reference])
		.output()
		.expect("Git ref readback should start");

	assert_eq!(
		output.status.code(),
		Some(1),
		"unexpected Git ref readback: {}",
		String::from_utf8_lossy(&output.stderr)
	);
}

fn run_checked(command: &mut Command) {
	let output = command.output().expect("command should start");

	assert_success(&output);
}

fn assert_success(output: &Output) {
	assert!(
		output.status.success(),
		"command failed with stdout `{}` and stderr `{}`",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);
}
