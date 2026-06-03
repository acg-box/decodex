use std::{
	fs,
	io::{self, Read as _},
	path::{Path, PathBuf},
	time::Duration,
};

use clap::{
	Args, Parser, Subcommand,
	builder::{
		Styles,
		styling::{AnsiColor, Effects},
	},
};
use serde::{Deserialize, Serialize};

use crate::{
	accounts::{self, AccountImportRequest, AccountLoginRequest, AccountUseRequest},
	agent,
	archive_hygiene::{self, ArchiveHygieneRequest},
	maintenance::{self, MaintenanceMode, MaintenancePruneRequest, MaintenanceScope},
	manual::{self, ManualCommitRequest, ManualLandRequest},
	orchestrator::{
		self, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, DiagnoseRequest, EvidenceRequest,
		IssueDispatchMode, LaneInspectRequest, LaneInterruptRequest, LaneSteerReport,
		LaneSteerRequest, RunOnceRequest, ServeRequest,
	},
	prelude::{Result, eyre},
	radar::{
		self, RadarBackfillReleaseRangeRequest, RadarBundleBuildRequest,
		RadarBundleValidateRequest, RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
		RadarRefreshQueueRequest, RadarRefreshReleaseDeltaRequest, RadarRenderSignalRequest,
		RadarValidateRequest,
	},
	recovery::{self, ReviewHandoffDiagnoseRequest, ReviewHandoffRebindRequest},
	runtime,
};

/// Root CLI parser for the Decodex control plane.
#[derive(Debug, Parser)]
#[command(
	about = "Repo-native orchestration for autonomous coding agents.",
	version = concat!(
		env!("CARGO_PKG_VERSION"),
		"-",
		env!("VERGEN_GIT_SHA"),
		"-",
		env!("VERGEN_CARGO_TARGET_TRIPLE"),
	),
	arg_required_else_help = true,
	rename_all = "kebab",
	subcommand_required = true,
	styles = styles(),
)]
pub(crate) struct Cli {
	#[command(subcommand)]
	command: Command,
}
impl Cli {
	pub(crate) fn run(&self) -> Result<()> {
		match &self.command {
			Command::Commit(args) => args.run(),
			Command::Land(args) => args.run(),
			Command::Run(args) => args.run(),
			Command::Serve(args) => args.run(),
			Command::Project(args) => args.run(),
			Command::Lane(args) => args.run(),
			Command::Status(args) => args.run(),
			Command::Diagnose(args) => args.run(),
			Command::Evidence(args) => args.run(),
			Command::Recover(args) => args.run(),
			Command::ArchiveLinear(args) => args.run(),
			Command::Maintenance(args) => args.run(),
			Command::Account(args) => args.run(),
			Command::Radar(args) => args.run(),
			Command::Probe(args) => args.run(),
			Command::Attempt(args) => args.run(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct AttemptRequest {
	#[serde(default)]
	pub(crate) dry_run: bool,
	pub(crate) issue_id: String,
	pub(crate) issue_state: String,
	pub(crate) initial_issue_state: Option<String>,
	#[serde(default)]
	pub(crate) lease_preacquired: bool,
	#[serde(default)]
	pub(crate) allow_unverified_codex: bool,
	pub(crate) issue_claim_fd: Option<i32>,
	pub(crate) dispatch_slot_fd: Option<i32>,
	pub(crate) dispatch_slot_index: Option<usize>,
	pub(crate) dispatch_mode: AttemptDispatchMode,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) retry_budget_base: i64,
	pub(crate) workflow_snapshot: String,
}

#[derive(Debug, Args)]
struct ProjectConfigArgs {
	/// Use this Decodex project directory or `project.toml` instead of resolving from the
	/// registered current checkout.
	#[arg(short = 'c', long, value_name = "PROJECT_DIR")]
	config: Option<PathBuf>,
}
impl ProjectConfigArgs {
	fn as_path(&self) -> Option<&Path> {
		self.config.as_deref()
	}
}

#[derive(Debug, Args)]
struct AccountCommand {
	#[command(subcommand)]
	command: AccountSubcommand,
}
impl AccountCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			AccountSubcommand::List(args) => accounts::run_account_list(args.json),
			AccountSubcommand::Select(args) =>
				accounts::run_account_select(&args.selector, args.json),
			AccountSubcommand::Clear(args) => accounts::run_account_clear(args.json),
			AccountSubcommand::Logout(args) =>
				accounts::run_account_logout(&args.selector, args.json),
			AccountSubcommand::ImportAuth(args) =>
				accounts::run_account_import(&AccountImportRequest {
					auth_json_path: args.auth_json.clone(),
					json: args.json,
				}),
			AccountSubcommand::Use(args) => accounts::run_account_use(&AccountUseRequest {
				selector: args.selector.clone(),
				auth_json_path: args.auth_json.clone(),
				json: args.json,
			}),
			AccountSubcommand::Login(args) => accounts::run_account_login(&AccountLoginRequest {
				codex_bin: args.codex_bin.clone(),
				keep_temp_home: args.keep_temp_home,
			}),
		}
	}
}

