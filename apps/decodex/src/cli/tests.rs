use std::{ffi::OsString, path::Path};

use clap::{Parser, error::ErrorKind};

use crate::{
	cli::{
		AppCommand, AttemptCommand, Cli, Command, ProbeCommand, ProjectConfigArgs,
		account_commands::{AccountCommand, AccountSubcommand, AccountUseCommand},
		control_commands::{
			DiagnoseCommand, EvidenceCommand, LaneCommand, LaneInspectCommand,
			LaneInterruptCommand, LaneSteerCommand, LaneSubcommand, McpSubcommand, ProjectCommand,
			ProjectSubcommand, RunCommand, ServeCommand, StatusCommand,
		},
		docs_okf_commands::{
			DocsCommand, DocsGraphCommand, DocsSubcommand, OkfCommand, OkfFindCommand,
			OkfFindFilters, OkfInitCommand, OkfInitProfileArg, OkfSubcommand,
		},
		git_hook_commands::{GitHookCommand, GitHookSubcommand},
		manual_commands::{CommitCommand, LandCommand},
		recovery_commands::{
			GhostLaneCleanupCommand, GhostLaneDiagnoseCommand, GhostLaneRecoveryCommand,
			GhostLaneRecoverySubcommand, LegacyCloseoutRecoveryCommand,
			MergedCloseoutRecoveryCommand, RecoverCommand, RecoverSubcommand,
			ReviewHandoffAdoptCommand, ReviewHandoffDiagnoseCommand, ReviewHandoffRebindCommand,
			ReviewHandoffRecoveryCommand, ReviewHandoffRecoverySubcommand,
			StaleActiveDiagnoseCommand, StaleActiveRecoveryCommand, StaleActiveRecoverySubcommand,
			StaleActiveReleaseCommand,
		},
		research_intake_commands::{
			IntakeCommand, IntakeGoalCommand, IntakeIssuesCommand, IntakeSubcommand,
			ResearchCommand, ResearchCompileCommand, ResearchOutcomeArg, ResearchPromoteCommand,
			ResearchSubcommand,
		},
	},
	mcp::{McpCapabilityProfile, McpTransport},
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
		super::decodex_app_open_args(None, false),
		vec![OsString::from("-a"), OsString::from("Decodex")]
	);
	assert_eq!(
		super::decodex_app_open_args(Some(Path::new("target/decodex-app/Decodex.app")), true),
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

#[test]
fn parses_run_modes() {
	for (case_name, args, expected_issue, expected_dry_run, expected_explain) in [
		(
			"positional issue dry run",
			&["decodex", "run", "issue-1", "--dry-run"][..],
			Some("issue-1"),
			true,
			false,
		),
		("default run", &["decodex", "run"][..], None, false, false),
		("explain dry run", &["decodex", "run", "--dry-run", "--explain"][..], None, true, true),
	] {
		let cli = Cli::parse_from(args.iter().copied());

		assert!(
			matches!(
				cli.command,
				Command::Run(RunCommand { issue, dry_run, explain, .. })
					if issue.as_deref() == expected_issue
						&& dry_run == expected_dry_run
						&& explain == expected_explain
			),
			"unexpected parsed run command for `{case_name}`"
		);
	}

	let error = Cli::try_parse_from(["decodex", "run", "--explain"])
		.expect_err("explain should require dry-run");

	assert!(error.to_string().contains("--dry-run"));

	let error = Cli::try_parse_from(["decodex", "run", "issue-1", "--dry-run", "--explain"])
		.expect_err("explain should reject positional issue");

	assert!(error.to_string().contains("--explain"));
	assert!(error.to_string().contains("[ISSUE]"));
}

#[test]
fn parses_serve_modes() {
	for (case_name, args, expected_listen_address, expected_config, expected_dev) in [
		("default listen address", &["decodex", "serve"][..], "127.0.0.1:8192", None, false),
		(
			"custom listen address and project config",
			&[
				"decodex",
				"serve",
				"--config",
				"./project.toml",
				"--listen-address",
				"127.0.0.1:9000",
			][..],
			"127.0.0.1:9000",
			Some("./project.toml"),
			false,
		),
		("dev mode", &["decodex", "serve", "--dev"][..], "127.0.0.1:8192", None, true),
	] {
		let cli = Cli::parse_from(args.iter().copied());

		assert!(
			matches!(
				cli.command,
				Command::Serve(ServeCommand {
					project_config: ProjectConfigArgs { config },
					listen_address,
					dev,
				}) if listen_address == expected_listen_address
					&& config.as_deref() == expected_config.map(Path::new)
					&& dev == expected_dev
			),
			"unexpected parsed serve command for `{case_name}`"
		);
	}
}

#[test]
fn parses_mcp_stdio_serve() {
	let cli = Cli::parse_from([
		"decodex",
		"mcp",
		"serve",
		"--config",
		"./project.toml",
		"--transport",
		"stdio",
	]);
	let Command::Mcp(command) = cli.command else {
		panic!("expected mcp command");
	};
	let McpSubcommand::Serve(serve) = command.command;

	assert_eq!(serve.project_config.config.as_deref(), Some(Path::new("./project.toml")));
	assert_eq!(serve.transport, McpTransport::Stdio);
	assert_eq!(serve.effective_capability_profile(), McpCapabilityProfile::Admin);
	assert_eq!(serve.listen_address, crate::mcp::DEFAULT_MCP_HTTP_LISTEN_ADDRESS);
	assert!(serve.allowed_origins.is_empty());
	assert_eq!(serve.bearer_token_env, None);
}

#[test]
fn parses_mcp_streamable_http_serve_with_safe_profile_default() {
	let cli = Cli::parse_from([
		"decodex",
		"mcp",
		"serve",
		"--transport",
		"streamable-http",
		"--listen-address",
		"127.0.0.1:8194",
		"--allow-origin",
		"http://127.0.0.1:8194",
		"--bearer-token-env",
		"DECODEX_MCP_TOKEN",
	]);
	let Command::Mcp(command) = cli.command else {
		panic!("expected mcp command");
	};
	let McpSubcommand::Serve(serve) = command.command;

	assert_eq!(serve.transport, McpTransport::StreamableHttp);
	assert_eq!(serve.effective_capability_profile(), McpCapabilityProfile::Observe);
	assert_eq!(serve.listen_address, "127.0.0.1:8194");
	assert_eq!(serve.allowed_origins, vec!["http://127.0.0.1:8194"]);
	assert_eq!(serve.bearer_token_env.as_deref(), Some("DECODEX_MCP_TOKEN"));
}

#[test]
fn rejects_serve_interval_argument() {
	let error = Cli::try_parse_from(["decodex", "serve", "--interval", "30s"])
		.expect_err("serve interval override should be removed");
	let message = error.to_string();

	assert!(message.contains("--interval"));
}

#[test]
fn parses_project_subcommands() {
	enum ExpectedProjectSubcommand {
		Add,
		Enable,
		Remove,
	}

	for (case_name, args, expected) in [
		(
			"add",
			&["decodex", "project", "add", "./project.toml"][..],
			ExpectedProjectSubcommand::Add,
		),
		(
			"enable",
			&["decodex", "project", "enable", "pubfi"][..],
			ExpectedProjectSubcommand::Enable,
		),
		(
			"remove",
			&["decodex", "project", "remove", "vibe-mono"][..],
			ExpectedProjectSubcommand::Remove,
		),
	] {
		let cli = Cli::parse_from(args.iter().copied());

		match expected {
			ExpectedProjectSubcommand::Add => assert!(
				matches!(
					cli.command,
					Command::Project(ProjectCommand { command: ProjectSubcommand::Add(_) })
				),
				"unexpected parsed project subcommand for `{case_name}`"
			),
			ExpectedProjectSubcommand::Enable => assert!(
				matches!(
					cli.command,
					Command::Project(ProjectCommand { command: ProjectSubcommand::Enable(_) })
				),
				"unexpected parsed project subcommand for `{case_name}`"
			),
			ExpectedProjectSubcommand::Remove => assert!(
				matches!(
					cli.command,
					Command::Project(ProjectCommand { command: ProjectSubcommand::Remove(_) })
				),
				"unexpected parsed project subcommand for `{case_name}`"
			),
		}
	}
}

#[test]
fn parses_account_use_with_auth_json_override() {
	let cli = Cli::parse_from([
		"decodex",
		"account",
		"use",
		"copy@example.com",
		"--auth-json",
		"./auth.json",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Account(AccountCommand {
			command: AccountSubcommand::Use(AccountUseCommand {
				selector,
				auth_json: Some(_),
				json: true,
			})
		}) if selector == "copy@example.com"
	));
}

#[test]
fn account_commands_reject_project_config() {
	let error = Cli::try_parse_from(["decodex", "account", "list", "--config", "./project.toml"])
		.expect_err("global account commands should not accept project config");

	assert!(error.to_string().contains("--config"));
}

#[test]
fn project_config_must_belong_to_project_scoped_command() {
	let error = Cli::try_parse_from(["decodex", "--config", "./project.toml", "status"])
		.expect_err("project config should not be accepted at root");

	assert!(error.to_string().contains("--config"));
}

#[test]
fn parses_hidden_attempt_with_stdin_request() {
	let cli = Cli::parse_from(["decodex", "_attempt", "--config", "./project.toml", "-"]);

	assert!(matches!(
		cli.command,
		Command::Attempt(AttemptCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			request,
		}) if request == "-" && config == Path::new("./project.toml")
	));
}

