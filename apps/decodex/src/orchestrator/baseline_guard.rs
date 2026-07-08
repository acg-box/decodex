use std::{
	fs::{self, OpenOptions},
	io::{ErrorKind, Read as _},
	path::{Path, PathBuf},
	process::{Command, Output},
	thread,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use color_eyre::Report;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	commit_message::BASELINE_AUTHORITY,
	default_branch_sync,
	git_credentials::{GitCredentialEnvironment, GitCredentialSource},
	github,
	orchestrator::{
		IssueDispatchMode, OperatorSnapshotWarningDetail, ServiceConfig, StateStore,
		WorkflowDocument, git_ops,
	},
	prelude::{Result, eyre},
	state::ProjectLoopEvidenceSnapshot,
};

const BASELINE_ISSUE_ID: &str = "__baseline__";
const BASELINE_ATTEMPT_NUMBER: i64 = 1;
const LOCK_FILE_NAME: &str = ".decodex-baseline-normalization.lock";
const LOCK_SCHEMA: &str = "decodex/baseline_normalization_lock/1";
const MERGE_READBACK_TIMEOUT: Duration = Duration::from_secs(120);
const NORMALIZATION_WAIT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const NORMALIZATION_WAIT_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) const BASELINE_GUARD_CLEAN_EVENT_TYPE: &str = "baseline_guard_clean";
pub(crate) const BASELINE_GUARD_DIRTY_EVENT_TYPE: &str = "baseline_guard_dirty";
pub(crate) const BASELINE_GUARD_FAILED_EVENT_TYPE: &str = "baseline_guard_failed";
pub(crate) const BASELINE_NORMALIZATION_STARTED_EVENT_TYPE: &str = "baseline_normalization_started";
pub(crate) const BASELINE_NORMALIZATION_PR_CREATED_EVENT_TYPE: &str =
	"baseline_normalization_pr_created";
pub(crate) const BASELINE_NORMALIZATION_REPO_GATE_PASSED_EVENT_TYPE: &str =
	"baseline_normalization_repo_gate_passed";
pub(crate) const BASELINE_NORMALIZATION_REPO_GATE_FAILED_EVENT_TYPE: &str =
	"baseline_normalization_repo_gate_failed";
pub(crate) const BASELINE_NORMALIZATION_MERGED_EVENT_TYPE: &str = "baseline_normalization_merged";
pub(crate) const BASELINE_NORMALIZATION_RECHECK_PASSED_EVENT_TYPE: &str =
	"baseline_normalization_recheck_passed";
pub(crate) const BASELINE_NORMALIZATION_RECHECK_FAILED_EVENT_TYPE: &str =
	"baseline_normalization_recheck_failed";
pub(crate) const BASELINE_NORMALIZATION_FAILED_EVENT_TYPE: &str = "baseline_normalization_failed";