#[derive(Debug, Args)]
struct AccountListCommand {
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountSelectCommand {
	/// Email, full account id, or redacted fingerprint to pin.
	selector: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountClearCommand {
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountLogoutCommand {
	/// Email, full account id, or redacted fingerprint to remove.
	selector: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountImportCommand {
	/// Path to a Codex `auth.json` file to import.
	#[arg(value_name = "AUTH_JSON")]
	auth_json: PathBuf,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountUseCommand {
	/// Email, full account id, or redacted fingerprint to write into Codex `auth.json`.
	selector: String,
	/// Override the Codex `auth.json` destination. Defaults to `$CODEX_HOME/auth.json`
	/// or `~/.codex/auth.json`.
	#[arg(long, value_name = "AUTH_JSON")]
	auth_json: Option<PathBuf>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountLoginCommand {
	/// Codex CLI binary used for isolated device login.
	#[arg(long, default_value = "codex")]
	codex_bin: String,
	/// Keep the temporary Codex home after login for manual inspection.
	#[arg(long, hide = true)]
	keep_temp_home: bool,
}

#[derive(Debug, Args)]
struct CommitCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Tree-change summary for the new commit message.
	#[arg(value_name = "SUMMARY")]
	summary: String,
	/// Primary issue that authorizes the change. Defaults to the current issue worktree name.
	#[arg(long, value_name = "ISSUE", conflicts_with = "manual_authority")]
	authority: Option<String>,
	/// Use reserved authority `manual` instead of a Linear issue.
	#[arg(long, conflicts_with = "authority")]
	manual_authority: bool,
	/// Additional related issues for the commit message.
	#[arg(long, value_name = "ISSUE")]
	related: Vec<String>,
	/// Mark the change as breaking.
	#[arg(long)]
	breaking: bool,
}
impl CommitCommand {
	fn run(&self) -> Result<()> {
		manual::run_commit(
			self.project_config.as_path(),
			&ManualCommitRequest {
				summary: self.summary.clone(),
				authority: self.authority.clone(),
				manual_authority: self.manual_authority,
				related: self.related.clone(),
				breaking: self.breaking,
			},
		)
	}
}

#[derive(Debug, Args)]
struct LandCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Tree-change summary for the landed change record.
	#[arg(value_name = "SUMMARY")]
	summary: String,
	/// Primary issue that authorizes the merged change. Defaults to the current issue worktree
	/// name.
	#[arg(long, value_name = "ISSUE", conflicts_with = "manual_authority")]
	authority: Option<String>,
	/// Use reserved authority `manual` instead of a Linear issue; requires `--pr`.
	#[arg(long, conflicts_with = "authority", requires = "pr")]
	manual_authority: bool,
	/// Pull request URL to land. Required with `--manual-authority`; otherwise defaults to the
	/// current review handoff marker.
	#[arg(long, value_name = "URL")]
	pr: Option<String>,
	/// Additional related issues for the landed change record.
	#[arg(long, value_name = "ISSUE")]
	related: Vec<String>,
	/// Mark the landed change record as breaking.
	#[arg(long)]
	breaking: bool,
}
impl LandCommand {
	fn run(&self) -> Result<()> {
		manual::run_land(
			self.project_config.as_path(),
			&ManualLandRequest {
				summary: self.summary.clone(),
				authority: self.authority.clone(),
				manual_authority: self.manual_authority,
				pr_url: self.pr.clone(),
				related: self.related.clone(),
				breaking: self.breaking,
			},
		)
	}
}

#[derive(Debug, Args)]
struct RunCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Run a specific leased or queued issue by Linear identifier or tracker issue id.
	#[arg(value_name = "ISSUE")]
	issue: Option<String>,
	/// Validate project loading, queue eligibility, and lane planning without tracker mutation.
	#[arg(long)]
	dry_run: bool,
	/// Explain current queued candidates without preparing or dispatching a lane.
	#[arg(long, requires = "dry_run", conflicts_with = "issue")]
	explain: bool,
	/// Continue after warning when Codex app-server is outside the locally verified range.
	#[arg(long)]
	allow_unverified_codex: bool,
}
impl RunCommand {
	fn run(&self) -> Result<()> {
		orchestrator::run_once(RunOnceRequest {
			config_path: self.project_config.as_path(),
			dry_run: self.dry_run,
			explain_queue: self.explain,
			preferred_issue_id: self.issue.as_deref(),
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_lease_acquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			preferred_dispatch_mode: None,
			preferred_run_id: None,
			preferred_attempt_number: None,
			preferred_retry_budget_base: None,
			preferred_workflow_snapshot: None,
			allow_unverified_codex: self.allow_unverified_codex,
		})
	}
}

#[derive(Debug, Args)]
struct ServeCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Operator UI listen address.
	#[arg(long, value_name = "ADDR", default_value = "127.0.0.1:8912")]
	listen_address: String,
	/// Start the local dev endpoint without polling or dispatching projects.
	#[arg(long, hide = true)]
	dev: bool,
	/// Continue after warning when Codex app-server is outside the locally verified range.
	#[arg(long)]
	allow_unverified_codex: bool,
}
impl ServeCommand {
	fn run(&self) -> Result<()> {
		orchestrator::run_control_plane(ServeRequest {
			config_path: self.project_config.as_path(),
			listen_address: &self.listen_address,
			dev: self.dev,
			allow_unverified_codex: self.allow_unverified_codex,
		})
	}
}

#[derive(Debug, Args)]
struct ProjectCommand {
	#[command(subcommand)]
	command: ProjectSubcommand,
}
impl ProjectCommand {
	fn run(&self) -> Result<()> {
		let state_store = runtime::open_runtime_store()?;

		match &self.command {
			ProjectSubcommand::Add(args) => {
				let registration =
					runtime::register_project_config(&state_store, &args.config, true)?;

				if !registration.enabled() {
					state_store.set_project_enabled(registration.service_id(), true)?;
				}

				println!(
					"registered project {} at {}",
					registration.service_id(),
					registration.config_path().display()
				);
			},
			ProjectSubcommand::List => {
				let projects = state_store.list_projects()?;

				if projects.is_empty() {
					println!("No registered projects.");
				} else {
					for project in projects {
						let status = if project.enabled() { "enabled" } else { "disabled" };

						println!(
							"{}\t{}\t{}",
							project.service_id(),
							status,
							project.config_path().display()
						);
					}
				}
			},
			ProjectSubcommand::Enable(args) => {
				state_store.set_project_enabled(&args.service_id, true)?;

				println!("enabled project {}", args.service_id);
			},
			ProjectSubcommand::Disable(args) => {
				state_store.set_project_enabled(&args.service_id, false)?;

				println!("disabled project {}", args.service_id);
			},
			ProjectSubcommand::Remove(args) => {
				let removed = state_store.remove_project(&args.service_id)?;

				println!(
					"removed project {} at {}",
					removed.service_id(),
					removed.config_path().display()
				);
			},
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
struct ProjectAddCommand {
	/// Path to a Decodex project directory containing `project.toml` and `WORKFLOW.md`.
	#[arg(value_name = "PROJECT_DIR")]
	config: PathBuf,
}

#[derive(Debug, Args)]
struct ProjectToggleCommand {
	/// Project service id from the registered Decodex config.
	#[arg(value_name = "SERVICE_ID")]
	service_id: String,
}

#[derive(Debug, Args)]
struct LaneCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	#[command(subcommand)]
	command: LaneSubcommand,
}
impl LaneCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			LaneSubcommand::Inspect(args) => orchestrator::print_lane_inspect(LaneInspectRequest {
				config_path: self.project_config.as_path(),
				issue: &args.issue,
				run_id: args.run_id.as_deref(),
				json: args.json,
			}),
			LaneSubcommand::Interrupt(args) => orchestrator::interrupt_lane(LaneInterruptRequest {
				config_path: self.project_config.as_path(),
				issue: &args.issue,
				run_id: &args.run_id,
				force: args.force,
				reason: args.reason.as_deref(),
				json: args.json,
				source: "cli",
			})
			.map(|_report| ()),
			LaneSubcommand::Steer(args) => args.run(self.project_config.as_path()),
		}
	}
}

#[derive(Debug, Args)]
struct LaneInspectCommand {
	/// Issue identifier or local issue id to inspect.
	#[arg(value_name = "ISSUE")]
	issue: String,
	/// Restrict inspection to one run id.
	#[arg(long, value_name = "RUN_ID")]
	run_id: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct LaneInterruptCommand {
	/// Issue identifier or local issue id to interrupt.
	#[arg(value_name = "ISSUE")]
	issue: String,
	/// Run id for the active app-server turn to interrupt.
	#[arg(long, value_name = "RUN_ID")]
	run_id: String,
	/// Use hard process-kill fallback when soft interrupt is unavailable or fails.
	#[arg(long)]
	force: bool,
	/// Operator-visible reason retained in local private evidence.
	#[arg(long, value_name = "TEXT")]
	reason: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct LaneSteerCommand {
	/// Issue identifier or local issue id for the active lane.
	#[arg(value_name = "ISSUE")]
	issue: String,
	/// Run id that must own the active turn.
	#[arg(long, value_name = "RUN_ID")]
	run_id: String,
	/// Current active app-server turn id precondition.
	#[arg(long, value_name = "TURN_ID")]
	expected_turn_id: String,
	/// Operator-supplied steer text to send to the active turn.
	#[arg(long, value_name = "TEXT")]
	message: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
	/// How long to wait for the active attempt to report delivery.
	#[arg(long, value_name = "MILLISECONDS", default_value_t = default_lane_steer_wait_timeout_ms())]
	wait_timeout_ms: u64,
}
impl LaneSteerCommand {
	fn run(&self, config_path: Option<&Path>) -> Result<()> {
		let report = orchestrator::steer_lane(LaneSteerRequest {
			config_path,
			project_id: None,
			issue: &self.issue,
			run_id: &self.run_id,
			expected_turn_id: &self.expected_turn_id,
			message: &self.message,
			source: "cli",
			wait_timeout: Duration::from_millis(self.wait_timeout_ms),
		})?;

		if self.json {
			println!("{}", serde_json::to_string_pretty(&report)?);
		} else {
			print!("{}", render_lane_steer_report(&report));
		}
		if lane_steer_report_is_failure(&report) {
			eyre::bail!(
				"lane steer {}: {}",
				report.outcome,
				report.failure_class.as_deref().unwrap_or(&report.reason)
			);
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
struct StatusCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
	/// Maximum number of recent runs to display.
	#[arg(long, value_name = "COUNT", default_value_t = orchestrator::DEFAULT_STATUS_RUN_LIMIT)]
	limit: usize,
}
impl StatusCommand {
	fn run(&self) -> Result<()> {
		orchestrator::print_status(self.project_config.as_path(), self.json, self.limit)
	}
}

#[derive(Debug, Args)]
struct DiagnoseCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Emit the agent handoff index JSON instead of a one-line path summary.
	#[arg(long)]
	json: bool,
	/// Maximum number of recent runs to include while generating evidence.
	#[arg(long, value_name = "COUNT", default_value_t = orchestrator::DEFAULT_STATUS_RUN_LIMIT)]
	limit: usize,
}
impl DiagnoseCommand {
	fn run(&self) -> Result<()> {
		orchestrator::run_diagnose(DiagnoseRequest {
			config_path: self.project_config.as_path(),
			json: self.json,
			limit: self.limit,
		})
	}
}

#[derive(Debug, Args)]
struct EvidenceCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Issue identifier or local issue id to inspect.
	issue: String,
	/// Restrict readback to one run id. Defaults to the latest local run for the issue.
	#[arg(long, value_name = "RUN_ID")]
	run_id: Option<String>,
	/// Restrict readback to one attempt number. Defaults to the selected run attempt.
	#[arg(long, value_name = "NUMBER")]
	attempt: Option<i64>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
	/// Include full structured payload values instead of compact payload summaries only.
	#[arg(long)]
	include_payload: bool,
}
impl EvidenceCommand {
	fn run(&self) -> Result<()> {
		orchestrator::print_private_evidence(EvidenceRequest {
			config_path: self.project_config.as_path(),
			issue: &self.issue,
			run_id: self.run_id.as_deref(),
			attempt_number: self.attempt,
			json: self.json,
			include_payload: self.include_payload,
		})
	}
}

#[derive(Debug, Args)]
struct RecoverCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	#[command(subcommand)]
	command: RecoverSubcommand,
}
impl RecoverCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			RecoverSubcommand::ReviewHandoff(args) => args.run(self.project_config.as_path()),
		}
	}
}

