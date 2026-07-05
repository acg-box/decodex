use clap::Parser;

use crate::cli::{
	Cli, Command,
	git_hook_commands::{GitHookCommand, GitHookSubcommand},
};

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
