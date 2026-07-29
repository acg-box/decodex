#![cfg(unix)]
//! Process-level proof that local commit authority traverses the operator Git hook.

use std::{
	env, fs,
	os::unix::fs::PermissionsExt as _,
	path::Path,
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

#[test]
fn local_commit_binary_signs_an_exact_record_through_the_commit_message_hook() {
	let temp = TempDir::new().expect("temporary directory should create");
	let origin = temp.path().join("origin.git");
	let primary = temp.path().join("repo");
	let checkout = primary.join(".worktrees/commit");
	let hooks = temp.path().join("hooks");
	let hook_marker = temp.path().join("commit-msg-ran");

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
	git_checked(&primary, &["add", "README.md"]);
	git_checked(&primary, &["commit", "-m", "base"]);
	git_checked(&primary, &["push", "-u", "origin", "main"]);
	configure_signing(temp.path(), &primary);
	git_checked(
		&primary,
		&[
			"worktree",
			"add",
			"-b",
			"xv/local-commit",
			checkout.to_str().expect("checkout path should be UTF-8"),
		],
	);
	write_commit_message_hook(&hooks, &hook_marker);
	fs::write(checkout.join("feature.txt"), "feature\n").expect("feature file should write");
	git_checked(&checkout, &["add", "feature.txt"]);

	let output = Command::new(env!("CARGO_BIN_EXE_decodex"))
		.current_dir(&checkout)
		.args(["commit", "Exact local candidate", "--manual-authority"])
		.output()
		.expect("Decodex binary should start");

	assert_success(&output);
	assert!(hook_marker.is_file(), "commit-msg hook should execute");
	let commit = git(&checkout, &["rev-parse", "HEAD"]);
	let subject = git(&checkout, &["show", "-s", "--format=%s", &commit]);

	assert_eq!(
		subject,
		r#"{"schema":"decodex/commit/2","change":"Exact local candidate","authority":"manual","impact":"compatible"}"#
	);
	run_checked(Command::new("git").arg("-C").arg(&checkout).args([
		"verify-commit",
		"--raw",
		&commit,
	]));
}

fn write_commit_message_hook(hooks: &Path, marker: &Path) {
	let hook = hooks.join("commit-msg");
	let body = format!(
		"#!/bin/sh\nset -eu\n{} git-hook commit-msg \"$1\"\n: > {}\n",
		shell_quote(env!("CARGO_BIN_EXE_decodex")),
		shell_quote(marker.to_str().expect("hook marker path should be UTF-8")),
	);

	fs::write(&hook, body).expect("commit-msg hook should write");
	let mut permissions = fs::metadata(&hook).expect("hook metadata should read").permissions();

	permissions.set_mode(0o700);
	fs::set_permissions(&hook, permissions).expect("commit-msg hook should be executable");
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

fn shell_quote(value: &str) -> String {
	format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn git(cwd: &Path, args: &[&str]) -> String {
	let output =
		Command::new("git").arg("-C").arg(cwd).args(args).output().expect("Git should start");

	assert_success(&output);
	String::from_utf8(output.stdout).expect("Git output should be UTF-8").trim().to_owned()
}

fn git_checked(cwd: &Path, args: &[&str]) {
	run_checked(Command::new("git").arg("-C").arg(cwd).args(args));
}

fn run_checked(command: &mut Command) {
	let output = command.output().expect("command should start");

	assert_success(&output);
}

fn assert_success(output: &Output) {
	assert!(
		output.status.success(),
		"command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
		output.status.code(),
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr),
	);
}