#[test]
fn parses_probe_with_custom_transport() {
	let cli = Cli::parse_from(["decodex", "probe", "ws://127.0.0.1:9000"]);

	assert!(matches!(
		cli.command,
		Command::Probe(ProbeCommand { transport, .. }) if transport == "ws://127.0.0.1:9000"
	));
}

#[test]
fn parses_status_with_json_limit_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"status",
		"--config",
		"./project.toml",
		"--json",
		"--limit",
		"5",
	]);

	assert!(matches!(
		cli.command,
		Command::Status(StatusCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			json: true,
			limit: 5,
			live: false,
		}) if config == Path::new("./project.toml")
	));
}

#[test]
fn parses_lane_inspect_with_run_id_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"lane",
		"--config",
		"./project.toml",
		"inspect",
		"XY-703",
		"--run-id",
		"xy-703-attempt-1",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Lane(LaneCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: LaneSubcommand::Inspect(LaneInspectCommand {
				issue,
				run_id: Some(run_id),
				json: true,
			})
		}) if config == Path::new("./project.toml")
			&& issue == "XY-703"
			&& run_id == "xy-703-attempt-1"
	));
}

#[test]
fn parses_lane_interrupt_with_force_reason_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"lane",
		"--config",
		"./project.toml",
		"interrupt",
		"XY-703",
		"--run-id",
		"xy-703-attempt-1",
		"--force",
		"--reason",
		"operator requested",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Lane(LaneCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: LaneSubcommand::Interrupt(LaneInterruptCommand {
				issue,
				run_id,
				force: true,
				reason: Some(reason),
				json: true,
			})
		}) if config == Path::new("./project.toml")
			&& issue == "XY-703"
			&& run_id == "xy-703-attempt-1"
			&& reason == "operator requested"
	));
}

