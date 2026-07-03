use std::{ffi::OsString, path::Path};

use clap::{Parser, error::ErrorKind};

use crate::cli::{
	self, AppCommand, Cli, Command,
	docs_okf_commands::{
		DocsCommand, DocsGraphCommand, DocsSubcommand, OkfCommand, OkfFindCommand, OkfFindFilters,
		OkfInitCommand, OkfInitProfileArg, OkfSubcommand,
	},
	git_hook_commands::{GitHookCommand, GitHookSubcommand},
	manual_commands::{CommitCommand, LandCommand},
};

#[test]
fn parses_app_command() {
	let cli = Cli::parse_from(["decodex", "app"]);

	assert!(matches!(cli.command, Command::App(AppCommand { bundle: None, new: false })));
}

#[test]
fn rejects_radar_as_runtime_subcommand() {
	let error = Cli::try_parse_from(["decodex", "radar"]).expect_err("radar is a standalone tool");

	assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
}

#[test]
fn parses_app_bundle_and_new_instance() {
	let cli =
		Cli::parse_from(["decodex", "app", "--bundle", "target/decodex-app/Decodex.app", "--new"]);

	assert!(matches!(
		cli.command,
		Command::App(AppCommand {
			bundle: Some(bundle),
			new: true,
		}) if bundle == Path::new("target/decodex-app/Decodex.app")
	));
}

#[test]
fn builds_macos_open_arguments_for_decodex_app() {
	assert_eq!(
		cli::decodex_app_open_args(None, false),
		vec![OsString::from("-a"), OsString::from("Decodex")]
	);
	assert_eq!(
		cli::decodex_app_open_args(Some(Path::new("target/decodex-app/Decodex.app")), true,),
		vec![
			OsString::from("-n"),
			Path::new("target/decodex-app/Decodex.app").as_os_str().to_owned(),
		]
	);
}

#[test]
fn parses_commit_with_authority_related_and_breaking() {
	let cli = Cli::parse_from([
		"decodex",
		"commit",
		"redesign decodex cli",
		"--authority",
		"XY-225",
		"--related",
		"XY-201",
		"--related",
		"XY-202",
		"--breaking",
	]);

	assert!(matches!(
		cli.command,
		Command::Commit(CommitCommand {
			authority: Some(_),
			manual_authority: false,
			breaking: true,
			..
		})
	));
}

#[test]
fn parses_git_hook_commands() {
	for (args, expected) in [
		(&["decodex", "git-hook", "commit-msg", ".git/COMMIT_EDITMSG"][..], "commit-msg"),
		(
			&["decodex", "git-hook", "pre-push", "origin", "git@github.com-y:hack-ink/repo.git"][..],
			"pre-push",
		),
	] {
		let cli = Cli::parse_from(args.iter().copied());
		let Command::GitHook(GitHookCommand { command }) = cli.command else {
			panic!("expected git-hook command");
		};

		match (expected, command) {
			("commit-msg", GitHookSubcommand::CommitMsg(_)) => {},
			("pre-push", GitHookSubcommand::PrePush(_)) => {},
			_ => panic!("unexpected parsed git-hook command for `{expected}`"),
		}
	}
}

#[test]
fn parses_okf_and_docs_find_graph_commands() {
	let okf_cli = Cli::parse_from([
		"decodex",
		"okf",
		"find",
		"docs",
		"--tag",
		"okf",
		"--text",
		"command design",
	]);
	let Command::Okf(OkfCommand {
		command:
			OkfSubcommand::Find(OkfFindCommand {
				root,
				filters: OkfFindFilters { tag, text: Some(text), .. },
			}),
	}) = okf_cli.command
	else {
		panic!("expected okf find command");
	};

	assert_eq!(root, Path::new("docs"));
	assert_eq!(tag, vec![String::from("okf")]);
	assert_eq!(text, "command design");

	let docs_cli = Cli::parse_from(["decodex", "docs", "graph", "--json"]);

	assert!(matches!(
		docs_cli.command,
		Command::Docs(DocsCommand {
			root,
			command: DocsSubcommand::Graph(DocsGraphCommand {
				json: true,
			}),
		}) if root == Path::new("docs")
	));
}

#[test]
fn docs_command_rejects_legacy_lint_alias() {
	let error = Cli::try_parse_from(["decodex", "docs", "lint"])
		.expect_err("command aliases are not supported");

	assert!(error.to_string().contains("unrecognized subcommand 'lint'"));
}

#[test]
fn parses_okf_init_command() {
	let cli = Cli::parse_from(["decodex", "okf", "init", "knowledge", "--profile", "wiki"]);

	assert!(matches!(
		cli.command,
		Command::Okf(OkfCommand {
			command: OkfSubcommand::Init(OkfInitCommand {
				root,
				profile: OkfInitProfileArg::Wiki,
			}),
		}) if root == Path::new("knowledge")
	));
}

#[test]
fn parses_land_with_pr_override() {
	let cli = Cli::parse_from([
		"decodex",
		"land",
		"redesign decodex cli",
		"--pr",
		"https://github.com/hack-ink/decodex/pull/64",
	]);

	assert!(matches!(cli.command, Command::Land(LandCommand { pr: Some(_), .. })));
}

#[test]
fn parses_manual_authority_commands() {
	enum ExpectedCommand {
		Commit,
		Land,
	}

	for (case_name, args, expected) in [
		(
			"commit manual authority",
			&["decodex", "commit", "ship hotfix", "--manual-authority"][..],
			ExpectedCommand::Commit,
		),
		(
			"land manual authority",
			&[
				"decodex",
				"land",
				"ship hotfix",
				"--manual-authority",
				"--pr",
				"https://github.com/hack-ink/decodex/pull/64",
			][..],
			ExpectedCommand::Land,
		),
	] {
		let cli = Cli::parse_from(args.iter().copied());

		match expected {
			ExpectedCommand::Commit => assert!(
				matches!(
					cli.command,
					Command::Commit(CommitCommand { authority: None, manual_authority: true, .. })
				),
				"unexpected parsed command for `{case_name}`"
			),
			ExpectedCommand::Land => assert!(
				matches!(
					cli.command,
					Command::Land(LandCommand {
						authority: None,
						manual_authority: true,
						pr: Some(_),
						..
					})
				),
				"unexpected parsed command for `{case_name}`"
			),
		}
	}
}

#[test]
fn land_manual_authority_requires_pr() {
	let error = Cli::try_parse_from(["decodex", "land", "ship hotfix", "--manual-authority"])
		.expect_err("manual authority land should require an explicit PR");

	assert!(error.to_string().contains("--manual-authority"));
	assert!(error.to_string().contains("--pr"));
}

#[test]
fn commit_rejects_authority_and_manual_authority_together() {
	let error = Cli::try_parse_from([
		"decodex",
		"commit",
		"ship hotfix",
		"--authority",
		"XY-225",
		"--manual-authority",
	])
	.expect_err("authority and manual-authority should conflict");

	assert!(error.to_string().contains("--authority"));
	assert!(error.to_string().contains("--manual-authority"));
}