pub(crate) fn baseline_guard_applies_to_dispatch_mode(dispatch_mode: IssueDispatchMode) -> bool {
	matches!(
		dispatch_mode,
		IssueDispatchMode::Normal | IssueDispatchMode::Program | IssueDispatchMode::Retry
	)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGuardDispatchOutcome {
	Clean,
	NormalizedMain,
}

pub(crate) fn ensure_clean_baseline_before_dispatch(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dispatch_mode: IssueDispatchMode,
	dry_run: bool,
) -> Result<BaselineGuardDispatchOutcome> {
	if dry_run || !baseline_guard_applies_to_dispatch_mode(dispatch_mode) {
		return Ok(BaselineGuardDispatchOutcome::Clean);
	}
	let repo_gate = BaselineRepoGateCommands::from_workflow(workflow);
	if repo_gate.canonicalize_commands.is_empty() {
		return Ok(BaselineGuardDispatchOutcome::Clean);
	}

	let context = BaselineGuardContext::new(project, workflow)?;

	if let Some((event_type, payload)) =
		latest_baseline_event_for_context(project, state_store, &context)?
	{
		match event_type.as_str() {
			BASELINE_GUARD_CLEAN_EVENT_TYPE | BASELINE_NORMALIZATION_RECHECK_PASSED_EVENT_TYPE => {
			},
			BASELINE_NORMALIZATION_STARTED_EVENT_TYPE
			| BASELINE_NORMALIZATION_PR_CREATED_EVENT_TYPE
			| BASELINE_NORMALIZATION_REPO_GATE_PASSED_EVENT_TYPE
			| BASELINE_NORMALIZATION_MERGED_EVENT_TYPE => {
				if baseline_normalization_lock_is_active(project.worktree_root())? {
					return wait_for_existing_normalization(
						project,
						workflow,
						state_store,
						dispatch_mode,
					);
				}
				if let Some(outcome) = resume_existing_normalization_from_payload(
					project,
					&repo_gate,
					state_store,
					&context,
					&payload,
				)? {
					return Ok(outcome);
				}
			},
			BASELINE_NORMALIZATION_REPO_GATE_FAILED_EVENT_TYPE
			| BASELINE_NORMALIZATION_RECHECK_FAILED_EVENT_TYPE
			| BASELINE_NORMALIZATION_FAILED_EVENT_TYPE => {
				if !baseline_normalization_lock_is_active(project.worktree_root())?
					&& let Some(outcome) = resume_existing_normalization_from_payload(
						project,
						&repo_gate,
						state_store,
						&context,
						&payload,
					)? {
					return Ok(outcome);
				}
			},
			_ => {},
		}
	}

	let guard_run_id = context.guard_run_id();

	match run_guard_once(project, &repo_gate, state_store, &context, &guard_run_id)? {
		BaselineGuardCheck::Clean =>
			if sync_repo_root_to_guarded_main(project, &context)? {
				Ok(BaselineGuardDispatchOutcome::Clean)
			} else {
				Ok(BaselineGuardDispatchOutcome::NormalizedMain)
			},
		BaselineGuardCheck::Dirty => {
			let Some(_lock) =
				BaselineNormalizationLock::acquire(project.worktree_root(), &context)?
			else {
				return wait_for_existing_normalization(
					project,
					workflow,
					state_store,
					dispatch_mode,
				);
			};

			let merged_main_oid =
				run_baseline_normalization(project, &repo_gate, state_store, &context)?;
			finish_normalized_main(project, &repo_gate, state_store, &context, merged_main_oid)
		},
	}
}

fn finish_normalized_main(
	project: &ServiceConfig,
	repo_gate: &BaselineRepoGateCommands,
	state_store: &StateStore,
	context: &BaselineGuardContext,
	merged_main_oid: String,
) -> Result<BaselineGuardDispatchOutcome> {
	let recheck_context = context.with_main_oid(merged_main_oid);
	let recheck_run_id = recheck_context.recheck_run_id();

	match run_guard_once(project, repo_gate, state_store, &recheck_context, &recheck_run_id) {
		Ok(BaselineGuardCheck::Clean) => {
			record_event(
				project,
				state_store,
				&recheck_run_id,
				BASELINE_NORMALIZATION_RECHECK_PASSED_EVENT_TYPE,
				recheck_context.payload(),
			)?;
			Ok(BaselineGuardDispatchOutcome::NormalizedMain)
		},
		Ok(BaselineGuardCheck::Dirty) => {
			record_event(
				project,
				state_store,
				&recheck_run_id,
				BASELINE_NORMALIZATION_RECHECK_FAILED_EVENT_TYPE,
				recheck_context.payload(),
			)?;
			eyre::bail!(
				"Baseline canonicalization still rewrites tracked files after normalization for `{}` at `{}`.",
				project.service_id(),
				recheck_context.main_oid
			)
		},
		Err(error) => {
			record_event(
				project,
				state_store,
				&recheck_run_id,
				BASELINE_NORMALIZATION_RECHECK_FAILED_EVENT_TYPE,
				payload_with_error(recheck_context.payload(), &error),
			)?;
			Err(error)
		},
	}
}

fn resume_existing_normalization_from_payload(
	project: &ServiceConfig,
	repo_gate: &BaselineRepoGateCommands,
	state_store: &StateStore,
	context: &BaselineGuardContext,
	payload: &Value,
) -> Result<Option<BaselineGuardDispatchOutcome>> {
	let run = BaselineNormalizationRun::new(project, context);

	if let Some(merge_commit) = payload.get("merge_commit").and_then(Value::as_str) {
		let merged_main_oid =
			resolve_merged_baseline_main(project, state_store, context, &run, merge_commit)?;

		return finish_normalized_main(project, repo_gate, state_store, context, merged_main_oid)
			.map(Some);
	}

	let Some(pr_url) = payload.get("pr_url").and_then(Value::as_str) else {
		return Ok(None);
	};
	let Some(head_oid) = payload.get("head_oid").and_then(Value::as_str) else {
		return Ok(None);
	};

	with_detached_worktree(project.repo_root(), &run.path, head_oid, &context.git_env, || {
		let merge_commit =
			merge_baseline_normalization_pr(project, state_store, context, &run, head_oid, pr_url)?;
		let merged_main_oid =
			resolve_merged_baseline_main(project, state_store, context, &run, &merge_commit)?;

		finish_normalized_main(project, repo_gate, state_store, context, merged_main_oid).map(Some)
	})
}

pub(crate) fn push_baseline_status_projection(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	warnings: &mut Vec<String>,
	warning_details: &mut Vec<OperatorSnapshotWarningDetail>,
) {
	let Some(event) = loop_evidence.private_events_for_issue(BASELINE_ISSUE_ID).into_iter().last()
	else {
		return;
	};
	let warning = match event.event_type() {
		BASELINE_GUARD_CLEAN_EVENT_TYPE | BASELINE_NORMALIZATION_RECHECK_PASSED_EVENT_TYPE => {
			return;
		},
		BASELINE_GUARD_DIRTY_EVENT_TYPE => "baseline_guard_dirty",
		BASELINE_GUARD_FAILED_EVENT_TYPE => "baseline_guard_failed",
		BASELINE_NORMALIZATION_STARTED_EVENT_TYPE
		| BASELINE_NORMALIZATION_PR_CREATED_EVENT_TYPE
		| BASELINE_NORMALIZATION_REPO_GATE_PASSED_EVENT_TYPE
		| BASELINE_NORMALIZATION_MERGED_EVENT_TYPE => "baseline_normalization_in_progress",
		BASELINE_NORMALIZATION_REPO_GATE_FAILED_EVENT_TYPE => "baseline_normalization_gate_failed",
		BASELINE_NORMALIZATION_RECHECK_FAILED_EVENT_TYPE => "baseline_normalization_recheck_failed",
		BASELINE_NORMALIZATION_FAILED_EVENT_TYPE => "baseline_normalization_failed",
		_ => return,
	};

	warnings.push(String::from(warning));
	warning_details.push(OperatorSnapshotWarningDetail {
		warning: String::from(warning),
		project_id: Some(project.service_id().to_owned()),
		repo_root: Some(project.repo_root().display().to_string()),
		reason: baseline_status_reason(event.event_type(), event.payload()),
		next_action: Some(String::from(
			"let Decodex finish automatic baseline normalization before dispatching ordinary task lanes",
		)),
	});
}

enum BaselineGuardCheck {
	Clean,
	Dirty,
}

struct BaselineRepoGateCommands {
	canonicalize_commands: Vec<String>,
	verify_commands: Vec<String>,
}
impl BaselineRepoGateCommands {
	fn from_workflow(workflow: &WorkflowDocument) -> Self {
		let execution = workflow.frontmatter().execution();

		Self {
			canonicalize_commands: execution.canonicalize_commands().to_vec(),
			verify_commands: execution.verify_commands().to_vec(),
		}
	}
}

struct BaselineGuardContext {
	default_branch: String,
	main_oid: String,
	workflow_hash: String,
	github_token: String,
	git_env: GitCredentialEnvironment,
}
impl BaselineGuardContext {
	fn new(project: &ServiceConfig, workflow: &WorkflowDocument) -> Result<Self> {
		let github_token = project.github().resolve_token()?;
		let git_env = GitCredentialSource::new(project.github().token_env_var(), &github_token)
			.materialize_github_credentials();
		let default_branch = resolve_origin_default_branch(project.repo_root(), &git_env)?;
		let main_oid =
			fetch_and_resolve_origin_branch(project.repo_root(), &default_branch, &git_env)?;
		let workflow_hash = workflow_hash(workflow)?;

		Ok(Self { default_branch, main_oid, workflow_hash, github_token, git_env })
	}

	fn binding_id(&self) -> String {
		format!("{}-{}", short_oid(&self.main_oid), &self.workflow_hash[..12])
	}

	fn unique_binding_id(&self) -> String {
		let timestamp =
			SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_nanos();

		format!("{}-{}-{timestamp}", self.binding_id(), std::process::id())
	}

	fn guard_run_id(&self) -> String {
		format!("baseline-guard-{}", self.binding_id())
	}

	fn normalization_run_id(&self) -> String {
		format!("baseline-normalization-{}", self.binding_id())
	}

	fn recheck_run_id(&self) -> String {
		format!("baseline-recheck-{}", self.binding_id())
	}

	fn payload(&self) -> Value {
		serde_json::json!({
			"schema": "decodex/baseline_guard/1",
			"default_branch": self.default_branch,
			"main_oid": self.main_oid,
			"workflow_hash": self.workflow_hash,
		})
	}

	fn with_main_oid(&self, main_oid: String) -> Self {
		Self {
			default_branch: self.default_branch.clone(),
			main_oid,
			workflow_hash: self.workflow_hash.clone(),
			github_token: self.github_token.clone(),
			git_env: self.git_env.clone(),
		}
	}
}

struct BaselineNormalizationLock {
	path: PathBuf,
}
impl BaselineNormalizationLock {
	fn acquire(worktree_root: &Path, context: &BaselineGuardContext) -> Result<Option<Self>> {
		fs::create_dir_all(worktree_root)?;
		let path = worktree_root.join(LOCK_FILE_NAME);

		remove_stale_lock_if_needed(&path)?;

		let lock_payload = serde_json::json!({
			"schema": LOCK_SCHEMA,
			"main_oid": context.main_oid,
			"workflow_hash": context.workflow_hash,
			"pid": std::process::id(),
			"created_at_unix": time::OffsetDateTime::now_utc().unix_timestamp(),
		});
		let file_result = OpenOptions::new().write(true).create_new(true).open(&path);

		match file_result {
			Ok(mut file) => {
				use std::io::Write as _;

				file.write_all(lock_payload.to_string().as_bytes())?;
				Ok(Some(Self { path }))
			},
			Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(None),
			Err(error) => Err(error.into()),
		}
	}
}
impl Drop for BaselineNormalizationLock {
	fn drop(&mut self) {
		let _ = fs::remove_file(&self.path);
	}
}

fn baseline_normalization_lock_is_active(worktree_root: &Path) -> Result<bool> {
	let lock_path = worktree_root.join(LOCK_FILE_NAME);

	remove_stale_lock_if_needed(&lock_path)?;

	Ok(lock_path.exists())
}

fn sync_repo_root_to_guarded_main(
	project: &ServiceConfig,
	context: &BaselineGuardContext,
) -> Result<bool> {
	default_branch_sync::sync_repo_root_default_branch(
		project.repo_root(),
		&context.default_branch,
		Some(GitCredentialSource::new(project.github().token_env_var(), &context.github_token)),
	)?;
	let local_main_oid =
		git_capture(project.repo_root(), &["rev-parse", "HEAD"], &context.git_env)?;

	Ok(local_main_oid == context.main_oid)
}

fn run_guard_once(
	project: &ServiceConfig,
	repo_gate: &BaselineRepoGateCommands,
	state_store: &StateStore,
	context: &BaselineGuardContext,
	run_id: &str,
) -> Result<BaselineGuardCheck> {
	let guard_path = baseline_worktree_path(project, "guard", &context.unique_binding_id());

	remove_worktree_best_effort(project.repo_root(), &guard_path, &context.git_env);
	if let Some(parent) = guard_path.parent() {
		fs::create_dir_all(parent)?;
	}
	git_checked(
		project.repo_root(),
		&["worktree", "add", "--detach", path_arg(&guard_path).as_str(), &context.main_oid],
		"create baseline guard worktree",
		&context.git_env,
	)?;

	let result = git_ops::run_canonicalize_commands(&repo_gate.canonicalize_commands, &guard_path);

	match result {
		Ok(()) => {
			let has_tracked_changes = worktree_has_tracked_changes(&guard_path, &context.git_env)?;
			remove_worktree_best_effort(project.repo_root(), &guard_path, &context.git_env);
			let event_type = if has_tracked_changes {
				BASELINE_GUARD_DIRTY_EVENT_TYPE
			} else {
				BASELINE_GUARD_CLEAN_EVENT_TYPE
			};
			record_event(project, state_store, run_id, event_type, context.payload())?;
			Ok(if has_tracked_changes {
				BaselineGuardCheck::Dirty
			} else {
				BaselineGuardCheck::Clean
			})
		},
		Err(error) => {
			record_event(
				project,
				state_store,
				run_id,
				BASELINE_GUARD_FAILED_EVENT_TYPE,
				payload_with_error(context.payload(), &error),
			)?;
			remove_worktree_best_effort(project.repo_root(), &guard_path, &context.git_env);
			Err(error)
		},
	}
}

fn run_baseline_normalization(
	project: &ServiceConfig,
	repo_gate: &BaselineRepoGateCommands,
	state_store: &StateStore,
	context: &BaselineGuardContext,
) -> Result<String> {
	let run = BaselineNormalizationRun::new(project, context);

	record_baseline_normalization_started(project, state_store, context, &run)?;

	with_branch_worktree(
		project.repo_root(),
		&run.path,
		&run.branch_name,
		&context.main_oid,
		&context.git_env,
		|| execute_baseline_normalization(project, repo_gate, state_store, context, &run),
	)
}

struct BaselineNormalizationRun {
	run_id: String,
	branch_name: String,
	path: PathBuf,
}
impl BaselineNormalizationRun {
	fn new(project: &ServiceConfig, context: &BaselineGuardContext) -> Self {
		let binding_id = context.binding_id();

		Self {
			run_id: context.normalization_run_id(),
			branch_name: format!(
				"xy/{}-baseline-normalize-{}",
				git_ref_component(project.service_id()),
				binding_id.as_str()
			),
			path: baseline_worktree_path(project, "normalization", &binding_id),
		}
	}
}

fn record_baseline_normalization_started(
	project: &ServiceConfig,
	state_store: &StateStore,
	context: &BaselineGuardContext,
	run: &BaselineNormalizationRun,
) -> Result<()> {
	record_event(
		project,
		state_store,
		&run.run_id,
		BASELINE_NORMALIZATION_STARTED_EVENT_TYPE,
		payload_with_fields(
			context.payload(),
			[
				("branch", Value::String(run.branch_name.clone())),
				("worktree", Value::String(run.path.display().to_string())),
			],
		),
	)
}

fn execute_baseline_normalization(
	project: &ServiceConfig,
	repo_gate: &BaselineRepoGateCommands,
	state_store: &StateStore,
	context: &BaselineGuardContext,
	run: &BaselineNormalizationRun,
) -> Result<String> {
	git_ops::run_canonicalize_commands(&repo_gate.canonicalize_commands, &run.path).inspect_err(
		|error| {
			let _ = record_normalization_failed(project, state_store, &run.run_id, context, error);
		},
	)?;
	commit_normalization_diff(&run.path, context).inspect_err(|error| {
		let _ = record_normalization_failed(project, state_store, &run.run_id, context, error);
	})?;
	let head_oid = git_capture(&run.path, &["rev-parse", "HEAD"], &context.git_env)?;
	run_baseline_normalization_repo_gate(
		project,
		repo_gate,
		state_store,
		context,
		run,
		&head_oid,
		None,
	)?;
	let pr_url = push_baseline_normalization_pr(project, state_store, context, run, &head_oid)?;
	let merge_commit =
		merge_baseline_normalization_pr(project, state_store, context, run, &head_oid, &pr_url)?;

	resolve_merged_baseline_main(project, state_store, context, run, &merge_commit)
}

fn push_baseline_normalization_pr(
	project: &ServiceConfig,
	state_store: &StateStore,
	context: &BaselineGuardContext,
	run: &BaselineNormalizationRun,
	head_oid: &str,
) -> Result<String> {
	git_checked(
		&run.path,
		&["push", "-u", "origin", &run.branch_name],
		"push baseline normalization branch",
		&context.git_env,
	)
	.inspect_err(|error| {
		let _ = record_normalization_failed(project, state_store, &run.run_id, context, error);
	})?;
	let pr_url = create_pull_request(
		&run.path,
		&run.branch_name,
		&context.default_branch,
		&context.github_token,
		project.github().command_path(),
	)
	.inspect_err(|error| {
		let _ = record_normalization_failed(project, state_store, &run.run_id, context, error);
	})?;

	record_event(
		project,
		state_store,
		&run.run_id,
		BASELINE_NORMALIZATION_PR_CREATED_EVENT_TYPE,
		normalization_payload(context, run, head_oid, Some(&pr_url)),
	)?;

	Ok(pr_url)
}

fn run_baseline_normalization_repo_gate(
	project: &ServiceConfig,
	repo_gate: &BaselineRepoGateCommands,
	state_store: &StateStore,
	context: &BaselineGuardContext,
	run: &BaselineNormalizationRun,
	head_oid: &str,
	pr_url: Option<&str>,
) -> Result<()> {
	match git_ops::run_repo_gate_commands(
		&repo_gate.canonicalize_commands,
		&repo_gate.verify_commands,
		&run.path,
	) {
		Ok(()) => record_event(
			project,
			state_store,
			&run.run_id,
			BASELINE_NORMALIZATION_REPO_GATE_PASSED_EVENT_TYPE,
			normalization_payload(context, run, head_oid, pr_url),
		),
		Err(error) => {
			record_event(
				project,
				state_store,
				&run.run_id,
				BASELINE_NORMALIZATION_REPO_GATE_FAILED_EVENT_TYPE,
				payload_with_error(normalization_payload(context, run, head_oid, pr_url), &error),
			)?;
			Err(error)
		},
	}
}

fn merge_baseline_normalization_pr(
	project: &ServiceConfig,
	state_store: &StateStore,
	context: &BaselineGuardContext,
	run: &BaselineNormalizationRun,
	head_oid: &str,
	pr_url: &str,
) -> Result<String> {
	let merge_subject = baseline_commit_message("Normalize repo gate baseline");

	github::admin_merge_pull_request(
		&run.path,
		pr_url,
		head_oid,
		Some(merge_subject.as_str()),
		&context.github_token,
		project.github().command_path(),
	)
	.inspect_err(|error| {
		let _ = record_event(
			project,
			state_store,
			&run.run_id,
			BASELINE_NORMALIZATION_FAILED_EVENT_TYPE,
			payload_with_error(normalization_payload(context, run, head_oid, Some(pr_url)), error),
		);
	})?;
	let merge_commit = github::wait_for_pull_request_merge_commit(
		&run.path,
		pr_url,
		&context.github_token,
		MERGE_READBACK_TIMEOUT,
		project.github().command_path(),
	)
	.inspect_err(|error| {
		let _ = record_event(
			project,
			state_store,
			&run.run_id,
			BASELINE_NORMALIZATION_FAILED_EVENT_TYPE,
			payload_with_error(normalization_payload(context, run, head_oid, Some(pr_url)), error),
		);
	})?;

	record_event(
		project,
		state_store,
		&run.run_id,
		BASELINE_NORMALIZATION_MERGED_EVENT_TYPE,
		payload_with_fields(
			normalization_payload(context, run, head_oid, Some(pr_url)),
			[("merge_commit", Value::String(merge_commit.clone()))],
		),
	)?;

	Ok(merge_commit)
}

fn resolve_merged_baseline_main(
	project: &ServiceConfig,
	state_store: &StateStore,
	context: &BaselineGuardContext,
	run: &BaselineNormalizationRun,
	merge_commit: &str,
) -> Result<String> {
	let merged_main_oid = fetch_and_resolve_origin_branch(
		project.repo_root(),
		&context.default_branch,
		&context.git_env,
	)?;
	if merged_main_oid != merge_commit {
		let error = eyre::eyre!(
			"Baseline normalization merge readback returned `{merge_commit}`, but `origin/{}` resolved to `{merged_main_oid}` after fetch.",
			context.default_branch
		);
		record_event(
			project,
			state_store,
			&run.run_id,
			BASELINE_NORMALIZATION_FAILED_EVENT_TYPE,
			payload_with_error(
				payload_with_fields(
					context.payload(),
					[("merge_commit", Value::String(merge_commit.to_owned()))],
				),
				&error,
			),
		)?;

		return Err(error);
	}
	default_branch_sync::sync_repo_root_default_branch(
		project.repo_root(),
		&context.default_branch,
		Some(GitCredentialSource::new(project.github().token_env_var(), &context.github_token)),
	)
	.inspect_err(|error| {
		let _ = record_normalization_failed(project, state_store, &run.run_id, context, error);
	})?;
	let local_main_oid =
		git_capture(project.repo_root(), &["rev-parse", "HEAD"], &context.git_env)?;
	if local_main_oid != merge_commit {
		let error = eyre::eyre!(
			"Baseline normalization synced `origin/{}` to `{merge_commit}`, but the local repo root HEAD is `{local_main_oid}`.",
			context.default_branch
		);
		record_event(
			project,
			state_store,
			&run.run_id,
			BASELINE_NORMALIZATION_FAILED_EVENT_TYPE,
			payload_with_error(
				payload_with_fields(
					context.payload(),
					[("merge_commit", Value::String(merge_commit.to_owned()))],
				),
				&error,
			),
		)?;

		return Err(error);
	}

	Ok(merged_main_oid)
}

fn normalization_payload(
	context: &BaselineGuardContext,
	run: &BaselineNormalizationRun,
	head_oid: &str,
	pr_url: Option<&str>,
) -> Value {
	let payload = payload_with_fields(
		context.payload(),
		[
			("head_oid", Value::String(head_oid.to_owned())),
			("branch", Value::String(run.branch_name.clone())),
			("worktree", Value::String(run.path.display().to_string())),
		],
	);

	match pr_url {
		Some(pr_url) =>
			payload_with_fields(payload, [("pr_url", Value::String(pr_url.to_owned()))]),
		None => payload,
	}
}

fn commit_normalization_diff(
	normalization_path: &Path,
	context: &BaselineGuardContext,
) -> Result<()> {
	git_checked(
		normalization_path,
		&["add", "--update"],
		"stage baseline normalization diff",
		&context.git_env,
	)?;

	if !worktree_has_staged_changes(normalization_path, &context.git_env)? {
		eyre::bail!(
			"Baseline canonicalize produced no staged diff in `{}`.",
			normalization_path.display()
		);
	}

	let message = baseline_commit_message("Normalize repo gate baseline");

	git_checked(
		normalization_path,
		&["commit", "-m", message.as_str()],
		"commit baseline normalization diff",
		&context.git_env,
	)?;

	Ok(())
}

fn create_pull_request(
	cwd: &Path,
	branch_name: &str,
	default_branch: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let mut command = github::gh_command_with_config(gh_command_path);

	command.current_dir(cwd).args([
		"pr",
		"create",
		"--fill",
		"--base",
		default_branch,
		"--head",
		branch_name,
	]);
	github::configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if output.status.success() {
		let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();

		if stdout.is_empty() {
			eyre::bail!("GitHub CLI created no PR URL for branch `{branch_name}`.");
		}

		return Ok(stdout);
	}

	eyre::bail!(
		"Failed to create baseline normalization PR for `{branch_name}`: {}",
		output_text(&output)
	)
}

fn with_branch_worktree<F, T>(
	repo_root: &Path,
	worktree_path: &Path,
	branch_name: &str,
	start_oid: &str,
	git_env: &GitCredentialEnvironment,
	action: F,
) -> Result<T>
where
	F: FnOnce() -> Result<T>,
{
	remove_worktree_best_effort(repo_root, worktree_path, git_env);
	if let Some(parent) = worktree_path.parent() {
		fs::create_dir_all(parent)?;
	}
	let _ = git_status(repo_root, &["branch", "-D", branch_name], git_env);
	git_checked(
		repo_root,
		&["worktree", "add", "-b", branch_name, path_arg(worktree_path).as_str(), start_oid],
		"create baseline normalization worktree",
		git_env,
	)?;

	let result = action();
	if result.is_ok() {
		remove_worktree_best_effort(repo_root, worktree_path, git_env);
		let _ = git_status(repo_root, &["branch", "-D", branch_name], git_env);
	}
	result
}

fn with_detached_worktree<F, T>(
	repo_root: &Path,
	worktree_path: &Path,
	start_oid: &str,
	git_env: &GitCredentialEnvironment,
	action: F,
) -> Result<T>
where
	F: FnOnce() -> Result<T>,
{
	remove_worktree_best_effort(repo_root, worktree_path, git_env);
	if let Some(parent) = worktree_path.parent() {
		fs::create_dir_all(parent)?;
	}
	git_checked(
		repo_root,
		&["worktree", "add", "--detach", path_arg(worktree_path).as_str(), start_oid],
		"create baseline normalization resume worktree",
		git_env,
	)?;

	let result = action();
	if result.is_ok() {
		remove_worktree_best_effort(repo_root, worktree_path, git_env);
	}
	result
}

fn baseline_worktree_path(project: &ServiceConfig, kind: &str, binding_id: &str) -> PathBuf {
	project.worktree_root().join(".baseline").join(format!(
		"{}-{}-{binding_id}",
		project.service_id(),
		kind
	))
}

fn resolve_origin_default_branch(
	repo_root: &Path,
	git_env: &GitCredentialEnvironment,
) -> Result<String> {
	git_checked(repo_root, &["fetch", "origin"], "fetch origin for baseline guard", git_env)?;

	let output = git_status(
		repo_root,
		&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"],
		git_env,
	)?;
	if output.status.success() {
		let remote_head = output_text(&output);
		if let Some(default_branch) = remote_head.strip_prefix("origin/") {
			return Ok(default_branch.to_owned());
		}
	}

	let output = git_checked(
		repo_root,
		&["ls-remote", "--symref", "origin", "HEAD"],
		"resolve origin default branch",
		git_env,
	)?;
	let stdout = String::from_utf8_lossy(&output.stdout);

	stdout
		.lines()
		.find_map(|line| {
			line.trim()
				.strip_prefix("ref: refs/heads/")
				.and_then(|value| value.strip_suffix("\tHEAD"))
				.map(str::to_owned)
		})
		.ok_or_else(|| eyre::eyre!("Remote `origin` did not advertise a default branch."))
}

fn fetch_and_resolve_origin_branch(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<String> {
	let refspec = format!("refs/heads/{default_branch}:refs/remotes/origin/{default_branch}");

	git_checked(
		repo_root,
		&["fetch", "origin", refspec.as_str()],
		"fetch origin default branch",
		git_env,
	)?;

	git_capture(repo_root, &["rev-parse", &format!("origin/{default_branch}")], git_env)
}

fn git_checked(
	cwd: &Path,
	args: &[&str],
	action: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<Output> {
	let output = git_status(cwd, args, git_env)?;

	if output.status.success() {
		return Ok(output);
	}

	eyre::bail!("Failed to {action} in `{}`: {}", cwd.display(), output_text(&output))
}

fn git_capture(cwd: &Path, args: &[&str], git_env: &GitCredentialEnvironment) -> Result<String> {
	let output = git_checked(cwd, args, "run git command", git_env)?;

	Ok(output_text(&output))
}

fn git_status(cwd: &Path, args: &[&str], git_env: &GitCredentialEnvironment) -> Result<Output> {
	let mut command = Command::new("git");

	command.arg("-C").arg(cwd).args(args);
	git_env.apply_to(&mut command);

	command.output().map_err(Report::new)
}

fn worktree_has_tracked_changes(
	worktree_path: &Path,
	git_env: &GitCredentialEnvironment,
) -> Result<bool> {
	let unstaged = git_status(worktree_path, &["diff", "--quiet"], git_env)?;
	let staged = git_status(worktree_path, &["diff", "--cached", "--quiet"], git_env)?;

	Ok(!unstaged.status.success() || !staged.status.success())
}

fn worktree_has_staged_changes(
	worktree_path: &Path,
	git_env: &GitCredentialEnvironment,
) -> Result<bool> {
	let output = git_status(worktree_path, &["diff", "--cached", "--quiet"], git_env)?;

	Ok(!output.status.success())
}

fn remove_worktree_best_effort(
	repo_root: &Path,
	worktree_path: &Path,
	git_env: &GitCredentialEnvironment,
) {
	if worktree_path.exists() {
		let _ = git_status(
			repo_root,
			&["worktree", "remove", "--force", path_arg(worktree_path).as_str()],
			git_env,
		);
		let _ = fs::remove_dir_all(worktree_path);
	}
}

fn remove_stale_lock_if_needed(path: &Path) -> Result<()> {
	let Ok(mut file) = fs::File::open(path) else {
		return Ok(());
	};
	let mut contents = String::new();
	file.read_to_string(&mut contents)?;
	let payload = serde_json::from_str::<Value>(&contents).ok();
	let lock_has_valid_schema =
		payload.as_ref().and_then(|payload| payload.get("schema").and_then(Value::as_str))
			== Some(LOCK_SCHEMA);
	let lock_has_live_process = payload
		.as_ref()
		.and_then(|payload| payload.get("pid").and_then(Value::as_i64))
		.is_some_and(process_is_alive);

	if !lock_has_valid_schema || !lock_has_live_process {
		fs::remove_file(path)?;
	}

	Ok(())
}

fn baseline_commit_message(change: &str) -> String {
	serde_json::json!({
		"schema": "decodex/commit/2",
		"change": change,
		"authority": BASELINE_AUTHORITY,
		"impact": "compatible"
	})
	.to_string()
}

fn process_is_alive(pid: i64) -> bool {
	if pid <= 0 {
		return false;
	}

	Command::new("kill")
		.args(["-0", &pid.to_string()])
		.output()
		.map(|output| output.status.success())
		.unwrap_or(false)
}

fn wait_for_existing_normalization(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dispatch_mode: IssueDispatchMode,
) -> Result<BaselineGuardDispatchOutcome> {
	let lock_path = project.worktree_root().join(LOCK_FILE_NAME);
	let deadline = Instant::now() + NORMALIZATION_WAIT_TIMEOUT;

	loop {
		remove_stale_lock_if_needed(&lock_path)?;

		if !lock_path.exists() {
			return ensure_clean_baseline_before_dispatch(
				project,
				workflow,
				state_store,
				dispatch_mode,
				false,
			);
		}

		if Instant::now() >= deadline {
			eyre::bail!(
				"Timed out waiting for existing baseline normalization for `{}` to finish.",
				project.service_id()
			);
		}

		thread::sleep(NORMALIZATION_WAIT_INTERVAL);
	}
}

fn record_normalization_failed(
	project: &ServiceConfig,
	state_store: &StateStore,
	run_id: &str,
	context: &BaselineGuardContext,
	error: &Report,
) -> Result<()> {
	record_event(
		project,
		state_store,
		run_id,
		BASELINE_NORMALIZATION_FAILED_EVENT_TYPE,
		payload_with_error(context.payload(), error),
	)
}

fn latest_baseline_event_for_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	context: &BaselineGuardContext,
) -> Result<Option<(String, Value)>> {
	Ok(state_store
		.list_private_execution_events_for_issue(project.service_id(), BASELINE_ISSUE_ID)?
		.into_iter()
		.rev()
		.find(|event| baseline_event_matches_context(event.payload(), context))
		.map(|event| (event.event_type().to_owned(), event.payload().clone())))
}

fn baseline_event_matches_context(payload: &Value, context: &BaselineGuardContext) -> bool {
	payload.get("main_oid").and_then(Value::as_str) == Some(context.main_oid.as_str())
		&& payload.get("workflow_hash").and_then(Value::as_str)
			== Some(context.workflow_hash.as_str())
}

fn record_event(
	project: &ServiceConfig,
	state_store: &StateStore,
	run_id: &str,
	event_type: &str,
	payload: Value,
) -> Result<()> {
	state_store.append_private_execution_event(
		project.service_id(),
		BASELINE_ISSUE_ID,
		run_id,
		BASELINE_ATTEMPT_NUMBER,
		event_type,
		payload,
	)?;

	Ok(())
}

fn payload_with_error(mut payload: Value, error: &Report) -> Value {
	if let Some(object) = payload.as_object_mut() {
		object.insert(String::from("error"), Value::String(error.to_string()));
	}

	payload
}

fn payload_with_fields<I>(mut payload: Value, fields: I) -> Value
where
	I: IntoIterator<Item = (&'static str, Value)>,
{
	if let Some(object) = payload.as_object_mut() {
		for (key, value) in fields {
			object.insert(String::from(key), value);
		}
	}

	payload
}

fn baseline_status_reason(event_type: &str, payload: &Value) -> String {
	let main_oid = payload.get("main_oid").and_then(Value::as_str).unwrap_or("unknown");
	let workflow_hash = payload.get("workflow_hash").and_then(Value::as_str).unwrap_or("unknown");
	let error = payload.get("error").and_then(Value::as_str);

	match error {
		Some(error) => format!(
			"latest baseline event `{event_type}` for main `{main_oid}` and workflow `{workflow_hash}`: {error}"
		),
		None => format!(
			"latest baseline event `{event_type}` for main `{main_oid}` and workflow `{workflow_hash}`"
		),
	}
}

fn workflow_hash(workflow: &WorkflowDocument) -> Result<String> {
	let markdown = workflow.to_markdown()?;
	let digest = Sha256::digest(markdown.as_bytes());

	Ok(hex_digest(&digest))
}

fn hex_digest(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn short_oid(oid: &str) -> &str {
	oid.get(..12).unwrap_or(oid)
}

fn git_ref_component(value: &str) -> String {
	value
		.chars()
		.map(|character| {
			if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
				character
			} else {
				'-'
			}
		})
		.collect()
}

fn path_arg(path: &Path) -> String {
	path.display().to_string()
}

fn output_text(output: &Output) -> String {
	git_ops::repo_gate_output_text(output)
}

#[cfg(test)]
mod tests {
	#[cfg(unix)] use std::os::unix::fs::PermissionsExt;
	use std::{fs, path::Path, process::Command};

	use tempfile::TempDir;

	use super::*;
	use crate::{config::ServiceConfig, state::StateStore, test_support::TestEnvVarGuard};

	#[test]
	fn clean_guard_records_clean_event_and_leaves_no_worktree() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let fixture = BaselineGuardFixture::new(
			r#"canonicalize_commands = ["true"]
verify_commands = []"#,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect("clean guard should pass");

		let events = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list");

		assert_eq!(events.len(), 1);
		assert_eq!(events[0].event_type(), BASELINE_GUARD_CLEAN_EVENT_TYPE);
		assert!(baseline_dir_is_absent_or_empty(fixture.config.worktree_root()));
		assert!(
			git_capture_plain(fixture.config.repo_root(), &["status", "--porcelain"]).is_empty()
		);
	}

	#[test]
	fn clean_guard_fast_forwards_repo_root_to_guarded_main_before_dispatch() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let fixture = BaselineGuardFixture::new(
			r#"canonicalize_commands = ["true"]
verify_commands = []"#,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		fs::write(fixture.config.repo_root().join("README.md"), "remote baseline\n")
			.expect("readme should write");
		run_git(fixture.config.repo_root(), &["add", "README.md"]);
		run_git(
			fixture.config.repo_root(),
			&[
				"commit",
				"-m",
				r#"{"schema":"decodex/commit/2","change":"Advance remote baseline","authority":"manual","impact":"compatible"}"#,
			],
		);
		run_git(fixture.config.repo_root(), &["push", "origin", "main"]);
		let remote_head = git_capture_plain(fixture.config.repo_root(), &["rev-parse", "HEAD"]);

		run_git(fixture.config.repo_root(), &["reset", "--hard", "HEAD~1"]);
		let stale_local_head =
			git_capture_plain(fixture.config.repo_root(), &["rev-parse", "HEAD"]);

		assert_ne!(stale_local_head, remote_head);

		let outcome = ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect("clean guard should sync repo root");
		let local_head = git_capture_plain(fixture.config.repo_root(), &["rev-parse", "HEAD"]);

		assert_eq!(outcome, BaselineGuardDispatchOutcome::Clean);
		assert_eq!(local_head, remote_head);
	}

	#[test]
	fn guard_failure_records_failed_event_without_dirty_normalization() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let fixture = BaselineGuardFixture::new(
			r#"canonicalize_commands = ["python3 -c \"from pathlib import Path; Path('README.md').write_text('rewritten\\n'); raise SystemExit(2)\""]
verify_commands = []"#,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		let error = ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect_err("failed canonicalize command should fail guard");

		assert!(error.to_string().contains("Repo canonicalize command"));

		let events = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list");

		assert_eq!(events.len(), 1);
		assert_eq!(events[0].event_type(), BASELINE_GUARD_FAILED_EVENT_TYPE);
		let loop_evidence = state_store
			.project_loop_evidence_snapshot("baseline-test")
			.expect("loop evidence should load");
		let mut warnings = Vec::new();
		let mut warning_details = Vec::new();

		push_baseline_status_projection(
			&fixture.config,
			&loop_evidence,
			&mut warnings,
			&mut warning_details,
		);

		assert_eq!(warnings, vec![String::from("baseline_guard_failed")]);
		assert_eq!(warning_details.len(), 1);
		assert!(
			git_capture_plain(fixture.config.repo_root(), &["status", "--porcelain"]).is_empty()
		);
	}

	#[test]
	fn normalization_lock_uses_live_pid_for_single_flight_and_removes_dead_pid() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let fixture = BaselineGuardFixture::new(
			r#"canonicalize_commands = ["true"]
verify_commands = []"#,
		);
		let context = BaselineGuardContext::new(&fixture.config, &fixture.workflow)
			.expect("context should load");
		let lock_path = fixture.config.worktree_root().join(LOCK_FILE_NAME);

		fs::write(
			&lock_path,
			serde_json::json!({
				"schema": LOCK_SCHEMA,
				"main_oid": context.main_oid.as_str(),
				"workflow_hash": context.workflow_hash.as_str(),
				"pid": std::process::id(),
			})
			.to_string(),
		)
		.expect("live lock should write");

		let live_lock =
			BaselineNormalizationLock::acquire(fixture.config.worktree_root(), &context)
				.expect("lock acquire should inspect live lock");

		assert!(live_lock.is_none());

		fs::write(
			&lock_path,
			serde_json::json!({
				"schema": LOCK_SCHEMA,
				"main_oid": "different-main",
				"workflow_hash": context.workflow_hash.as_str(),
				"pid": std::process::id(),
			})
			.to_string(),
		)
		.expect("mismatched live lock should write");

		let mismatched_lock =
			BaselineNormalizationLock::acquire(fixture.config.worktree_root(), &context)
				.expect("mismatched live lock should still single-flight");

		assert!(mismatched_lock.is_none());
		assert!(lock_path.exists());

		fs::write(
			&lock_path,
			serde_json::json!({
				"schema": LOCK_SCHEMA,
				"main_oid": context.main_oid.as_str(),
				"workflow_hash": context.workflow_hash.as_str(),
				"pid": 999_999_999_i64,
			})
			.to_string(),
		)
		.expect("dead lock should write");

		let dead_lock =
			BaselineNormalizationLock::acquire(fixture.config.worktree_root(), &context)
				.expect("dead lock should be replaced");

		assert!(dead_lock.is_some());
		drop(dead_lock);
		assert!(!lock_path.exists());
	}

	#[test]
	fn normalization_attempt_branch_is_bound_to_main_and_workflow_for_retry() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let fixture = BaselineGuardFixture::new(
			r#"canonicalize_commands = ["true"]
verify_commands = []"#,
		);
		let context = BaselineGuardContext::new(&fixture.config, &fixture.workflow)
			.expect("context should load");
		let first = BaselineNormalizationRun::new(&fixture.config, &context);

		std::thread::sleep(Duration::from_millis(1));

		let second = BaselineNormalizationRun::new(&fixture.config, &context);

		assert_eq!(first.branch_name, second.branch_name);
		assert_eq!(first.path, second.path);
	}

	#[test]
	fn closeout_dispatch_skips_baseline_guard() {
		let fixture = BaselineGuardFixture::new("canonicalize_commands = []\nverify_commands = []");
		let state_store = StateStore::open_in_memory().expect("state store should open");

		ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Closeout,
			false,
		)
		.expect("closeout should skip guard without credentials or origin checks");

		let events = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list");

		assert!(events.is_empty());
	}

	#[test]
	fn dirty_guard_runs_full_automatic_normalization_and_rechecks_merged_main() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let temp_dir = TempDir::new().expect("temp dir should create");
		let fake_gh_path = temp_dir.path().join("fake-gh");

		write_fake_gh(&fake_gh_path);

		let fixture = BaselineGuardFixture::new_with_github_command_path(
			r#"canonicalize_commands = ["python3 -c \"from pathlib import Path; Path('README.md').write_text('normalized\\n'); Path('cache.tmp').write_text('do not commit\\n')\""]
verify_commands = ["python3 -c \"from pathlib import Path; assert Path('README.md').read_text() == 'normalized\\n'\""]"#,
			&fake_gh_path,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect("normalization should complete and recheck");

		let origin_main =
			git_capture_plain(fixture.config.repo_root(), &["show", "origin/main:README.md"]);
		let local_main = git_capture_plain(fixture.config.repo_root(), &["show", "HEAD:README.md"]);
		let local_head = git_capture_plain(fixture.config.repo_root(), &["rev-parse", "HEAD"]);
		let remote_head =
			git_capture_plain(fixture.config.repo_root(), &["rev-parse", "origin/main"]);
		let origin_files = git_capture_plain(
			fixture.config.repo_root(),
			&["ls-tree", "-r", "--name-only", "origin/main"],
		);
		let merge_subject = git_capture_plain(
			fixture.config.repo_root(),
			&["log", "-1", "--format=%s", "origin/main"],
		);
		let events = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list")
			.into_iter()
			.map(|event| event.event_type().to_owned())
			.collect::<Vec<_>>();

		assert_eq!(origin_main, "normalized");
		assert_eq!(local_main, "normalized");
		assert_eq!(local_head, remote_head);
		assert!(!origin_files.lines().any(|path| path == "cache.tmp"));
		assert!(merge_subject.contains(r#""authority":"baseline""#));
		assert!(events.contains(&String::from(BASELINE_GUARD_DIRTY_EVENT_TYPE)));
		assert!(events.contains(&String::from(BASELINE_NORMALIZATION_REPO_GATE_PASSED_EVENT_TYPE)));
		assert!(events.contains(&String::from(BASELINE_NORMALIZATION_MERGED_EVENT_TYPE)));
		assert!(events.contains(&String::from(BASELINE_NORMALIZATION_RECHECK_PASSED_EVENT_TYPE)));
		assert!(
			git_capture_plain(
				fixture.config.repo_root(),
				&["branch", "--list", "xy/*-baseline-normalize-*"]
			)
			.is_empty()
		);
	}

	#[test]
	fn baseline_guard_does_not_widen_to_profile_canonicalize_commands() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let fixture = BaselineGuardFixture::new(
			r#"canonicalize_commands = []
verify_commands = []

[execution.gate_profiles.docs]
match_mode = "only"
paths = ["README.md"]
canonicalize_commands = ["python3 -c \"from pathlib import Path; Path('README.md').write_text('profile-normalized\\n')\""]
verify_commands = ["python3 -c \"from pathlib import Path; assert Path('README.md').read_text() == 'profile-normalized\\n'\""]"#,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		let outcome = ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect("baseline guard should not run profile-scoped canonicalize commands");

		let origin_main =
			git_capture_plain(fixture.config.repo_root(), &["show", "origin/main:README.md"]);
		let events = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list");

		assert_eq!(outcome, BaselineGuardDispatchOutcome::Clean);
		assert_eq!(origin_main, "baseline");
		assert!(events.is_empty());
	}

	#[test]
	fn workflow_hash_is_stable_for_gate_profile_order() {
		let first = WorkflowDocument::parse_markdown(&workflow_markdown(
			r#"canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make check"]

[execution.gate_profiles.b]
match_mode = "only"
paths = ["b/**"]
canonicalize_commands = ["cargo make fmt-b"]
verify_commands = ["cargo make check-b"]

[execution.gate_profiles.a]
match_mode = "only"
paths = ["a/**"]
canonicalize_commands = ["cargo make fmt-a"]
verify_commands = ["cargo make check-a"]"#,
		))
		.expect("first workflow should parse");
		let second = WorkflowDocument::parse_markdown(&workflow_markdown(
			r#"canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make check"]

[execution.gate_profiles.a]
match_mode = "only"
paths = ["a/**"]
canonicalize_commands = ["cargo make fmt-a"]
verify_commands = ["cargo make check-a"]

[execution.gate_profiles.b]
match_mode = "only"
paths = ["b/**"]
canonicalize_commands = ["cargo make fmt-b"]
verify_commands = ["cargo make check-b"]"#,
		))
		.expect("second workflow should parse");

		assert_eq!(
			workflow_hash(&first).expect("first hash"),
			workflow_hash(&second).expect("second hash")
		);
	}

	#[test]
	fn pr_created_event_reuses_existing_baseline_pr_on_retry() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let temp_dir = TempDir::new().expect("temp dir should create");
		let fake_gh_path = temp_dir.path().join("fake-gh");
		let create_marker = temp_dir.path().join("create-called");

		write_fake_gh_rejecting_create(&fake_gh_path, &create_marker);

		let fixture = BaselineGuardFixture::new_with_github_command_path(
			r#"canonicalize_commands = ["python3 -c \"from pathlib import Path; Path('README.md').write_text('normalized\\n')\""]
verify_commands = ["python3 -c \"from pathlib import Path; assert Path('README.md').read_text() == 'normalized\\n'\""]"#,
			&fake_gh_path,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let context = BaselineGuardContext::new(&fixture.config, &fixture.workflow)
			.expect("context should load");
		let run = BaselineNormalizationRun::new(&fixture.config, &context);
		let repo_gate = BaselineRepoGateCommands::from_workflow(&fixture.workflow);
		let head_oid = with_branch_worktree(
			fixture.config.repo_root(),
			&run.path,
			&run.branch_name,
			&context.main_oid,
			&context.git_env,
			|| {
				git_ops::run_canonicalize_commands(&repo_gate.canonicalize_commands, &run.path)?;
				commit_normalization_diff(&run.path, &context)?;
				let head_oid = git_capture(&run.path, &["rev-parse", "HEAD"], &context.git_env)?;
				git_checked(
					&run.path,
					&["push", "-u", "origin", &run.branch_name],
					"push seeded baseline normalization branch",
					&context.git_env,
				)?;

				Ok(head_oid)
			},
		)
		.expect("seed normalization branch should succeed");

		record_event(
			&fixture.config,
			&state_store,
			&run.run_id,
			BASELINE_NORMALIZATION_PR_CREATED_EVENT_TYPE,
			normalization_payload(
				&context,
				&run,
				&head_oid,
				Some("https://github.com/example/repo/pull/1"),
			),
		)
		.expect("pr-created event should record");

		ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect("retry should reuse existing pr and finish");

		assert!(!create_marker.exists());
	}

	#[test]
	fn prior_clean_event_does_not_skip_current_baseline_guard() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let temp_dir = TempDir::new().expect("temp dir should create");
		let fake_gh_path = temp_dir.path().join("fake-gh");

		write_fake_gh(&fake_gh_path);

		let fixture = BaselineGuardFixture::new_with_github_command_path(
			r#"canonicalize_commands = ["python3 -c \"from pathlib import Path; Path('README.md').write_text('normalized\\n')\""]
verify_commands = []"#,
			&fake_gh_path,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let context = BaselineGuardContext::new(&fixture.config, &fixture.workflow)
			.expect("context should load");

		record_event(
			&fixture.config,
			&state_store,
			&context.guard_run_id(),
			BASELINE_GUARD_CLEAN_EVENT_TYPE,
			context.payload(),
		)
		.expect("stale clean event should record");

		ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect("current dirty baseline should normalize despite stale clean event");

		let events = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list")
			.into_iter()
			.map(|event| event.event_type().to_owned())
			.collect::<Vec<_>>();

		assert!(events.contains(&String::from(BASELINE_GUARD_DIRTY_EVENT_TYPE)));
		assert!(events.contains(&String::from(BASELINE_NORMALIZATION_MERGED_EVENT_TYPE)));
	}

	#[test]
	fn prior_failed_event_does_not_block_automatic_retry_for_same_binding() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let temp_dir = TempDir::new().expect("temp dir should create");
		let fake_gh_path = temp_dir.path().join("fake-gh");

		write_fake_gh(&fake_gh_path);

		let fixture = BaselineGuardFixture::new_with_github_command_path(
			r#"canonicalize_commands = ["python3 -c \"from pathlib import Path; Path('README.md').write_text('normalized\\n')\""]
verify_commands = []"#,
			&fake_gh_path,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let context = BaselineGuardContext::new(&fixture.config, &fixture.workflow)
			.expect("context should load");

		record_event(
			&fixture.config,
			&state_store,
			&context.normalization_run_id(),
			BASELINE_NORMALIZATION_FAILED_EVENT_TYPE,
			payload_with_error(context.payload(), &eyre::eyre!("transient failure")),
		)
		.expect("failed event should record");

		ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect("prior failure should not block retry");

		let events = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list")
			.into_iter()
			.map(|event| event.event_type().to_owned())
			.collect::<Vec<_>>();

		assert!(events.contains(&String::from(BASELINE_NORMALIZATION_FAILED_EVENT_TYPE)));
		assert!(events.contains(&String::from(BASELINE_NORMALIZATION_MERGED_EVENT_TYPE)));
		assert!(events.contains(&String::from(BASELINE_NORMALIZATION_RECHECK_PASSED_EVENT_TYPE)));
	}

	#[test]
	fn malformed_lock_with_started_event_is_removed_and_retried() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let temp_dir = TempDir::new().expect("temp dir should create");
		let fake_gh_path = temp_dir.path().join("fake-gh");

		write_fake_gh(&fake_gh_path);

		let fixture = BaselineGuardFixture::new_with_github_command_path(
			r#"canonicalize_commands = ["python3 -c \"from pathlib import Path; Path('README.md').write_text('normalized\\n')\""]
verify_commands = []"#,
			&fake_gh_path,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let context = BaselineGuardContext::new(&fixture.config, &fixture.workflow)
			.expect("context should load");
		let lock_path = fixture.config.worktree_root().join(LOCK_FILE_NAME);

		fs::write(&lock_path, "").expect("malformed lock should write");
		record_event(
			&fixture.config,
			&state_store,
			&context.normalization_run_id(),
			BASELINE_NORMALIZATION_STARTED_EVENT_TYPE,
			context.payload(),
		)
		.expect("started event should record");

		ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect("malformed stale lock should not block retry");

		assert!(!lock_path.exists());
		let origin_main =
			git_capture_plain(fixture.config.repo_root(), &["show", "origin/main:README.md"]);

		assert_eq!(origin_main, "normalized");
	}

	#[test]
	fn normalization_canonicalize_failure_records_failed_event() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let temp_dir = TempDir::new().expect("temp dir should create");
		let fake_gh_path = temp_dir.path().join("fake-gh");

		write_fake_gh(&fake_gh_path);

		let fixture = BaselineGuardFixture::new_with_github_command_path(
			r#"canonicalize_commands = ["python3 -c \"from pathlib import Path; import os, sys; Path('README.md').write_text('normalized\\n'); sys.exit(2 if 'normalization' in os.getcwd() else 0)\""]
verify_commands = []"#,
			&fake_gh_path,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		let error = ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect_err("normalization canonicalize failure should stop dispatch");
		let events = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list")
			.into_iter()
			.map(|event| event.event_type().to_owned())
			.collect::<Vec<_>>();

		assert!(error.to_string().contains("Repo canonicalize command"));
		assert!(events.contains(&String::from(BASELINE_NORMALIZATION_STARTED_EVENT_TYPE)));
		assert!(events.contains(&String::from(BASELINE_NORMALIZATION_FAILED_EVENT_TYPE)));
	}

	#[test]
	fn failed_normalization_gate_can_retry_same_binding_without_pr_side_effect() {
		let _env = TestEnvVarGuard::set("BASELINE_GUARD_TEST_GITHUB_TOKEN", "token");
		let temp_dir = TempDir::new().expect("temp dir should create");
		let fake_gh_path = temp_dir.path().join("fake-gh");

		write_fake_gh(&fake_gh_path);

		let fixture = BaselineGuardFixture::new_with_github_command_path(
			r#"canonicalize_commands = ["python3 -c \"from pathlib import Path; Path('README.md').write_text('normalized\\n')\""]
verify_commands = ["python3 -c \"raise SystemExit(3)\""]"#,
			&fake_gh_path,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		let first_error = ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect_err("failing normalization gate should stop dispatch");

		assert!(first_error.to_string().contains("Repo verify command"));

		let events_after_first = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list");

		assert!(
			!events_after_first
				.iter()
				.any(|event| event.event_type() == BASELINE_NORMALIZATION_PR_CREATED_EVENT_TYPE)
		);
		assert!(
			events_after_first
				.iter()
				.any(|event| event.event_type()
					== BASELINE_NORMALIZATION_REPO_GATE_FAILED_EVENT_TYPE)
		);

		let second_error = ensure_clean_baseline_before_dispatch(
			&fixture.config,
			&fixture.workflow,
			&state_store,
			IssueDispatchMode::Normal,
			false,
		)
		.expect_err("same failed binding should retry and fail the gate again");
		let events_after_second = state_store
			.list_private_execution_events_for_issue("baseline-test", BASELINE_ISSUE_ID)
			.expect("events should list");

		assert!(second_error.to_string().contains("Repo verify command"));
		assert!(events_after_second.len() > events_after_first.len());
		assert!(
			!events_after_second
				.iter()
				.any(|event| event.event_type() == BASELINE_NORMALIZATION_PR_CREATED_EVENT_TYPE)
		);
	}

	struct BaselineGuardFixture {
		_temp_dir: TempDir,
		config: ServiceConfig,
		workflow: WorkflowDocument,
	}
	impl BaselineGuardFixture {
		fn new(execution_command_lines: &str) -> Self {
			Self::new_with_optional_github_command_path(execution_command_lines, None)
		}

		fn new_with_github_command_path(
			execution_command_lines: &str,
			github_command_path: &Path,
		) -> Self {
			Self::new_with_optional_github_command_path(
				execution_command_lines,
				Some(github_command_path),
			)
		}

		fn new_with_optional_github_command_path(
			execution_command_lines: &str,
			github_command_path: Option<&Path>,
		) -> Self {
			let temp_dir = TempDir::new().expect("temp dir should create");
			let repo_root = temp_dir.path().join("repo");
			let remote_root = temp_dir.path().join("origin.git");
			let config_dir = temp_dir.path().join("config");

			fs::create_dir_all(&repo_root).expect("repo should create");
			fs::create_dir_all(repo_root.join(".worktrees")).expect("worktrees should create");
			fs::create_dir_all(&config_dir).expect("config dir should create");
			fs::write(repo_root.join("README.md"), "baseline\n").expect("readme should write");

			run_git(&repo_root, &["init", "-b", "main"]);
			run_git(&repo_root, &["config", "user.name", "Decodex Tests"]);
			run_git(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
			run_git(&repo_root, &["config", "commit.gpgsign", "false"]);
			run_git(&repo_root, &["config", "core.hooksPath", "/dev/null"]);
			run_git(&repo_root, &["add", "."]);
			run_git(
				&repo_root,
				&[
					"commit",
					"-m",
					r#"{"schema":"decodex/commit/2","change":"Bootstrap test repo","authority":"manual","impact":"compatible"}"#,
				],
			);
			run_git(
				temp_dir.path(),
				&["init", "--bare", "-b", "main", path_arg(&remote_root).as_str()],
			);
			run_git(&repo_root, &["remote", "add", "origin", path_arg(&remote_root).as_str()]);
			run_git(&repo_root, &["push", "-u", "origin", "main"]);

			fs::write(config_dir.join("WORKFLOW.md"), workflow_markdown(execution_command_lines))
				.expect("workflow should write");
			let github_command_path_line = github_command_path
				.map(|path| format!("command_path = \"{}\"\n", path.display()))
				.unwrap_or_default();
			fs::write(
				config_dir.join("project.toml"),
				format!(
					r#"service_id = "baseline-test"

[tracker]
api_key_env_var = "BASELINE_GUARD_TEST_LINEAR_TOKEN"

[github]
token_env_var = "BASELINE_GUARD_TEST_GITHUB_TOKEN"
{github_command_path_line}

[paths]
repo_root = "{}"
worktree_root = ".worktrees"
"#,
					repo_root.display()
				),
			)
			.expect("config should write");

			let config = ServiceConfig::from_path(config_dir.join("project.toml"))
				.expect("config should load");
			let workflow =
				WorkflowDocument::from_path(config.workflow_path()).expect("workflow should load");

			Self { _temp_dir: temp_dir, config, workflow }
		}
	}

	fn write_fake_gh(path: &Path) {
		fs::write(
			path,
			r#"#!/bin/sh
set -eu

if [ "${1:-}" = "pr" ] && [ "${2:-}" = "create" ]; then
  echo "https://github.com/example/repo/pull/1"
  exit 0
fi

if [ "${1:-}" = "pr" ] && [ "${2:-}" = "merge" ]; then
  head=""
  subject='{"schema":"decodex/commit/2","change":"Merge baseline normalization","authority":"baseline","impact":"compatible"}'
  previous=""
  for argument in "$@"; do
    if [ "$previous" = "--match-head-commit" ]; then
      head="$argument"
    fi
    if [ "$previous" = "--subject" ]; then
      subject="$argument"
    fi
    previous="$argument"
  done
  remote="$(git remote get-url origin)"
  tmp="$(mktemp -d)"
  git clone "$remote" "$tmp" >/dev/null 2>&1
  git -C "$tmp" config user.name "Decodex Tests"
  git -C "$tmp" config user.email "decodex-tests@example.com"
  git -C "$tmp" config core.hooksPath /dev/null
  git -C "$tmp" fetch origin "$head"
  git -C "$tmp" merge --no-ff "$head" -m "$subject"
  git -C "$tmp" push origin HEAD:main
  rm -rf "$tmp"
  exit 0
fi

if [ "${1:-}" = "pr" ] && [ "${2:-}" = "view" ]; then
  oid="$(git ls-remote origin refs/heads/main | cut -f1)"
  printf '{"state":"MERGED","headRefOid":null,"mergeCommit":{"oid":"%s"}}\n' "$oid"
  exit 0
fi

echo "unsupported fake gh invocation: $*" >&2
exit 1
"#,
		)
		.expect("fake gh should write");

		#[cfg(unix)]
		{
			let mut permissions = fs::metadata(path).expect("fake gh metadata").permissions();
			permissions.set_mode(0o755);
			fs::set_permissions(path, permissions).expect("fake gh executable");
		}
	}

	fn write_fake_gh_rejecting_create(path: &Path, create_marker: &Path) {
		fs::write(
			path,
			format!(
				r#"#!/bin/sh
set -eu

if [ "${{1:-}}" = "pr" ] && [ "${{2:-}}" = "create" ]; then
  touch "{}"
  echo "pr create should not be called during resume" >&2
  exit 7
fi

if [ "${{1:-}}" = "pr" ] && [ "${{2:-}}" = "merge" ]; then
  head=""
  subject='{{"schema":"decodex/commit/2","change":"Merge baseline normalization","authority":"baseline","impact":"compatible"}}'
  previous=""
  for argument in "$@"; do
    if [ "$previous" = "--match-head-commit" ]; then
      head="$argument"
    fi
    if [ "$previous" = "--subject" ]; then
      subject="$argument"
    fi
    previous="$argument"
  done
  remote="$(git remote get-url origin)"
  tmp="$(mktemp -d)"
  git clone "$remote" "$tmp" >/dev/null 2>&1
  git -C "$tmp" config user.name "Decodex Tests"
  git -C "$tmp" config user.email "decodex-tests@example.com"
  git -C "$tmp" config core.hooksPath /dev/null
  git -C "$tmp" fetch origin "$head"
  git -C "$tmp" merge --no-ff "$head" -m "$subject"
  git -C "$tmp" push origin HEAD:main
  rm -rf "$tmp"
  exit 0
fi

if [ "${{1:-}}" = "pr" ] && [ "${{2:-}}" = "view" ]; then
  oid="$(git ls-remote origin refs/heads/main | cut -f1)"
  printf '{{"state":"MERGED","headRefOid":null,"mergeCommit":{{"oid":"%s"}}}}\n' "$oid"
  exit 0
fi

echo "unsupported fake gh invocation: $*" >&2
exit 1
"#,
				create_marker.display()
			),
		)
		.expect("fake gh should write");

		#[cfg(unix)]
		{
			let mut permissions = fs::metadata(path).expect("fake gh metadata").permissions();
			permissions.set_mode(0o755);
			fs::set_permissions(path, permissions).expect("fake gh executable");
		}
	}

	fn workflow_markdown(execution_command_lines: &str) -> String {
		let gate_profiles_line = if execution_command_lines.contains("gate_profiles")
			|| execution_command_lines.contains("[execution.gate_profiles")
		{
			""
		} else {
			"gate_profiles = {}\n"
		};

		format!(
			r#"+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done"]
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
{gate_profiles_line}
{execution_command_lines}

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Follow the repository policy.
"#
		)
	}

	fn run_git(cwd: &Path, args: &[&str]) {
		let output =
			Command::new("git").arg("-C").arg(cwd).args(args).output().expect("git should spawn");

		assert!(
			output.status.success(),
			"git {} failed: {}",
			args.join(" "),
			String::from_utf8_lossy(&output.stderr)
		);
	}

	fn git_capture_plain(cwd: &Path, args: &[&str]) -> String {
		let output =
			Command::new("git").arg("-C").arg(cwd).args(args).output().expect("git should spawn");

		assert!(
			output.status.success(),
			"git {} failed: {}",
			args.join(" "),
			String::from_utf8_lossy(&output.stderr)
		);

		String::from_utf8_lossy(&output.stdout).trim().to_owned()
	}

	fn baseline_dir_is_absent_or_empty(worktree_root: &Path) -> bool {
		let baseline_dir = worktree_root.join(".baseline");

		!baseline_dir.exists()
			|| fs::read_dir(baseline_dir).expect("baseline dir should read").next().is_none()
	}
}