#[test]
fn parses_lane_steer_with_expected_turn_precondition() {
	let cli = Cli::parse_from([
		"decodex",
		"lane",
		"--config",
		"./project.toml",
		"steer",
		"XY-704",
		"--run-id",
		"run-1",
		"--expected-turn-id",
		"turn-1",
		"--message",
		"adjust the current implementation",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Lane(LaneCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: LaneSubcommand::Steer(LaneSteerCommand {
				issue,
				run_id,
				expected_turn_id,
				message,
				json: true,
				..
			})
		}) if config == Path::new("./project.toml")
			&& issue == "XY-704"
			&& run_id == "run-1"
			&& expected_turn_id == "turn-1"
			&& message == "adjust the current implementation"
	));
}

#[test]
fn parses_research_compile_with_intent_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"research",
		"--config",
		"./project.toml",
		"compile",
		"--intent",
		"research X",
		"--source-issue",
		"XY-860",
		"--outcome",
		"needs-human-decision",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Research(ResearchCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: ResearchSubcommand::Compile(ResearchCompileCommand {
				intent: Some(intent),
				source_issue: Some(source_issue),
				outcome: ResearchOutcomeArg::NeedsHumanDecision,
				json: true,
				..
			})
		}) if config == Path::new("./project.toml")
			&& intent == "research X"
			&& source_issue == "XY-860"
	));
}

#[test]
fn parses_intake_issues_dry_run_with_project() {
	let cli = Cli::parse_from([
		"decodex",
		"intake",
		"issues",
		"--project",
		"decodex",
		"XY-1",
		"XY-2",
		"--dry-run",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Intake(IntakeCommand {
			command: IntakeSubcommand::Issues(IntakeIssuesCommand {
				project: Some(_),
				dry_run: true,
				apply: false,
				json: true,
				issues,
				..
			})
		}) if issues == vec![String::from("XY-1"), String::from("XY-2")]
	));
}