#[derive(Debug, Args)]
struct ReviewHandoffRecoveryCommand {
	#[command(subcommand)]
	command: ReviewHandoffRecoverySubcommand,
}
impl ReviewHandoffRecoveryCommand {
	fn run(&self, config_path: Option<&Path>) -> Result<()> {
		match &self.command {
			ReviewHandoffRecoverySubcommand::Diagnose(args) =>
				recovery::run_review_handoff_diagnose(
					config_path,
					&ReviewHandoffDiagnoseRequest { issue: args.issue.clone(), json: args.json },
				),
			ReviewHandoffRecoverySubcommand::Rebind(args) => recovery::run_review_handoff_rebind(
				config_path,
				&ReviewHandoffRebindRequest {
					issue: args.issue.clone(),
					pr_url: args.pr.clone(),
					dry_run: args.dry_run,
				},
			),
		}
	}
}

#[derive(Debug, Args)]
struct ReviewHandoffDiagnoseCommand {
	/// Issue identifier to inspect. Omit to inspect all retained review worktrees.
	#[arg(value_name = "ISSUE")]
	issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct ReviewHandoffRebindCommand {
	/// Issue identifier for the retained review lane.
	#[arg(value_name = "ISSUE")]
	issue: String,
	/// Pull request URL to bind after validation.
	#[arg(long, value_name = "URL")]
	pr: String,
	/// Validate only; do not write runtime markers or tracker audit comments.
	#[arg(long)]
	dry_run: bool,
}

#[derive(Debug, Args)]
struct ArchiveLinearCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Repo label scope to inspect, for example `repo:decodex`.
	#[arg(long = "repo-label", value_name = "LABEL", required = true)]
	repo_labels: Vec<String>,
	/// Archive only issues last updated more than this many days ago.
	#[arg(long, value_name = "DAYS", default_value_t = 30)]
	older_than_days: u32,
	/// Perform the archive mutation. Omit this flag for the dry-run candidate report.
	#[arg(long)]
	execute: bool,
}
impl ArchiveLinearCommand {
	fn run(&self) -> Result<()> {
		archive_hygiene::run(
			self.project_config.as_path(),
			&ArchiveHygieneRequest {
				repo_labels: self.repo_labels.clone(),
				older_than_days: self.older_than_days,
				execute: self.execute,
			},
		)
	}
}

#[derive(Debug, Args)]
struct MaintenanceCommand {
	#[command(subcommand)]
	command: MaintenanceSubcommand,
}
impl MaintenanceCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			MaintenanceSubcommand::Prune(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct RadarCommand {
	#[command(subcommand)]
	command: RadarSubcommand,
}
impl RadarCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			RadarSubcommand::Bundle(args) => args.run(),
			RadarSubcommand::Ledger(args) => args.run(),
			RadarSubcommand::RefreshUpstreamQueue(args) => args.run(),
			RadarSubcommand::RefreshReleaseDelta(args) => args.run(),
			RadarSubcommand::Validate(args) => args.run(),
			RadarSubcommand::RenderSignal(args) => args.run(),
			RadarSubcommand::BackfillReleaseRange(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct RadarLedgerCommand {
	/// SQLite ledger path.
	#[arg(long, value_name = "DB", default_value_os_t = radar::default_ledger_path())]
	db: PathBuf,
	#[command(subcommand)]
	command: RadarLedgerSubcommand,
}
impl RadarLedgerCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			RadarLedgerSubcommand::Bootstrap => {
				let path = radar::ledger_bootstrap(&RadarLedgerBootstrapRequest {
					db_path: self.db.clone(),
				})?;

				println!("{}", path.display());

				Ok(())
			},
			RadarLedgerSubcommand::Ingest(args) => {
				let summary = radar::ledger_ingest(&RadarLedgerIngestRequest {
					db_path: self.db.clone(),
					bundle_path: args.bundle.clone(),
					analysis_path: args.analysis.clone(),
					signal_path: args.signal.clone(),
				})?;

				println!("{}", serde_json::to_string_pretty(&summary)?);

				Ok(())
			},
			RadarLedgerSubcommand::IngestExisting(args) => {
				let summary = radar::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
					db_path: self.db.clone(),
					bundles_dir: args.bundles_dir.clone(),
					analysis_dir: args.analysis_dir.clone(),
					signals_dir: args.signals_dir.clone(),
				})?;

				println!("{}", serde_json::to_string_pretty(&summary)?);

				Ok(())
			},
			RadarLedgerSubcommand::ArtifactLink(args) => {
				let summary = radar::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
					db_path: self.db.clone(),
					repo: args.repo.clone(),
					subject_kind: args.subject_kind.clone(),
					subject_id: args.subject_id.clone(),
					artifact_kind: args.artifact_kind.clone(),
					path: args.path.clone(),
				})?;

				println!("{}", serde_json::to_string_pretty(&summary)?);

				Ok(())
			},
			RadarLedgerSubcommand::Summary(args) => {
				let summary =
					radar::ledger_summary(&RadarLedgerSummaryRequest { db_path: self.db.clone() })?;

				if args.json {
					println!("{}", serde_json::to_string_pretty(&summary)?);
				} else {
					for (key, value) in summary {
						println!("{key}\t{value}");
					}
				}

				Ok(())
			},
		}
	}
}