#[test]
fn parses_intake_issues_apply_with_project() {
	let cli = Cli::parse_from([
		"decodex",
		"intake",
		"issues",
		"--project",
		"decodex",
		"XY-1",
		"XY-2",
		"--apply",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Intake(IntakeCommand {
			command: IntakeSubcommand::Issues(IntakeIssuesCommand {
				project: Some(_),
				dry_run: false,
				apply: true,
				json: true,
				issues,
				..
			})
		}) if issues == vec![String::from("XY-1"), String::from("XY-2")]
	));
}

#[test]
fn parses_intake_goal_apply_with_project_and_team_anchor() {
	let cli = Cli::parse_from([
		"decodex",
		"intake",
		"goal",
		"--project",
		"decodex",
		"goal-intake-contract",
		"--apply",
		"--team-issue",
		"XY-852",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Intake(IntakeCommand {
			command: IntakeSubcommand::Goal(IntakeGoalCommand {
				project: Some(_),
				contract_id,
				dry_run: false,
				apply: true,
				team_issue: Some(team_issue),
				json: true,
				..
			})
		}) if contract_id == "goal-intake-contract" && team_issue == "XY-852"
	));
}

#[test]
fn rejects_intake_issues_without_explicit_mode() {
	let error = Cli::try_parse_from(["decodex", "intake", "issues", "XY-1"])
		.expect_err("intake issues requires dry-run or apply");

	assert!(error.to_string().contains("--dry-run") || error.to_string().contains("--apply"));
}

#[test]
fn parses_research_promote_with_acceptance_metadata() {
	let cli = Cli::parse_from([
		"decodex",
		"research",
		"--config",
		"./project.toml",
		"promote",
		"research-design-contract",
		"--accepted-by",
		"operator",
		"--accepted-at",
		"2026-06-10T00:00:00Z",
		"--acceptance-source",
		"conversation",
		"--reason",
		"push this forward",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Research(ResearchCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: ResearchSubcommand::Promote(ResearchPromoteCommand {
				contract_id,
				accepted_by,
				accepted_at: Some(accepted_at),
				acceptance_source,
				reason: Some(reason),
				json: true,
			})
		}) if config == Path::new("./project.toml")
			&& contract_id == "research-design-contract"
			&& accepted_by == "operator"
			&& accepted_at == "2026-06-10T00:00:00Z"
			&& acceptance_source == "conversation"
			&& reason == "push this forward"
	));
}

#[test]
fn parses_diagnose_with_json_limit_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"diagnose",
		"--config",
		"./project.toml",
		"--json",
		"--limit",
		"5",
	]);

	assert!(matches!(
		cli.command,
		Command::Diagnose(DiagnoseCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			json: true,
			limit: 5,
		}) if config == Path::new("./project.toml")
	));
}

#[test]
fn parses_evidence_with_issue_run_attempt_json_payload_and_project_config() {
	let cli = Cli::parse_from([
		"decodex",
		"evidence",
		"--config",
		"./project.toml",
		"PUB-101",
		"--run-id",
		"run-1",
		"--attempt",
		"2",
		"--json",
		"--include-payload",
	]);

	assert!(matches!(
		cli.command,
		Command::Evidence(EvidenceCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			project: None,
			issue,
			run_id: Some(_),
			attempt: Some(2),
			json: true,
			include_payload: true,
		}) if config == Path::new("./project.toml") && issue == "PUB-101"
	));
}

#[test]
fn parses_evidence_with_registered_project_id() {
	let cli =
		Cli::parse_from(["decodex", "evidence", "--project", "pubfi-mono", "PUB-101", "--json"]);

	assert!(matches!(
		cli.command,
		Command::Evidence(EvidenceCommand {
			project_config: ProjectConfigArgs { config: None },
			project: Some(project),
			issue,
			run_id: None,
			attempt: None,
			json: true,
			include_payload: false,
		}) if project == "pubfi-mono" && issue == "PUB-101"
	));
}

#[test]
fn parses_review_handoff_diagnose_with_issue_and_json() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"--config",
		"./project.toml",
		"review-handoff",
		"diagnose",
		"PUB-718",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
				command: ReviewHandoffRecoverySubcommand::Diagnose(
					ReviewHandoffDiagnoseCommand { issue: Some(_), json: true }
				)
			})
		}) if config == Path::new("./project.toml")
	));
}