#[derive(Debug, Args)]
struct RadarLedgerIngestCommand {
	/// Path to a `github_change_bundle/v1` JSON file.
	#[arg(long)]
	bundle: PathBuf,
	/// Optional analysis draft path.
	#[arg(long)]
	analysis: Option<PathBuf>,
	/// Optional rendered `signal_entry/v1` path.
	#[arg(long)]
	signal: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct RadarLedgerIngestExistingCommand {
	/// Directory containing `github_change_bundle/v1` JSON files.
	#[arg(long, default_value = "artifacts/github/bundles")]
	bundles_dir: PathBuf,
	/// Directory containing analysis draft JSON files.
	#[arg(long, default_value = "artifacts/github/analysis")]
	analysis_dir: PathBuf,
	/// Directory containing rendered `signal_entry/v1` JSON files.
	#[arg(long, default_value = "site/src/content/signals")]
	signals_dir: PathBuf,
}

#[derive(Debug, Args)]
struct RadarLedgerArtifactLinkCommand {
	/// GitHub repository in owner/name format.
	#[arg(long)]
	repo: String,
	/// Subject kind, either `commit` or `pr`.
	#[arg(long)]
	subject_kind: String,
	/// Subject id, either a commit SHA or pull request number.
	#[arg(long)]
	subject_id: String,
	/// Artifact kind to link.
	#[arg(long)]
	artifact_kind: String,
	/// Artifact path to digest and link.
	#[arg(long)]
	path: PathBuf,
}

#[derive(Debug, Args)]
struct RadarLedgerSummaryCommand {
	/// Emit machine-readable JSON.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct RadarBundleCommand {
	#[command(subcommand)]
	command: RadarBundleSubcommand,
}
impl RadarBundleCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			RadarBundleSubcommand::Build(args) => {
				let path = radar::build_bundle(&RadarBundleBuildRequest {
					repo: args.repo.clone(),
					pr: args.pr,
					commit: args.commit.clone(),
					force_commit_only: args.force_commit_only,
					token_env: args.token_env.clone(),
					out: args.out.clone(),
					notes: args.note.clone(),
				})?;

				println!("{}", path.display());

				Ok(())
			},
			RadarBundleSubcommand::Validate(args) => {
				let report = radar::validate_bundles(&RadarBundleValidateRequest {
					paths: args.paths.clone(),
				})?;

				println!("OK ({} GitHub change bundle JSON files validated)", report.checked_files);

				Ok(())
			},
		}
	}
}

#[derive(Debug, Args)]
struct RadarRefreshUpstreamQueueCommand {
	/// GitHub repository in owner/name format.
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	/// How many recent upstream commits to inspect.
	#[arg(long, default_value_t = 40)]
	search_limit: usize,
	/// Published signal directory used to suppress already-published subjects.
	#[arg(long, default_value = "site/src/content/signals")]
	signals_dir: PathBuf,
	/// Path to write the deterministic upstream_review_queue/v1 artifact.
	#[arg(long, default_value = "artifacts/github/review-queue/openai-codex-latest.json")]
	queue_out: PathBuf,
	/// Environment variable containing a GitHub token.
	#[arg(long)]
	token_env: Option<String>,
	/// Local SQLite Radar ledger path.
	#[arg(long, default_value = ".decodex/radar.sqlite3")]
	ledger: PathBuf,
	/// Disable local Radar ledger writes.
	#[arg(long)]
	no_ledger: bool,
	/// Print the queue without writing queue-out.
	#[arg(long)]
	dry_run: bool,
}
impl RadarRefreshUpstreamQueueCommand {
	fn run(&self) -> Result<()> {
		let report = radar::refresh_queue(&RadarRefreshQueueRequest {
			repo: self.repo.clone(),
			search_limit: self.search_limit,
			signals_dir: self.signals_dir.clone(),
			queue_out: self.queue_out.clone(),
			token_env: self.token_env.clone(),
			ledger: self.ledger.clone(),
			no_ledger: self.no_ledger,
			dry_run: self.dry_run,
		})?;

		if !self.dry_run {
			println!(
				"{}",
				serde_json::to_string(&serde_json::json!({
					"repo": self.repo,
					"recent_commits_scanned": report.recent_commits_scanned,
					"published_subjects_seen": report.published_subjects_seen,
					"subjects_queued": report.subjects_queued,
					"ledger_enabled": if report.ledger_enabled { 1 } else { 0 },
					"changed": report.changed,
					"queue_out": report.queue_out.display().to_string(),
				}))?
			);
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarRefreshReleaseDeltaCommand {
	/// GitHub repository in owner/name format.
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	/// Directory containing published signal-entry JSON files.
	#[arg(long, default_value = "site/src/content/signals")]
	signals_dir: PathBuf,
	/// Path to write the release-delta JSON artifact.
	#[arg(long, default_value = "site/src/content/release-deltas/openai-codex-latest.json")]
	out: PathBuf,
	/// Release tag prefix to scope the tracked channel.
	#[arg(long, default_value = "rust-v")]
	tag_prefix: String,
	/// Environment variable containing a GitHub token.
	#[arg(long)]
	token_env: Option<String>,
	/// Maximum recent stable releases to include. Use 0 for all releases at or above the floor.
	#[arg(long, default_value_t = 0)]
	stable_limit: usize,
	/// Maximum recent prereleases to include. Use 0 for all supported prereleases.
	#[arg(long, default_value_t = 0)]
	preview_limit: usize,
	/// Maximum signal-bearing compare entries. Use 0 for all valid pairs.
	#[arg(long, default_value_t = 24)]
	pair_limit: usize,
	/// Minimum stable tag to include in the comparator option set.
	#[arg(long, default_value = "rust-v0.116.0")]
	min_stable_tag: String,
	/// Print the release delta without writing out.
	#[arg(long)]
	dry_run: bool,
}
impl RadarRefreshReleaseDeltaCommand {
	fn run(&self) -> Result<()> {
		let report = radar::refresh_release_delta(&RadarRefreshReleaseDeltaRequest {
			repo: self.repo.clone(),
			signals_dir: self.signals_dir.clone(),
			out: self.out.clone(),
			tag_prefix: self.tag_prefix.clone(),
			token_env: self.token_env.clone(),
			stable_limit: self.stable_limit,
			preview_limit: self.preview_limit,
			pair_limit: self.pair_limit,
			min_stable_tag: self.min_stable_tag.clone(),
			dry_run: self.dry_run,
		})?;

		if !self.dry_run {
			println!(
				"{}",
				serde_json::to_string(&serde_json::json!({
					"repo": self.repo,
					"stable_tag_name": report.stable_tag_name,
					"prerelease_tag_name": report.prerelease_tag_name,
					"comparisons": report.comparisons,
					"changed": report.changed,
					"out": report.out.display().to_string(),
				}))?
			);
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarBundleBuildCommand {
	/// GitHub repository in owner/name format.
	#[arg(long)]
	repo: String,
	/// Pull request number to fetch.
	#[arg(long, conflicts_with = "commit", required_unless_present = "commit")]
	pr: Option<u64>,
	/// Commit SHA to fetch when PR context is unavailable.
	#[arg(long, required_unless_present = "pr")]
	commit: Option<String>,
	/// Skip PR lookup for commit input.
	#[arg(long, requires = "commit")]
	force_commit_only: bool,
	/// Environment variable name holding a GitHub token.
	#[arg(long)]
	token_env: Option<String>,
	/// Path to write the bundle JSON.
	#[arg(long)]
	out: PathBuf,
	/// Additional note strings to store in the bundle.
	#[arg(long)]
	note: Vec<String>,
}

#[derive(Debug, Args)]
struct RadarBundleValidateCommand {
	/// Bundle JSON files or directories.
	#[arg(value_name = "PATH")]
	paths: Vec<PathBuf>,
}

#[derive(Debug, Args)]
struct RadarValidateCommand {
	/// Radar JSON files or directories. Defaults to the checked-in Radar collections.
	#[arg(value_name = "PATH")]
	paths: Vec<PathBuf>,
}
impl RadarValidateCommand {
	fn run(&self) -> Result<()> {
		let report = radar::validate(&RadarValidateRequest { paths: self.paths.clone() })?;

		println!("OK ({} Radar artifact JSON files validated)", report.checked_files);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarRenderSignalCommand {
	/// Path to a github_change_bundle/v1 JSON artifact.
	#[arg(long)]
	bundle: PathBuf,
	/// Path to a Codex-owned analysis_draft JSON artifact.
	#[arg(long)]
	analysis: PathBuf,
	/// Path to write the rendered signal_entry/v1 artifact.
	#[arg(long)]
	out: PathBuf,
	/// Override the rendered publication timestamp.
	#[arg(long)]
	published_at: Option<String>,
}
impl RadarRenderSignalCommand {
	fn run(&self) -> Result<()> {
		let report = radar::render_signal(&RadarRenderSignalRequest {
			bundle: self.bundle.clone(),
			analysis: self.analysis.clone(),
			out: self.out.clone(),
			published_at: self.published_at.clone(),
		})?;

		println!("{}", report.out.display());

		Ok(())
	}
}

#[derive(Debug, Args)]
struct RadarBackfillReleaseRangeCommand {
	/// GitHub repository in owner/name format.
	#[arg(long, default_value = "openai/codex")]
	repo: String,
	/// Release-delta artifact to read or refresh.
	#[arg(long, default_value = "site/src/content/release-deltas/openai-codex-latest.json")]
	release_delta: PathBuf,
	/// Stable tag name to backfill from. Defaults to the top-level stable release.
	#[arg(long)]
	stable_tag: Option<String>,
	/// Preview tag name to backfill to. Defaults to the top-level prerelease.
	#[arg(long)]
	preview_tag: Option<String>,
	/// Directory containing published signal_entry/v1 artifacts.
	#[arg(long, default_value = "site/src/content/signals")]
	signals_dir: PathBuf,
	/// Directory for generated GitHub bundles.
	#[arg(long, default_value = "artifacts/github/bundles")]
	bundles_dir: PathBuf,
	/// Directory for Codex-owned analysis drafts.
	#[arg(long, default_value = "artifacts/github/analysis")]
	analysis_dir: PathBuf,
	/// Environment variable containing a GitHub token.
	#[arg(long)]
	token_env: Option<String>,
	/// Codex executable to invoke at the AI analysis boundary.
	#[arg(long, default_value = "codex")]
	codex_bin: String,
	/// Optional Codex model override.
	#[arg(long)]
	model: Option<String>,
	/// Optional PR cap for debugging or partial runs.
	#[arg(long)]
	max_prs: Option<usize>,
	/// Print selected PRs without generating new content.
	#[arg(long)]
	dry_run: bool,
	/// Refresh release_delta/v1 into a temporary file before selecting the prerelease range.
	#[arg(long)]
	refresh_release_delta_first: bool,
	/// Stable release limit passed through only by --refresh-release-delta-first.
	#[arg(long)]
	refresh_stable_limit: Option<usize>,
	/// Prerelease limit passed through only by --refresh-release-delta-first.
	#[arg(long)]
	refresh_preview_limit: Option<usize>,
	/// Compare pair limit passed through only by --refresh-release-delta-first.
	#[arg(long)]
	refresh_pair_limit: Option<usize>,
	/// Python executable for the Codex AI analysis helper boundary.
	#[arg(long, default_value = "python3")]
	python_bin: String,
}
impl RadarBackfillReleaseRangeCommand {
	fn run(&self) -> Result<()> {
		let report = radar::backfill_release_range(&RadarBackfillReleaseRangeRequest {
			repo: self.repo.clone(),
			release_delta: self.release_delta.clone(),
			stable_tag: self.stable_tag.clone(),
			preview_tag: self.preview_tag.clone(),
			signals_dir: self.signals_dir.clone(),
			bundles_dir: self.bundles_dir.clone(),
			analysis_dir: self.analysis_dir.clone(),
			token_env: self.token_env.clone(),
			codex_bin: self.codex_bin.clone(),
			model: self.model.clone(),
			max_prs: self.max_prs,
			dry_run: self.dry_run,
			refresh_release_delta_first: self.refresh_release_delta_first,
			refresh_stable_limit: self.refresh_stable_limit,
			refresh_preview_limit: self.refresh_preview_limit,
			refresh_pair_limit: self.refresh_pair_limit,
			python_bin: self.python_bin.clone(),
		})?;

		println!("{}", serde_json::to_string_pretty(&report)?);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct MaintenancePruneCommand {
	/// Report candidates without applying retention changes. This is the default mode.
	#[arg(long, conflicts_with = "apply")]
	dry_run: bool,
	/// Apply safe file retention, state-aware runtime compaction, and WAL checkpointing.
	#[arg(long, conflicts_with = "dry_run")]
	apply: bool,
	/// Emit the maintenance report as JSON.
	#[arg(long)]
	json: bool,
}
impl MaintenancePruneCommand {
	fn run(&self) -> Result<()> {
		let mode = if self.apply { MaintenanceMode::Apply } else { MaintenanceMode::DryRun };

		maintenance::run_prune_command(MaintenancePruneRequest {
			mode,
			scope: MaintenanceScope::Full,
			json: self.json,
		})
	}
}

#[derive(Debug, Args)]
struct ProbeCommand {
	/// Override the expected app-server transport during probing.
	#[arg(value_name = "TRANSPORT", default_value = "stdio://")]
	transport: String,
	/// Continue after warning when Codex app-server is outside the locally verified range.
	#[arg(long)]
	allow_unverified_codex: bool,
}
impl ProbeCommand {
	fn run(&self) -> Result<()> {
		let report = agent::probe_app_server(&self.transport, self.allow_unverified_codex)?;

		println!(
			"probe ok: compatibility={} support_decision={} codex_version={} supported_versions=\"{}\" capability_evidence=\"{}\" schema_evidence={} schema_cache={} schema_marker_count={} thread={} turn={} events={} output={}",
			report.capability_preflight.compatibility_status(),
			report.capability_preflight.compatibility_support_decision().unwrap_or("unknown"),
			report.capability_preflight.compatibility_codex_cli_version().unwrap_or("unknown"),
			report.capability_preflight.compatibility_supported_versions().unwrap_or("unknown"),
			report.capability_preflight.compatibility_capability_evidence().unwrap_or("unknown"),
			report.capability_preflight.compatibility_schema_evidence().unwrap_or("unknown"),
			report.capability_preflight.compatibility_schema_cache().unwrap_or("none"),
			report.capability_preflight.compatibility_schema_marker_count().unwrap_or("0"),
			report.thread_id,
			report.turn_id,
			report.event_count,
			report.final_output
		);

		tracing::info!(
			user_agent = %report.user_agent,
			thread_id = %report.thread_id,
			turn_id = %report.turn_id,
			event_count = report.event_count,
			"Completed probe."
		);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct AttemptCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Structured request file path, or `-` to read the request from stdin.
	#[arg(value_name = "REQUEST", default_value = "-")]
	request: String,
}
impl AttemptCommand {
	fn run(&self) -> Result<()> {
		let request = read_attempt_request(&self.request)?;

		orchestrator::run_once(RunOnceRequest {
			config_path: self.project_config.as_path(),
			dry_run: request.dry_run,
			explain_queue: false,
			preferred_issue_id: Some(request.issue_id.as_str()),
			preferred_issue_state: Some(request.issue_state.as_str()),
			preferred_initial_issue_state: request.initial_issue_state.as_deref(),
			preferred_lease_acquired: request.lease_preacquired,
			preferred_issue_claim_fd: request.issue_claim_fd,
			preferred_dispatch_slot_fd: request.dispatch_slot_fd,
			preferred_dispatch_slot_index: request.dispatch_slot_index,
			preferred_dispatch_mode: Some(request.dispatch_mode.into()),
			preferred_run_id: Some(request.run_id.as_str()),
			preferred_attempt_number: Some(request.attempt_number),
			preferred_retry_budget_base: Some(request.retry_budget_base),
			preferred_workflow_snapshot: Some(request.workflow_snapshot.as_str()),
			allow_unverified_codex: request.allow_unverified_codex,
		})
	}
}

impl From<AttemptDispatchMode> for IssueDispatchMode {
	fn from(value: AttemptDispatchMode) -> Self {
		match value {
			AttemptDispatchMode::Normal => Self::Normal,
			AttemptDispatchMode::Retry => Self::Retry,
			AttemptDispatchMode::ReviewRepair => Self::ReviewRepair,
			AttemptDispatchMode::Closeout => Self::Closeout,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AttemptDispatchMode {
	Normal,
	Retry,
	ReviewRepair,
	Closeout,
}
impl From<IssueDispatchMode> for AttemptDispatchMode {
	fn from(value: IssueDispatchMode) -> Self {
		match value {
			IssueDispatchMode::Normal => Self::Normal,
			IssueDispatchMode::Retry => Self::Retry,
			IssueDispatchMode::ReviewRepair => Self::ReviewRepair,
			IssueDispatchMode::Closeout => Self::Closeout,
		}
	}
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
enum Command {
	/// Create a signed local commit with a `decodex/commit/1` message.
	Commit(CommitCommand),
	/// Land the current reviewed lane with a GitHub admin merge commit.
	Land(LandCommand),
	/// Run one orchestration pass.
	Run(RunCommand),
	/// Run the local multi-project Decodex control plane.
	Serve(ServeCommand),
	/// Manage the local Decodex project registry.
	Project(ProjectCommand),
	/// Inspect or influence a local lane.
	Lane(LaneCommand),
	/// Inspect the current local runtime state for one configured project.
	Status(StatusCommand),
	/// Write and print the agent-readable local evidence index.
	Diagnose(DiagnoseCommand),
	/// Inspect local-only private execution evidence for one issue or run.
	Evidence(EvidenceCommand),
	/// Diagnose or explicitly repair supported retained-lane recovery cases.
	Recover(RecoverCommand),
	/// Dry-run or archive old terminal Linear issues by repo label.
	ArchiveLinear(ArchiveLinearCommand),
	/// Maintain local Decodex logs, evidence, backups, and runtime storage.
	Maintenance(MaintenanceCommand),
	/// Manage the global Decodex Codex account pool.
	Account(AccountCommand),
	/// Refresh and validate Decodex Radar artifacts.
	Radar(RadarCommand),
	/// Validate the local app-server integration boundary.
	Probe(ProbeCommand),
	/// Run one daemon-planned attempt from a structured request.
	#[command(name = "_attempt", hide = true)]
	Attempt(AttemptCommand),
}

#[derive(Debug, Subcommand)]
enum AccountSubcommand {
	/// List configured Codex accounts without printing token material.
	List(AccountListCommand),
	/// Pin new Decodex runs to one account.
	Select(AccountSelectCommand),
	/// Return new Decodex runs to balanced account selection.
	Clear(AccountClearCommand),
	/// Remove one account from the Decodex account pool.
	Logout(AccountLogoutCommand),
	/// Import an existing Codex `auth.json` into the Decodex account pool.
	ImportAuth(AccountImportCommand),
	/// Force Codex to use one stored account by overwriting its `auth.json`.
	Use(AccountUseCommand),
	/// Run Codex device login in an isolated temporary home, then import it.
	Login(AccountLoginCommand),
}

#[derive(Debug, Subcommand)]
enum ProjectSubcommand {
	/// Register or refresh one Decodex project config and enable it.
	Add(ProjectAddCommand),
	/// List registered local projects.
	List,
	/// Enable one registered project for `decodex serve`.
	Enable(ProjectToggleCommand),
	/// Disable one registered project for `decodex serve`.
	Disable(ProjectToggleCommand),
	/// Remove one registered project from the local registry.
	Remove(ProjectToggleCommand),
}

#[derive(Debug, Subcommand)]
enum LaneSubcommand {
	/// Inspect one local lane by issue identifier or tracker issue id.
	Inspect(LaneInspectCommand),
	/// Soft-interrupt an active app-server turn, with optional hard fallback.
	Interrupt(LaneInterruptCommand),
	/// Send operator-supplied text to an active steerable turn.
	Steer(LaneSteerCommand),
}

#[derive(Debug, Subcommand)]
enum RecoverSubcommand {
	/// Recover retained review lanes whose handoff marker is missing.
	ReviewHandoff(ReviewHandoffRecoveryCommand),
}

#[derive(Debug, Subcommand)]
enum ReviewHandoffRecoverySubcommand {
	/// Read-only diagnosis for orphaned retained review lanes.
	Diagnose(ReviewHandoffDiagnoseCommand),
	/// Explicitly bind a validated PR URL to one retained review lane.
	Rebind(ReviewHandoffRebindCommand),
}

#[derive(Debug, Subcommand)]
enum MaintenanceSubcommand {
	/// Inspect or apply local Decodex storage retention.
	Prune(MaintenancePruneCommand),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
enum RadarSubcommand {
	/// Build and validate deterministic GitHub change bundles.
	Bundle(RadarBundleCommand),
	/// Maintain the local Radar SQLite ledger.
	Ledger(RadarLedgerCommand),
	/// Refresh the deterministic upstream Radar review queue.
	RefreshUpstreamQueue(RadarRefreshUpstreamQueueCommand),
	/// Refresh the stable-versus-prerelease release-delta artifact.
	RefreshReleaseDelta(RadarRefreshReleaseDeltaCommand),
	/// Validate checked-in Radar artifact JSON contracts.
	Validate(RadarValidateCommand),
	/// Render a signal_entry/v1 artifact from a bundle plus Codex analysis draft.
	RenderSignal(RadarRenderSignalCommand),
	/// Select and optionally execute release-window signal backfills.
	BackfillReleaseRange(RadarBackfillReleaseRangeCommand),
}

#[derive(Debug, Subcommand)]
enum RadarLedgerSubcommand {
	/// Initialize the local Radar ledger schema.
	#[command(alias = "init")]
	Bootstrap,
	/// Ingest one bundle and optional derived artifacts.
	Ingest(RadarLedgerIngestCommand),
	/// Ingest existing checked-in bundles, analyses, and signals.
	IngestExisting(RadarLedgerIngestExistingCommand),
	/// Link one artifact path to a Radar subject.
	ArtifactLink(RadarLedgerArtifactLinkCommand),
	/// Print ledger counts.
	Summary(RadarLedgerSummaryCommand),
}

#[derive(Debug, Subcommand)]
enum RadarBundleSubcommand {
	/// Build a PR-first or commit-only GitHub change bundle.
	Build(RadarBundleBuildCommand),
	/// Validate one or more GitHub change bundle JSON files.
	Validate(RadarBundleValidateCommand),
}

fn default_lane_steer_wait_timeout_ms() -> u64 {
	u64::try_from(DEFAULT_STEER_RESULT_WAIT_TIMEOUT.as_millis()).unwrap_or(10_000)
}

fn lane_steer_report_is_failure(report: &LaneSteerReport) -> bool {
	matches!(report.outcome.as_str(), "rejected" | "failed" | "timed_out" | "fallback")
}

fn render_lane_steer_report(report: &LaneSteerReport) -> String {
	format!(
		"lane steer {}: issue={} run_id={} attempt={} expected_turn_id={} current_turn_id={} response_turn_id={} failure_class={} audit_record_id={} delivery_status={}\n",
		report.outcome,
		report.issue_identifier.as_deref().unwrap_or(&report.issue_id),
		report.run_id,
		report.attempt_number,
		report.expected_turn_id,
		report.current_turn_id.as_deref().unwrap_or("none"),
		report.response_turn_id.as_deref().unwrap_or("none"),
		report.failure_class.as_deref().unwrap_or("none"),
		report.audit_record_id,
		report.delivery_status
	)
}

fn read_attempt_request(request: &str) -> Result<AttemptRequest> {
	let raw = if request == "-" {
		let mut raw = String::new();

		io::stdin().read_to_string(&mut raw)?;

		raw
	} else {
		fs::read_to_string(request)?
	};

	serde_json::from_str(&raw).map_err(|error| {
		eyre::eyre!("Failed to parse `_attempt` request from `{}`: {error}", request)
	})
}

fn styles() -> Styles {
	Styles::styled()
		.header(AnsiColor::Red.on_default() | Effects::BOLD)
		.usage(AnsiColor::Red.on_default() | Effects::BOLD)
		.literal(AnsiColor::Blue.on_default() | Effects::BOLD)
		.placeholder(AnsiColor::Green.on_default())
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use clap::Parser;

	use crate::cli::{
		AccountCommand, AccountSubcommand, AccountUseCommand, AttemptCommand, Cli, Command,
		CommitCommand, DiagnoseCommand, EvidenceCommand, LandCommand, LaneCommand,
		LaneInspectCommand, LaneInterruptCommand, LaneSteerCommand, LaneSubcommand, ProbeCommand,
		ProjectCommand, ProjectConfigArgs, ProjectSubcommand, RadarBackfillReleaseRangeCommand,
		RadarBundleBuildCommand, RadarBundleCommand, RadarBundleSubcommand,
		RadarBundleValidateCommand, RadarCommand, RadarLedgerCommand,
		RadarLedgerIngestExistingCommand, RadarLedgerSubcommand, RadarLedgerSummaryCommand,
		RadarRefreshReleaseDeltaCommand, RadarRefreshUpstreamQueueCommand,
		RadarRenderSignalCommand, RadarSubcommand, RadarValidateCommand, RecoverCommand,
		RecoverSubcommand, ReviewHandoffDiagnoseCommand, ReviewHandoffRebindCommand,
		ReviewHandoffRecoveryCommand, ReviewHandoffRecoverySubcommand, RunCommand, ServeCommand,
		StatusCommand,
	};

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
	fn parses_commit_with_manual_authority() {
		let cli = Cli::parse_from(["decodex", "commit", "ship hotfix", "--manual-authority"]);

		assert!(matches!(
			cli.command,
			Command::Commit(CommitCommand { authority: None, manual_authority: true, .. })
		));
	}

	#[test]
	fn parses_land_with_manual_authority() {
		let cli = Cli::parse_from([
			"decodex",
			"land",
			"ship hotfix",
			"--manual-authority",
			"--pr",
			"https://github.com/hack-ink/decodex/pull/64",
		]);

		assert!(matches!(
			cli.command,
			Command::Land(LandCommand { authority: None, manual_authority: true, pr: Some(_), .. })
		));
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
	fn parses_run_with_positional_issue_and_dry_run() {
		let cli = Cli::parse_from(["decodex", "run", "issue-1", "--dry-run"]);

		assert!(matches!(
			cli.command,
			Command::Run(RunCommand { issue: Some(_), dry_run: true, explain: false, .. })
		));
	}

	#[test]
	fn parses_run_without_issue() {
		let cli = Cli::parse_from(["decodex", "run"]);

		assert!(matches!(
			cli.command,
			Command::Run(RunCommand { issue: None, dry_run: false, explain: false, .. })
		));
	}

	#[test]
	fn parses_run_dry_run_explain() {
		let cli = Cli::parse_from(["decodex", "run", "--dry-run", "--explain"]);

		assert!(matches!(
			cli.command,
			Command::Run(RunCommand { issue: None, dry_run: true, explain: true, .. })
		));

		let error = Cli::try_parse_from(["decodex", "run", "--explain"])
			.expect_err("explain should require dry-run");

		assert!(error.to_string().contains("--dry-run"));

		let error = Cli::try_parse_from(["decodex", "run", "issue-1", "--dry-run", "--explain"])
			.expect_err("explain should reject positional issue");

		assert!(error.to_string().contains("--explain"));
		assert!(error.to_string().contains("[ISSUE]"));
	}

	#[test]
	fn parses_serve_with_listen_address_and_project_config() {
		let cli = Cli::parse_from([
			"decodex",
			"serve",
			"--config",
			"./project.toml",
			"--listen-address",
			"127.0.0.1:9000",
		]);

		assert!(matches!(
		cli.command,
			Command::Serve(ServeCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				listen_address,
				dev,
				allow_unverified_codex,
			})
				if listen_address == "127.0.0.1:9000"
					&& !dev
					&& !allow_unverified_codex
					&& config == Path::new("./project.toml")
		));
	}

	#[test]
	fn parses_runtime_unverified_codex_override() {
		let cli = Cli::parse_from(["decodex", "run", "--allow-unverified-codex"]);

		assert!(matches!(
			cli.command,
			Command::Run(RunCommand { allow_unverified_codex: true, .. })
		));

		let cli = Cli::parse_from(["decodex", "serve", "--allow-unverified-codex"]);

		assert!(matches!(
			cli.command,
			Command::Serve(ServeCommand { allow_unverified_codex: true, .. })
		));

		let cli = Cli::parse_from(["decodex", "probe", "--allow-unverified-codex"]);

		assert!(matches!(
			cli.command,
			Command::Probe(ProbeCommand { allow_unverified_codex: true, .. })
		));
	}

	#[test]
	fn parses_serve_dev() {
		let cli = Cli::parse_from(["decodex", "serve", "--dev"]);

		assert!(matches!(cli.command, Command::Serve(ServeCommand { dev: true, .. })));
	}

	#[test]
	fn parses_radar_validate_paths() {
		let cli = Cli::parse_from(["decodex", "radar", "validate", "artifacts/github/bundles"]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::Validate(RadarValidateCommand { paths }),
			}) if paths == vec![Path::new("artifacts/github/bundles").to_path_buf()]
		));
	}

	#[test]
	fn parses_radar_render_signal_paths() {
		let cli = Cli::parse_from([
			"decodex",
			"radar",
			"render-signal",
			"--bundle",
			"artifacts/github/bundles/openai-codex-pr-1.json",
			"--analysis",
			"artifacts/github/analysis/openai-codex-pr-1.analysis.json",
			"--out",
			"site/src/content/signals/openai-codex-pr-1.json",
			"--published-at",
			"2026-06-01T00:00:00Z",
		]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::RenderSignal(RadarRenderSignalCommand {
					bundle,
					analysis,
					out,
					published_at: Some(published_at),
				}),
			}) if bundle == Path::new("artifacts/github/bundles/openai-codex-pr-1.json")
				&& analysis == Path::new("artifacts/github/analysis/openai-codex-pr-1.analysis.json")
				&& out == Path::new("site/src/content/signals/openai-codex-pr-1.json")
				&& published_at == "2026-06-01T00:00:00Z"
		));
	}

	#[test]
	fn parses_radar_backfill_release_range() {
		let cli = Cli::parse_from([
			"decodex",
			"radar",
			"backfill-release-range",
			"--repo",
			"openai/codex",
			"--stable-tag",
			"rust-v0.130.0",
			"--preview-tag",
			"rust-v0.131.0-alpha.9",
			"--max-prs",
			"2",
			"--dry-run",
		]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::BackfillReleaseRange(RadarBackfillReleaseRangeCommand {
					repo,
					stable_tag: Some(stable_tag),
					preview_tag: Some(preview_tag),
					max_prs: Some(2),
					dry_run: true,
					..
				}),
			}) if repo == "openai/codex"
				&& stable_tag == "rust-v0.130.0"
				&& preview_tag == "rust-v0.131.0-alpha.9"
		));
	}

	#[test]
	fn parses_radar_ledger_ingest_existing_defaults() {
		let cli = Cli::parse_from(["decodex", "radar", "ledger", "ingest-existing"]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::Ledger(RadarLedgerCommand {
					command: RadarLedgerSubcommand::IngestExisting(
						RadarLedgerIngestExistingCommand {
							bundles_dir,
							analysis_dir,
							signals_dir,
						}
					),
					..
				})
			}) if bundles_dir == Path::new("artifacts/github/bundles")
				&& analysis_dir == Path::new("artifacts/github/analysis")
				&& signals_dir == Path::new("site/src/content/signals")
		));
	}

	#[test]
	fn parses_radar_ledger_init_alias() {
		let cli = Cli::parse_from([
			"decodex",
			"radar",
			"ledger",
			"--db",
			".decodex/test-radar.sqlite3",
			"init",
		]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::Ledger(RadarLedgerCommand {
					db,
					command: RadarLedgerSubcommand::Bootstrap,
				})
			}) if db == Path::new(".decodex/test-radar.sqlite3")
		));
	}

	#[test]
	fn parses_radar_ledger_summary_json() {
		let cli = Cli::parse_from(["decodex", "radar", "ledger", "summary", "--json"]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::Ledger(RadarLedgerCommand {
					command: RadarLedgerSubcommand::Summary(RadarLedgerSummaryCommand {
						json: true
					}),
					..
				})
			})
		));
	}

	#[test]
	fn parses_radar_bundle_build_pr() {
		let cli = Cli::parse_from([
			"decodex",
			"radar",
			"bundle",
			"build",
			"--repo",
			"openai/codex",
			"--pr",
			"15222",
			"--out",
			"artifacts/github/bundles/openai-codex-pr-15222.json",
			"--note",
			"extra",
		]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::Bundle(RadarBundleCommand {
					command: RadarBundleSubcommand::Build(RadarBundleBuildCommand {
						repo,
						pr: Some(15_222),
						commit: None,
						out,
						note,
						..
					})
				})
			}) if repo == "openai/codex"
				&& out == Path::new("artifacts/github/bundles/openai-codex-pr-15222.json")
				&& note == vec!["extra".to_owned()]
		));
	}

	#[test]
	fn parses_radar_bundle_validate_paths() {
		let cli =
			Cli::parse_from(["decodex", "radar", "bundle", "validate", "artifacts/github/bundles"]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::Bundle(RadarBundleCommand {
					command: RadarBundleSubcommand::Validate(
						RadarBundleValidateCommand { paths }
					)
				})
			}) if paths == vec![Path::new("artifacts/github/bundles").to_path_buf()]
		));
	}

	#[test]
	fn parses_radar_refresh_upstream_queue() {
		let cli = Cli::parse_from([
			"decodex",
			"radar",
			"refresh-upstream-queue",
			"--repo",
			"openai/codex",
			"--token-env",
			"GITHUB_TOKEN",
			"--search-limit",
			"40",
		]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::RefreshUpstreamQueue(
					RadarRefreshUpstreamQueueCommand {
						repo,
						token_env: Some(token_env),
						search_limit: 40,
						queue_out,
						..
					}
				),
			}) if repo == "openai/codex"
				&& token_env == "GITHUB_TOKEN"
				&& queue_out == Path::new("artifacts/github/review-queue/openai-codex-latest.json")
		));
	}

	#[test]
	fn parses_radar_refresh_release_delta() {
		let cli = Cli::parse_from([
			"decodex",
			"radar",
			"refresh-release-delta",
			"--repo",
			"openai/codex",
			"--signals-dir",
			"site/src/content/signals",
			"--out",
			"site/src/content/release-deltas/openai-codex-latest.json",
			"--token-env",
			"GITHUB_TOKEN",
		]);

		assert!(matches!(
			cli.command,
			Command::Radar(RadarCommand {
				command: RadarSubcommand::RefreshReleaseDelta(
					RadarRefreshReleaseDeltaCommand {
						repo,
						token_env: Some(token_env),
						signals_dir,
						out,
						pair_limit: 24,
						..
					}
				),
			}) if repo == "openai/codex"
				&& token_env == "GITHUB_TOKEN"
				&& signals_dir == Path::new("site/src/content/signals")
				&& out == Path::new("site/src/content/release-deltas/openai-codex-latest.json")
		));
	}

	#[test]
	fn rejects_serve_interval_argument() {
		let error = Cli::try_parse_from(["decodex", "serve", "--interval", "30s"])
			.expect_err("serve interval override should be removed");
		let message = error.to_string();

		assert!(message.contains("--interval"));
	}

	#[test]
	fn parses_project_add() {
		let cli = Cli::parse_from(["decodex", "project", "add", "./project.toml"]);

		assert!(matches!(
			cli.command,
			Command::Project(ProjectCommand { command: ProjectSubcommand::Add(_) })
		));
	}

	#[test]
	fn parses_project_enable() {
		let cli = Cli::parse_from(["decodex", "project", "enable", "pubfi"]);

		assert!(matches!(
			cli.command,
			Command::Project(ProjectCommand { command: ProjectSubcommand::Enable(_) })
		));
	}

	#[test]
	fn parses_project_remove() {
		let cli = Cli::parse_from(["decodex", "project", "remove", "vibe-mono"]);

		assert!(matches!(
			cli.command,
			Command::Project(ProjectCommand { command: ProjectSubcommand::Remove(_) })
		));
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
		let error =
			Cli::try_parse_from(["decodex", "account", "list", "--config", "./project.toml"])
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
				issue,
				run_id: Some(_),
				attempt: Some(2),
				json: true,
				include_payload: true,
			}) if config == Path::new("./project.toml") && issue == "PUB-101"
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
}