#[test]
fn parses_ghost_lane_diagnose_with_issue_and_json() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"--config",
		"./project.toml",
		"ghost-lane",
		"diagnose",
		"PUBFI-012",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: RecoverSubcommand::GhostLane(GhostLaneRecoveryCommand {
				command: GhostLaneRecoverySubcommand::Diagnose(
					GhostLaneDiagnoseCommand { issue: Some(_), json: true }
				)
			})
		}) if config == Path::new("./project.toml")
	));
}

#[test]
fn parses_ghost_lane_cleanup_dry_run() {
	let cli =
		Cli::parse_from(["decodex", "recover", "ghost-lane", "cleanup", "PUBFI-012", "--dry-run"]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::GhostLane(GhostLaneRecoveryCommand {
				command: GhostLaneRecoverySubcommand::Cleanup(
					GhostLaneCleanupCommand { issue, dry_run: true }
				)
			}),
			..
		}) if issue == "PUBFI-012"
	));
}

#[test]
fn parses_stale_active_diagnose_with_issue_and_json() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"--config",
		"./project.toml",
		"stale-active",
		"diagnose",
		"PUB-1626",
		"--json",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			project_config: ProjectConfigArgs { config: Some(config) },
			command: RecoverSubcommand::StaleActive(StaleActiveRecoveryCommand {
				command: StaleActiveRecoverySubcommand::Diagnose(
					StaleActiveDiagnoseCommand { issue: Some(_), json: true }
				)
			})
		}) if config == Path::new("./project.toml")
	));
}

#[test]
fn parses_stale_active_release_dry_run() {
	let cli =
		Cli::parse_from(["decodex", "recover", "stale-active", "release", "PUB-1626", "--dry-run"]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::StaleActive(StaleActiveRecoveryCommand {
				command: StaleActiveRecoverySubcommand::Release(
					StaleActiveReleaseCommand { issue, dry_run: true }
				)
			}),
			..
		}) if issue == "PUB-1626"
	));
}

#[test]
fn parses_review_handoff_rebind_dry_run() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"review-handoff",
		"rebind",
		"PUB-718",
		"--pr",
		"https://github.com/hack-ink/pubfi-mono-v2/pull/14",
		"--dry-run",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
				command: ReviewHandoffRecoverySubcommand::Rebind(
					ReviewHandoffRebindCommand { issue, pr, dry_run: true }
				)
			}),
			..
		}) if issue == "PUB-718"
			&& pr == "https://github.com/hack-ink/pubfi-mono-v2/pull/14"
	));
}

#[test]
fn parses_review_handoff_adopt_dry_run() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"review-handoff",
		"adopt",
		"XY-944",
		"--pr",
		"https://github.com/hack-ink/decodex/pull/344",
		"--dry-run",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
				command: ReviewHandoffRecoverySubcommand::Adopt(
					ReviewHandoffAdoptCommand { issue, pr, dry_run: true }
				)
			}),
			..
		}) if issue == "XY-944"
			&& pr == "https://github.com/hack-ink/decodex/pull/344"
	));
}

#[test]
fn parses_legacy_closeout_manual_authority() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"legacy-closeout",
		"PUB-718",
		"--pr",
		"https://github.com/hack-ink/pubfi-mono-v2/pull/14",
		"--manual-authority",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::LegacyCloseout(LegacyCloseoutRecoveryCommand {
				issue,
				pr,
				dry_run: false,
				manual_authority: true,
			}),
			..
		}) if issue == "PUB-718"
			&& pr == "https://github.com/hack-ink/pubfi-mono-v2/pull/14"
	));
}

#[test]
fn parses_merged_closeout_manual_authority() {
	let cli = Cli::parse_from([
		"decodex",
		"recover",
		"merged-closeout",
		"PUB-1549",
		"--pr",
		"https://github.com/helixbox/pubfi-mono/pull/309",
		"--manual-authority",
	]);

	assert!(matches!(
		cli.command,
		Command::Recover(RecoverCommand {
			command: RecoverSubcommand::MergedCloseout(MergedCloseoutRecoveryCommand {
				issue,
				pr,
				dry_run: false,
				manual_authority: true,
			}),
			..
		}) if issue == "PUB-1549"
			&& pr == "https://github.com/helixbox/pubfi-mono/pull/309"
	));
}
