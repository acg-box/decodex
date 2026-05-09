use std::{
	env, fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process::{Command, Stdio},
	time::Duration,
};

use color_eyre::{Report, eyre::WrapErr};

use crate::{
	commit_message::{self, MANUAL_AUTHORITY},
	config::{self, ServiceConfig},
	default_branch_sync,
	git_credentials::GitCredentialSource,
	github::{self, RepositoryContext},
	orchestrator,
	prelude::{Result, eyre},
	pull_request::{self, PullRequestLandingState},
	runtime,
	state::{RUN_ACTIVITY_MARKER_FILE, ReviewHandoffMarker, StateStore},
	tracker::{self, IssueTracker, TrackerIssue, linear::LinearClient},
	workflow::WorkflowDocument,
	worktree::{self, WorktreeManager},
};

const MANUAL_LAND_CLOSEOUT_MARKER_GIT_PATH: &str = "decodex/manual-land-closeout";
const MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug)]
pub(crate) struct ManualCommitRequest {
	pub(crate) summary: String,
	pub(crate) authority: Option<String>,
	pub(crate) manual_authority: bool,
	pub(crate) related: Vec<String>,
	pub(crate) breaking: bool,
}

#[derive(Debug)]
pub(crate) struct ManualLandRequest {
	pub(crate) summary: String,
	pub(crate) authority: Option<String>,
	pub(crate) manual_authority: bool,
	pub(crate) pr_url: Option<String>,
	pub(crate) related: Vec<String>,
	pub(crate) breaking: bool,
}

struct PreparedCloseout {
	tracker: LinearClient,
	issue: TrackerIssue,
	completed_state: String,
	service_id: String,
	needs_attention_label: String,
}

struct ManualLandContext {
	cwd: PathBuf,
	current_branch: String,
	worktree_root: PathBuf,
	project_worktree_root: PathBuf,
	canonical_repo_root: PathBuf,
	authority: ManualAuthority,
	service_id: String,
	workflow: WorkflowDocument,
	github_token_env_var: String,
	github_token: String,
	repository: RepositoryContext,
	prepared_closeout: Option<PreparedCloseout>,
	pr_url: String,
	review_branch: String,
}
impl ManualLandContext {
	fn default_branch_git_credentials(&self) -> GitCredentialSource<'_> {
		GitCredentialSource::new(
			&self.github_token_env_var,
			&self.github_token,
			&self.worktree_root,
		)
	}
}

struct ManualLandRecoveryOutcome {
	merge_commit: String,
}

#[derive(Default)]
struct ManualLandCloseoutMarkerRecord {
	pr_url: Option<String>,
	merge_commit: Option<String>,
	branch_name: Option<String>,
	landed_change: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LandExecutionMode {
	MergeAndCloseout,
	CloseoutOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ManualAuthority {
	Issue(String),
	Manual,
}
impl ManualAuthority {
	fn commit_message_value(&self) -> &str {
		match self {
			Self::Issue(identifier) => identifier.as_str(),
			Self::Manual => MANUAL_AUTHORITY,
		}
	}

	fn issue_identifier(&self) -> Option<&str> {
		match self {
			Self::Issue(identifier) => Some(identifier.as_str()),
			Self::Manual => None,
		}
	}

	fn is_manual(&self) -> bool {
		matches!(self, Self::Manual)
	}
}

pub(crate) fn run_commit(config_path: Option<&Path>, request: &ManualCommitRequest) -> Result<()> {
	let cwd = env::current_dir()?;
	let worktree_root = current_worktree_root(&cwd)?;
	let authority = resolve_authority(
		config_path,
		request.authority.as_deref(),
		request.manual_authority,
		&worktree_root,
	)?;
	let message = commit_message::build_commit_message(
		&request.summary,
		authority.commit_message_value(),
		&request.related,
		request.breaking,
	)?;

	run_git_checked_with_stdio(&cwd, &["commit", "-S", "-m", message.as_str()])
}

pub(crate) fn run_land(config_path: Option<&Path>, request: &ManualLandRequest) -> Result<()> {
	let context = prepare_manual_land_context(config_path, request)?;

	if !github::pull_request_matches_repository(&context.pr_url, &context.repository)? {
		eyre::bail!(
			"Pull request `{}` does not belong to the current repository `{}/{}`.",
			context.pr_url,
			context.repository.owner,
			context.repository.name,
		);
	}

	if let Some(recovery) = finalize_already_merged_manual_land_recovery(&context, request)? {
		println!(
			"land ok: pr={} merge_commit={} default_branch={} local_default_branch_synced=true",
			context.pr_url, recovery.merge_commit, context.repository.default_branch
		);

		return Ok(());
	}

	ensure_manual_land_checkout_is_managed_lane(
		&context.worktree_root,
		&context.project_worktree_root,
		manual_land_cleanup_identifier(&context.authority, &context.current_branch),
	)?;

	if context.current_branch == context.repository.default_branch {
		eyre::bail!("`decodex land` must run from a reviewed lane branch, not the default branch.");
	}
	if context.review_branch != context.current_branch {
		eyre::bail!(
			"Review handoff expects branch `{}`, but the current branch is `{}`.",
			context.review_branch,
			context.current_branch,
		);
	}

	let default_branch = context.repository.default_branch.clone();
	let landing_state = github::inspect_pull_request_landing_state(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
	)?;
	let current_head = current_head_oid(&context.cwd)?;
	let execution_mode = validate_landing_state(
		&landing_state,
		&context.pr_url,
		&default_branch,
		&context.current_branch,
		&current_head,
	)?;

	default_branch_sync::preflight_repo_root_default_branch_sync(
		&context.canonical_repo_root,
		&default_branch,
		Some(context.default_branch_git_credentials()),
	)?;

	let landed_change_record = commit_message::build_landing_commit_message(
		&request.summary,
		context.authority.commit_message_value(),
		&request.related,
		request.breaking,
	)?;
	let merge_commit =
		execute_land_merge(&context, &current_head, landed_change_record.as_str(), execution_mode)?;
	let landed_change_record = load_authoritative_landed_change_record(&context, &merge_commit)?;

	finalize_land_closeout(
		&context,
		&merge_commit,
		&default_branch,
		landed_change_record.as_str(),
	)?;

	println!(
		"land ok: pr={} merge_commit={} default_branch={} local_default_branch_synced=true",
		context.pr_url, merge_commit, default_branch
	);

	Ok(())
}

fn prepare_manual_land_context(
	config_path: Option<&Path>,
	request: &ManualLandRequest,
) -> Result<ManualLandContext> {
	let cwd = env::current_dir()?;
	let worktree_root = current_worktree_root(&cwd)?;
	let current_branch = current_branch_name(&cwd)?;
	let resolved_config_path = resolve_manual_config_path(config_path, &cwd)?;
	let config = ServiceConfig::from_path(&resolved_config_path)?;
	let canonical_repo_root = config::canonical_repo_root_for_checkout(&cwd)?
		.unwrap_or_else(|| config.repo_root().to_path_buf());

	ensure_cli_repo_context(&cwd, &config, &canonical_repo_root)?;

	let authority = resolve_land_authority(
		Some(&resolved_config_path),
		request.authority.as_deref(),
		request.manual_authority,
		&worktree_root,
	)?;
	let github_token = config.github().resolve_token()?;
	let repository = github::inspect_repository_context(&canonical_repo_root, &github_token)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let prepared_closeout =
		prepare_manual_land_closeout(&config, &canonical_repo_root, workflow.clone(), &authority)?;
	let state_store = runtime::open_runtime_store()?;

	runtime::register_project_config(&state_store, &resolved_config_path, true)?;

	let handoff = match prepared_closeout.as_ref() {
		Some(prepared_closeout) => read_manual_land_handoff(
			&state_store,
			config.service_id(),
			&prepared_closeout.issue.id,
			&current_branch,
		)?,
		None => None,
	};
	let pr_url =
		resolve_pr_url(request.pr_url.as_deref(), handoff.as_ref(), authority.is_manual())?;
	let review_branch = handoff
		.as_ref()
		.map(|marker| marker.branch_name().to_owned())
		.unwrap_or_else(|| current_branch.clone());

	Ok(ManualLandContext {
		cwd,
		current_branch,
		worktree_root,
		project_worktree_root: config.worktree_root().to_path_buf(),
		canonical_repo_root,
		authority,
		service_id: config.service_id().to_owned(),
		workflow,
		github_token_env_var: config.github().token_env_var().to_owned(),
		github_token,
		repository,
		prepared_closeout,
		pr_url,
		review_branch,
	})
}

fn prepare_manual_land_closeout(
	config: &ServiceConfig,
	_canonical_repo_root: &Path,
	workflow: WorkflowDocument,
	authority: &ManualAuthority,
) -> Result<Option<PreparedCloseout>> {
	let Some(authority_issue) = authority.issue_identifier() else {
		return Ok(None);
	};

	prepare_closeout(config, workflow, authority_issue).map(Some)
}

fn execute_land_merge(
	context: &ManualLandContext,
	current_head: &str,
	landed_change_record: &str,
	execution_mode: LandExecutionMode,
) -> Result<String> {
	match execution_mode {
		LandExecutionMode::MergeAndCloseout => {
			ensure_clean_worktree(&context.cwd)?;

			if !context.repository.merge_commit_allowed {
				eyre::bail!(
					"GitHub repository `{}/{}` does not allow merge commits, but `decodex land` requires an admin merge commit.",
					context.repository.owner,
					context.repository.name
				);
			}

			if let Err(error) = github::admin_merge_pull_request(
				&context.canonical_repo_root,
				&context.pr_url,
				current_head,
				Some(landed_change_record),
				&context.github_token,
			) {
				if matches!(
					github::pull_request_is_merged_at_head(
						&context.canonical_repo_root,
						&context.pr_url,
						current_head,
						&context.github_token,
					),
					Ok(true)
				) {
					return github::wait_for_pull_request_merge_commit(
						&context.canonical_repo_root,
						&context.pr_url,
						&context.github_token,
						MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
					);
				}

				return Err(error);
			}
		},
		LandExecutionMode::CloseoutOnly => {},
	}

	github::wait_for_pull_request_merge_commit(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
	)
}

fn load_authoritative_landed_change_record(
	context: &ManualLandContext,
	merge_commit: &str,
) -> Result<String> {
	github::wait_for_commit_subject(
		&context.canonical_repo_root,
		&context.pr_url,
		merge_commit,
		&context.github_token,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
	)
}

fn finalize_land_closeout(
	context: &ManualLandContext,
	merge_commit: &str,
	default_branch: &str,
	landed_change_record: &str,
) -> Result<()> {
	if let Some(prepared_closeout) = context.prepared_closeout.as_ref() {
		apply_closeout(
			&context.cwd,
			prepared_closeout,
			&context.pr_url,
			merge_commit,
			&context.current_branch,
			landed_change_record,
		)?;
	}

	default_branch_sync::sync_repo_root_default_branch(
		&context.canonical_repo_root,
		default_branch,
		Some(context.default_branch_git_credentials()),
	)?;

	if context.prepared_closeout.is_none()
		&& !manual_land_closeout_matches(
			&context.cwd,
			&context.pr_url,
			merge_commit,
			&context.current_branch,
			landed_change_record,
		)? {
		write_manual_land_closeout_marker(
			&context.cwd,
			&context.pr_url,
			merge_commit,
			&context.current_branch,
			landed_change_record,
		)?;
	}

	cleanup_manual_land_lane_checkout(context)?;

	if let Some(prepared_closeout) = context.prepared_closeout.as_ref() {
		let state_store = runtime::open_runtime_store()?;

		clear_manual_closeout_runtime_state(&state_store, &prepared_closeout.issue.id)?;
		clear_manual_closeout_issue_scope(
			&prepared_closeout.tracker,
			&prepared_closeout.issue,
			&prepared_closeout.service_id,
			&prepared_closeout.needs_attention_label,
		)?;
	}

	Ok(())
}

fn cleanup_manual_land_lane_checkout(context: &ManualLandContext) -> Result<()> {
	let worktree_manager = WorktreeManager::new(
		context.service_id.as_str(),
		&context.canonical_repo_root,
		&context.project_worktree_root,
	);

	github::delete_pull_request_head_branch_if_present(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.current_branch,
		&context.github_token,
	)?;
	orchestrator::detach_worktree_head_from_branch_if_checked_out(
		&context.worktree_root,
		&context.current_branch,
	)?;
	orchestrator::delete_local_branch_if_present(
		&context.canonical_repo_root,
		&context.current_branch,
	)?;

	worktree_manager.remove_worktree_path_with_hooks(
		manual_land_cleanup_identifier(&context.authority, &context.current_branch),
		&context.current_branch,
		&context.worktree_root,
		context.workflow.frontmatter().execution().workspace_hooks(),
	)?;

	ensure_manual_land_left_no_merged_worktree_cleanup_debt(context)?;

	Ok(())
}

fn ensure_manual_land_left_no_merged_worktree_cleanup_debt(
	context: &ManualLandContext,
) -> Result<()> {
	let debts = worktree::merged_worktree_cleanup_debts(
		&context.canonical_repo_root,
		&context.project_worktree_root,
		&context.repository.default_branch,
	)?;

	if debts.is_empty() {
		return Ok(());
	}

	let details = debts
		.iter()
		.map(|debt| {
			format!(
				"{} on {} ({})",
				debt.path.display(),
				debt.branch_name,
				if debt.cleanliness.is_dirty() { "dirty" } else { "clean" }
			)
		})
		.collect::<Vec<_>>()
		.join(", ");

	eyre::bail!(
		"`decodex land` completed the merge but post-land worktree cleanup debt remains under `{}`: {details}. Remove or salvage those worktrees before continuing automation.",
		context.project_worktree_root.display()
	);
}

fn manual_land_cleanup_identifier<'a>(
	authority: &'a ManualAuthority,
	current_branch: &'a str,
) -> &'a str {
	authority.issue_identifier().unwrap_or(current_branch)
}

fn resolve_manual_config_path(explicit: Option<&Path>, cwd: &Path) -> Result<PathBuf> {
	if let Some(explicit) = explicit {
		return Ok(explicit.to_path_buf());
	}

	let state_store = runtime::open_runtime_store()?;

	if let Some(registered) = runtime::registered_config_path_for_cwd(&state_store, cwd)? {
		return Ok(registered);
	}

	eyre::bail!(
		"Decodex project config is required for this command. Pass `--config <PROJECT_DIR>` or register one with `decodex project add <PROJECT_DIR>`."
	);
}

fn resolve_authority(
	config_path: Option<&Path>,
	explicit: Option<&str>,
	manual_authority: bool,
	worktree_root: &Path,
) -> Result<ManualAuthority> {
	if manual_authority {
		return Ok(ManualAuthority::Manual);
	}

	if let Some(explicit) = explicit {
		return Ok(ManualAuthority::Issue(commit_message::normalize_issue_identifier(
			"authority",
			explicit,
		)?));
	}
	if let Some(inferred) = infer_issue_identifier_from_worktree_root(worktree_root) {
		return Ok(ManualAuthority::Issue(inferred));
	}

	if config_path.is_some() {
		eyre::bail!(
			"Failed to infer the issue authority from worktree `{}`. Pass `--authority <ISSUE>` or `--manual-authority`.",
			worktree_root.display()
		);
	}

	eyre::bail!(
		"`--authority <ISSUE>` or `--manual-authority` is required outside an issue worktree."
	)
}

fn resolve_land_authority(
	config_path: Option<&Path>,
	explicit: Option<&str>,
	manual_authority: bool,
	worktree_root: &Path,
) -> Result<ManualAuthority> {
	if manual_authority {
		return Ok(ManualAuthority::Manual);
	}

	let inferred = infer_issue_identifier_from_worktree_root(worktree_root);

	if let Some(explicit) = explicit {
		let explicit = commit_message::normalize_issue_identifier("authority", explicit)?;

		if let Some(inferred) = inferred {
			if !explicit.eq_ignore_ascii_case(&inferred) {
				eyre::bail!(
					"`decodex land` authority `{explicit}` does not match the current lane issue `{inferred}`."
				);
			}

			return Ok(ManualAuthority::Issue(inferred));
		}

		return Ok(ManualAuthority::Issue(explicit));
	}
	if let Some(inferred) = inferred {
		return Ok(ManualAuthority::Issue(inferred));
	}

	if config_path.is_some() {
		eyre::bail!(
			"Failed to infer the lane issue from worktree `{}`. Pass `--authority <ISSUE>` or `--manual-authority`.",
			worktree_root.display()
		);
	}

	eyre::bail!(
		"`--authority <ISSUE>` or `--manual-authority` is required outside an issue worktree."
	)
}

fn ensure_cli_repo_context(
	cwd: &Path,
	config: &ServiceConfig,
	canonical_repo_root: &Path,
) -> Result<()> {
	let worktree_root = current_worktree_root(cwd)?;

	if worktree_root == canonical_repo_root
		|| config::checkouts_share_repository(&worktree_root, canonical_repo_root)?
	{
		let config_repo_root = config.repo_root();

		if config_repo_root == canonical_repo_root
			|| config::checkouts_share_repository(config_repo_root, canonical_repo_root)?
		{
			return Ok(());
		}
	}

	eyre::bail!(
		"Current worktree `{}` does not match loaded config repo root `{}` for canonical repo root `{}`.",
		worktree_root.display(),
		config.repo_root().display(),
		canonical_repo_root.display(),
	);
}

fn current_worktree_root(cwd: &Path) -> Result<PathBuf> {
	let root = run_git_capture(cwd, &["rev-parse", "--show-toplevel"])?;

	Ok(PathBuf::from(root))
}

fn current_branch_name(cwd: &Path) -> Result<String> {
	let branch = run_git_capture(cwd, &["branch", "--show-current"])?;

	if branch.is_empty() {
		eyre::bail!("Current Git checkout is detached; switch back to a lane branch first.");
	}

	Ok(branch)
}

fn current_head_oid(cwd: &Path) -> Result<String> {
	run_git_capture(cwd, &["rev-parse", "HEAD"])
}

fn resolve_pr_url(
	explicit: Option<&str>,
	handoff: Option<&ReviewHandoffMarker>,
	manual_authority: bool,
) -> Result<String> {
	if let Some(explicit) = explicit {
		return Ok(explicit.trim().to_owned());
	}
	if let Some(handoff) = handoff {
		return Ok(handoff.pr_url().to_owned());
	}

	if manual_authority {
		eyre::bail!("`decodex land --manual-authority` requires `--pr <URL>`.");
	}

	eyre::bail!(
		"`decodex land` requires a PR URL. Run it from a handoff worktree or pass `--pr <URL>`."
	);
}

fn read_manual_land_handoff(
	state_store: &StateStore,
	service_id: &str,
	issue_id: &str,
	current_branch: &str,
) -> Result<Option<ReviewHandoffMarker>> {
	state_store.review_handoff_marker(service_id, issue_id, current_branch)
}

fn infer_issue_identifier_from_worktree_root(worktree_root: &Path) -> Option<String> {
	let basename = worktree_root.file_name()?.to_str()?;

	looks_like_issue_identifier(basename).then(|| basename.to_owned())
}

fn looks_like_issue_identifier(value: &str) -> bool {
	commit_message::looks_like_issue_identifier(value)
}

fn ensure_clean_worktree(cwd: &Path) -> Result<()> {
	let status = run_git_capture(cwd, &["status", "--porcelain"])?;

	if status.lines().any(is_landing_blocking_status_line) {
		eyre::bail!("Worktree has uncommitted changes. Commit or stash them before landing.");
	}

	Ok(())
}

fn is_landing_blocking_status_line(line: &str) -> bool {
	let line = line.trim_end();

	!line.is_empty() && !is_untracked_decodex_runtime_marker_status_line(line)
}

fn is_untracked_decodex_runtime_marker_status_line(line: &str) -> bool {
	let Some(path) = line.strip_prefix("?? ") else {
		return false;
	};

	path == RUN_ACTIVITY_MARKER_FILE
}

fn validate_landing_state(
	landing_state: &PullRequestLandingState,
	pr_url: &str,
	expected_base_branch: &str,
	current_branch: &str,
	current_head: &str,
) -> Result<LandExecutionMode> {
	let gate_view = landing_state.gate_view();

	if landing_state.base_ref_name != expected_base_branch {
		eyre::bail!(
			"Pull request `{pr_url}` targets base branch `{}`, but `decodex land` only lands into `{expected_base_branch}`.",
			landing_state.base_ref_name
		);
	}
	if landing_state.head_ref_name != current_branch {
		eyre::bail!(
			"Pull request `{pr_url}` points at branch `{}`, but the current branch is `{current_branch}`.",
			landing_state.head_ref_name
		);
	}
	if landing_state.head_ref_oid != current_head {
		eyre::bail!(
			"Pull request `{pr_url}` points at head `{}`, but the current branch head is `{current_head}`.",
			landing_state.head_ref_oid
		);
	}
	if gate_view.state == "MERGED" {
		return Ok(LandExecutionMode::CloseoutOnly);
	}
	if gate_view.state != "OPEN" {
		eyre::bail!("Pull request `{pr_url}` is `{}` and cannot be landed.", gate_view.state);
	}
	if gate_view.is_draft {
		eyre::bail!("Pull request `{pr_url}` is still draft.");
	}
	if gate_view.pending_review_requests > 0 {
		eyre::bail!(
			"Pull request `{pr_url}` still has {} pending review request(s).",
			gate_view.pending_review_requests
		);
	}
	if gate_view.unresolved_review_threads > 0 {
		eyre::bail!(
			"Pull request `{pr_url}` still has {} unresolved review thread(s).",
			gate_view.unresolved_review_threads
		);
	}
	if gate_view.review_decision == Some("CHANGES_REQUESTED") {
		eyre::bail!("Pull request `{pr_url}` still has active change requests.");
	}

	if let Some(reason) = pull_request::merge_state_requires_review_repair(
		gate_view.mergeable,
		gate_view.merge_state_status,
	) {
		eyre::bail!("Pull request `{pr_url}` requires review repair: {reason}.");
	}

	if pull_request::failed_checks_require_repair(
		gate_view.status_check_rollup_state,
		gate_view.merge_state_status,
	) {
		eyre::bail!("Pull request `{pr_url}` has failed required checks that need repair.");
	}
	if !pull_request::merge_state_allows_ready_to_land(gate_view.merge_state_status) {
		eyre::bail!(
			"Pull request `{pr_url}` is not ready to land: mergeStateStatus=`{}`.",
			gate_view.merge_state_status
		);
	}
	if gate_view.mergeable != "MERGEABLE" {
		eyre::bail!(
			"Pull request `{pr_url}` is not mergeable: mergeable=`{}`.",
			gate_view.mergeable
		);
	}

	match gate_view.status_check_rollup_state {
		Some(other) if pull_request::checks_require_wait(Some(other)) => eyre::bail!(
			"Pull request `{pr_url}` is still waiting on checks: statusCheckRollup=`{other}`."
		),
		Some("SUCCESS") | None => {
			debug_assert!(pull_request::manual_landing_gates_satisfied(gate_view));

			Ok(LandExecutionMode::MergeAndCloseout)
		},
		Some(other) => eyre::bail!(
			"Pull request `{pr_url}` still has non-green checks: statusCheckRollup=`{other}`."
		),
	}
}

fn finalize_already_merged_manual_land_recovery(
	context: &ManualLandContext,
	request: &ManualLandRequest,
) -> Result<Option<ManualLandRecoveryOutcome>> {
	if !request.manual_authority || request.pr_url.is_none() {
		return Ok(None);
	}
	if !current_checkout_is_repo_root_default_branch(context)? {
		return Ok(None);
	}

	let landing_state = github::inspect_pull_request_landing_state(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
	)?;

	if landing_state.state != "MERGED" {
		eyre::bail!(
			"`decodex land --manual-authority --pr` can recover from the repo-root default branch only after the PR is `MERGED`; `{}` is `{}`.",
			context.pr_url,
			landing_state.state
		);
	}

	let merge_commit = github::wait_for_pull_request_merge_commit(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
	)?;

	ensure_already_merged_manual_land_recovery_ready(context, &landing_state, &merge_commit)?;

	Ok(Some(ManualLandRecoveryOutcome { merge_commit }))
}

fn current_checkout_is_repo_root_default_branch(context: &ManualLandContext) -> Result<bool> {
	let canonical_checkout = fs::canonicalize(&context.worktree_root).wrap_err_with(|| {
		format!("Failed to canonicalize current checkout `{}`.", context.worktree_root.display())
	})?;
	let canonical_repo_root =
		fs::canonicalize(&context.canonical_repo_root).wrap_err_with(|| {
			format!(
				"Failed to canonicalize configured repo root `{}`.",
				context.canonical_repo_root.display()
			)
		})?;

	Ok(canonical_checkout == canonical_repo_root
		&& context.current_branch == context.repository.default_branch)
}

fn ensure_already_merged_manual_land_recovery_ready(
	context: &ManualLandContext,
	landing_state: &PullRequestLandingState,
	merge_commit: &str,
) -> Result<()> {
	ensure_already_merged_manual_land_recovery_state(context, landing_state)?;

	default_branch_sync::preflight_repo_root_default_branch_sync(
		&context.canonical_repo_root,
		&context.repository.default_branch,
		Some(context.default_branch_git_credentials()),
	)?;

	ensure_repo_root_default_branch_current(
		&context.canonical_repo_root,
		&context.repository.default_branch,
	)?;
	ensure_merge_commit_reachable_from_default_branch(
		&context.canonical_repo_root,
		&context.pr_url,
		merge_commit,
		&context.repository.default_branch,
	)?;
	ensure_manual_land_recovery_lane_cleanup_complete(context, landing_state)?;
	ensure_manual_land_left_no_merged_worktree_cleanup_debt(context)?;

	Ok(())
}

fn ensure_already_merged_manual_land_recovery_state(
	context: &ManualLandContext,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if landing_state.base_ref_name != context.repository.default_branch {
		eyre::bail!(
			"Pull request `{}` targets base branch `{}`, but manual land recovery only accepts already-merged PRs into `{}`.",
			context.pr_url,
			landing_state.base_ref_name,
			context.repository.default_branch
		);
	}
	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Pull request `{}` is `{}`; manual land recovery only accepts already-merged PRs.",
			context.pr_url,
			landing_state.state
		);
	}
	if landing_state.head_ref_name.trim().is_empty() {
		eyre::bail!(
			"Pull request `{}` does not expose the landed head branch required to verify lane cleanup.",
			context.pr_url
		);
	}
	if landing_state.head_ref_name == context.repository.default_branch {
		eyre::bail!(
			"Pull request `{}` uses the default branch `{}` as its head; manual land recovery cannot prove lane cleanup safely.",
			context.pr_url,
			context.repository.default_branch
		);
	}

	Ok(())
}

fn ensure_repo_root_default_branch_current(repo_root: &Path, default_branch: &str) -> Result<()> {
	let local_head = run_git_capture(repo_root, &["rev-parse", "HEAD"])?;
	let tracking_ref = format!("refs/remotes/origin/{default_branch}");
	let remote_head = run_git_capture(repo_root, &["rev-parse", tracking_ref.as_str()])?;

	if local_head == remote_head {
		return Ok(());
	}

	eyre::bail!(
		"Configured repo root `{}` is on `{default_branch}` but is not current with `{tracking_ref}`; sync the default branch before retrying manual land recovery.",
		repo_root.display()
	);
}

fn ensure_merge_commit_reachable_from_default_branch(
	repo_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	default_branch: &str,
) -> Result<()> {
	let status = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["merge-base", "--is-ancestor", merge_commit, "HEAD"])
		.status()?;

	if status.success() {
		return Ok(());
	}
	if status.code() == Some(1) {
		eyre::bail!(
			"Configured repo root `{}` is on `{default_branch}` but does not contain merge commit `{merge_commit}` for `{pr_url}`.",
			repo_root.display()
		);
	}

	eyre::bail!(
		"`git merge-base --is-ancestor {merge_commit} HEAD` failed in `{}` with status `{}`.",
		repo_root.display(),
		status
	);
}

fn ensure_manual_land_recovery_lane_cleanup_complete(
	context: &ManualLandContext,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	let pr_head_branch = landing_state.head_ref_name.as_str();

	if local_branch_exists(&context.canonical_repo_root, pr_head_branch)? {
		eyre::bail!(
			"Manual land recovery for `{}` requires the landed lane cleanup to be complete, but local branch `{pr_head_branch}` still exists.",
			context.pr_url
		);
	}

	let worktree_paths = linked_worktree_paths_for_landed_head_under_root(
		&context.canonical_repo_root,
		&context.project_worktree_root,
		pr_head_branch,
		&landing_state.head_ref_oid,
	)?;

	if worktree_paths.is_empty() {
		return Ok(());
	}

	let details =
		worktree_paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ");

	eyre::bail!(
		"Manual land recovery for `{}` requires the landed lane cleanup to be complete, but branch `{pr_head_branch}` or its head `{}` is still checked out under `{}`: {details}.",
		context.pr_url,
		landing_state.head_ref_oid,
		context.project_worktree_root.display()
	);
}

fn local_branch_exists(repo_root: &Path, branch_name: &str) -> Result<bool> {
	let ref_name = format!("refs/heads/{branch_name}");
	let status = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["show-ref", "--verify", "--quiet", ref_name.as_str()])
		.status()?;

	if status.success() {
		return Ok(true);
	}
	if status.code() == Some(1) {
		return Ok(false);
	}

	eyre::bail!(
		"`git show-ref --verify --quiet {ref_name}` failed in `{}` with status `{}`.",
		repo_root.display(),
		status
	);
}

fn linked_worktree_paths_for_landed_head_under_root(
	repo_root: &Path,
	worktree_root: &Path,
	branch_name: &str,
	head_oid: &str,
) -> Result<Vec<PathBuf>> {
	let output = run_git_capture(repo_root, &["worktree", "list", "--porcelain"])?;
	let mut matches = Vec::new();
	let mut current_path: Option<PathBuf> = None;
	let mut current_head: Option<String> = None;
	let mut current_branch: Option<String> = None;

	for line in output.lines() {
		if line.is_empty() {
			push_matching_worktree_path(
				&mut matches,
				&mut current_path,
				&mut current_head,
				&mut current_branch,
				worktree_root,
				branch_name,
				head_oid,
			)?;

			continue;
		}

		if let Some(path) = line.strip_prefix("worktree ") {
			push_matching_worktree_path(
				&mut matches,
				&mut current_path,
				&mut current_head,
				&mut current_branch,
				worktree_root,
				branch_name,
				head_oid,
			)?;

			current_path = Some(PathBuf::from(path));
		} else if let Some(head) = line.strip_prefix("HEAD ") {
			current_head = Some(head.to_owned());
		} else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
			current_branch = Some(branch.to_owned());
		}
	}

	push_matching_worktree_path(
		&mut matches,
		&mut current_path,
		&mut current_head,
		&mut current_branch,
		worktree_root,
		branch_name,
		head_oid,
	)?;

	Ok(matches)
}

fn push_matching_worktree_path(
	matches: &mut Vec<PathBuf>,
	path: &mut Option<PathBuf>,
	head: &mut Option<String>,
	branch: &mut Option<String>,
	worktree_root: &Path,
	branch_name: &str,
	head_oid: &str,
) -> Result<()> {
	if (branch.as_deref() == Some(branch_name) || head.as_deref() == Some(head_oid))
		&& let Some(path) = path.take()
		&& checkout_path_is_under_worktree_root(&path, worktree_root)?
	{
		matches.push(path);
	}

	*path = None;
	*head = None;
	*branch = None;

	Ok(())
}

fn checkout_path_is_under_worktree_root(path: &Path, worktree_root: &Path) -> Result<bool> {
	if !path.exists() || !worktree_root.exists() {
		return Ok(false);
	}

	let canonical_path = fs::canonicalize(path)?;
	let canonical_root = fs::canonicalize(worktree_root)?;

	Ok(canonical_path.starts_with(&canonical_root) && canonical_path != canonical_root)
}

fn prepare_closeout(
	config: &ServiceConfig,
	workflow: WorkflowDocument,
	authority: &str,
) -> Result<PreparedCloseout> {
	let tracker_policy = workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state().to_owned();
	let needs_attention_label = tracker_policy.needs_attention_label().to_owned();
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;
	let issue = tracker
		.get_issue_by_identifier(&authority.to_ascii_uppercase())?
		.ok_or_else(|| eyre::eyre!("Tracker does not contain issue `{authority}`."))?;

	ensure_manual_closeout_issue_scope(&tracker, &issue, config.service_id())?;

	Ok(PreparedCloseout {
		tracker,
		issue,
		completed_state,
		service_id: config.service_id().to_owned(),
		needs_attention_label,
	})
}

fn ensure_manual_land_checkout_is_managed_lane(
	checkout_root: &Path,
	project_worktree_root: &Path,
	issue_identifier: &str,
) -> Result<()> {
	let canonical_checkout = fs::canonicalize(checkout_root).wrap_err_with(|| {
		format!("Failed to canonicalize current lane checkout `{}`.", checkout_root.display())
	})?;
	let canonical_worktree_root = fs::canonicalize(project_worktree_root).wrap_err_with(|| {
		format!(
			"Failed to canonicalize configured worktree root `{}`.",
			project_worktree_root.display()
		)
	})?;

	if canonical_checkout.starts_with(&canonical_worktree_root)
		&& canonical_checkout != canonical_worktree_root
	{
		return Ok(());
	}

	eyre::bail!(
		"`decodex land` for issue `{issue_identifier}` must run from a managed lane under worktree_root `{}` so successful land can clean up the worktree and branch.",
		project_worktree_root.display()
	);
}

fn ensure_manual_closeout_issue_scope<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let active_label = tracker::automation_active_label(service_id);

	if tracker::issue_has_label_with_server_confirmation(tracker, issue, &active_label)? {
		return Ok(());
	}

	eyre::bail!(
		"Issue `{}` is not owned by service `{service_id}`; `decodex land` requires label `{active_label}`.",
		issue.identifier
	);
}

fn apply_closeout(
	checkout_root: &Path,
	prepared: &PreparedCloseout,
	pr_url: &str,
	merge_commit: &str,
	branch_name: &str,
	landed_change_record: &str,
) -> Result<()> {
	if prepared.issue.state.name != prepared.completed_state {
		let state_id =
			prepared.issue.state_id_for_name(&prepared.completed_state).ok_or_else(|| {
				eyre::eyre!(
					"Issue `{}` does not expose tracker state `{}` on its team.",
					prepared.issue.identifier,
					prepared.completed_state
				)
			})?;

		prepared.tracker.update_issue_state(prepared.issue.id.as_str(), state_id)?;
	}
	if !manual_land_closeout_matches(
		checkout_root,
		pr_url,
		merge_commit,
		branch_name,
		landed_change_record,
	)? {
		prepared.tracker.create_comment(
			prepared.issue.id.as_str(),
			format!(
				"decodex land completed\n\n- pr_url: `{pr_url}`\n- merge_commit: `{merge_commit}`\n- branch: `{branch_name}`\n- landed_change: `{landed_change_record}`"
			)
			.as_str(),
		)?;

		write_manual_land_closeout_marker(
			checkout_root,
			pr_url,
			merge_commit,
			branch_name,
			landed_change_record,
		)?;
	}

	Ok(())
}

fn clear_manual_closeout_issue_scope<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
	needs_attention_label: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let closeout_labels = [
		tracker::automation_active_label(service_id),
		tracker::automation_queue_label(service_id),
		needs_attention_label.to_owned(),
	];

	for label_name in closeout_labels {
		clear_manual_closeout_issue_label(tracker, issue, &label_name)?;
	}

	Ok(())
}

fn clear_manual_closeout_issue_label<T>(
	tracker: &T,
	issue: &TrackerIssue,
	label_name: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if let Err(error) = tracker::set_issue_label_presence(tracker, issue, label_name, false)
		&& !linear_label_not_on_issue_error(&error)
	{
		return Err(error);
	}

	Ok(())
}

fn clear_manual_closeout_runtime_state(state_store: &StateStore, issue_id: &str) -> Result<()> {
	state_store.succeed_active_run_attempts_for_issue(issue_id).wrap_err_with(|| {
		format!("Failed to finalize active runtime attempts for issue `{issue_id}`.")
	})?;
	state_store
		.clear_lease(issue_id)
		.wrap_err_with(|| format!("Failed to clear runtime lease for issue `{issue_id}`."))?;
	state_store.clear_worktree(issue_id).wrap_err_with(|| {
		format!("Failed to clear runtime worktree state for issue `{issue_id}`.")
	})?;

	Ok(())
}

fn linear_label_not_on_issue_error(error: &Report) -> bool {
	error
		.chain()
		.any(|source| source.to_string().to_ascii_lowercase().contains("label not on issue"))
}

fn manual_land_closeout_marker_path(checkout_root: &Path) -> Result<PathBuf> {
	let Some(git_dir) = config::git_dir_for_checkout(checkout_root)? else {
		eyre::bail!(
			"Current checkout `{}` does not expose a Git administrative directory.",
			checkout_root.display()
		);
	};

	Ok(git_dir.join(MANUAL_LAND_CLOSEOUT_MARKER_GIT_PATH))
}

fn manual_land_closeout_matches(
	checkout_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	branch_name: &str,
	landed_change_record: &str,
) -> Result<bool> {
	let Some(marker) = read_manual_land_closeout_marker(checkout_root)? else {
		return Ok(false);
	};

	Ok(marker.pr_url.as_deref() == Some(pr_url)
		&& marker.merge_commit.as_deref() == Some(merge_commit)
		&& marker.branch_name.as_deref() == Some(branch_name)
		&& marker.landed_change.as_deref() == Some(landed_change_record))
}

fn read_manual_land_closeout_marker(
	checkout_root: &Path,
) -> Result<Option<ManualLandCloseoutMarkerRecord>> {
	let marker_path = manual_land_closeout_marker_path(checkout_root)?;
	let marker_body = match fs::read_to_string(&marker_path) {
		Ok(marker_body) => marker_body,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => {
			return Err(error).wrap_err_with(|| {
				format!("Failed to read manual land closeout marker `{}`.", marker_path.display())
			});
		},
	};
	let mut marker = ManualLandCloseoutMarkerRecord::default();

	for line in marker_body.lines() {
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};

		match key {
			"pr_url" => marker.pr_url = Some(value.to_owned()),
			"merge_commit" => marker.merge_commit = Some(value.to_owned()),
			"branch_name" => marker.branch_name = Some(value.to_owned()),
			"landed_change" => marker.landed_change = Some(value.to_owned()),
			_ => {},
		}
	}

	Ok(Some(marker))
}

fn write_manual_land_closeout_marker(
	checkout_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	branch_name: &str,
	landed_change_record: &str,
) -> Result<()> {
	let marker_path = manual_land_closeout_marker_path(checkout_root)?;
	let Some(marker_dir) = marker_path.parent() else {
		eyre::bail!(
			"Manual land closeout marker path `{}` has no parent directory.",
			marker_path.display()
		);
	};

	fs::create_dir_all(marker_dir).wrap_err_with(|| {
		format!(
			"Failed to create manual land closeout marker directory `{}`.",
			marker_dir.display()
		)
	})?;
	fs::write(
		&marker_path,
		format!(
			"pr_url={pr_url}\nmerge_commit={merge_commit}\nbranch_name={branch_name}\nlanded_change={landed_change_record}\n"
		),
	)
	.wrap_err_with(|| {
		format!("Failed to write manual land closeout marker `{}`.", marker_path.display())
	})?;

	Ok(())
}

fn run_git_capture(cwd: &Path, args: &[&str]) -> Result<String> {
	let output = Command::new("git").arg("-C").arg(cwd).args(args).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		let stdout = String::from_utf8_lossy(&output.stdout);
		let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

		eyre::bail!("`git {}` failed in `{}`: {detail}", args.join(" "), cwd.display());
	}

	Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_git_checked_with_stdio(cwd: &Path, args: &[&str]) -> Result<()> {
	let status = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(args)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()?;

	if status.success() {
		return Ok(());
	}

	eyre::bail!("`git {}` failed in `{}`.", args.join(" "), cwd.display());
}

#[cfg(test)]
mod tests {
	use std::{
		cell::RefCell,
		collections::HashMap,
		env, fs,
		os::unix::fs::PermissionsExt,
		path::{Path, PathBuf},
		process::Command,
	};

	use tempfile::TempDir;

	use crate::{
		config::ServiceConfig,
		manual::{self, LandExecutionMode, ManualAuthority, ManualLandContext, ManualLandRequest},
		prelude::eyre,
		pull_request::PullRequestLandingState,
		runtime, state,
		test_support::TestEnvVarGuard,
		tracker::{IssueTracker, TrackerIssue, TrackerLabel, TrackerState, TrackerTeam},
		workflow::WorkflowDocument,
		worktree::WorktreeManager,
	};

	struct TestTracker {
		issues_by_label: HashMap<String, Vec<TrackerIssue>>,
		label_removals: RefCell<Vec<Vec<String>>>,
		label_removal_error: Option<String>,
	}

	struct MergedManualLandBranch {
		branch_name: String,
		head_oid: String,
		merge_commit: String,
		worktree_path: PathBuf,
	}

	impl TestTracker {
		fn new() -> Self {
			Self {
				issues_by_label: HashMap::new(),
				label_removals: RefCell::new(Vec::new()),
				label_removal_error: None,
			}
		}

		fn with_label_issues(mut self, label_name: &str, issues: Vec<TrackerIssue>) -> Self {
			self.issues_by_label.insert(label_name.to_owned(), issues);

			self
		}

		fn with_label_removal_error(mut self, message: &str) -> Self {
			self.label_removal_error = Some(message.to_owned());

			self
		}
	}

	impl IssueTracker for TestTracker {
		fn list_issues_with_label(
			&self,
			label_name: &str,
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			Ok(self.issues_by_label.get(label_name).cloned().unwrap_or_default())
		}

		fn find_team_label_id(
			&self,
			_team_id: &str,
			_label_name: &str,
		) -> crate::prelude::Result<Option<String>> {
			Ok(None)
		}

		fn get_issue_by_identifier(
			&self,
			_issue_identifier: &str,
		) -> crate::prelude::Result<Option<TrackerIssue>> {
			Ok(None)
		}

		fn refresh_issues(
			&self,
			_issue_ids: &[String],
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			Ok(Vec::new())
		}

		fn list_comments(
			&self,
			_issue_id: &str,
		) -> crate::prelude::Result<Vec<crate::tracker::TrackerComment>> {
			Ok(Vec::new())
		}

		fn update_issue_state(
			&self,
			_issue_id: &str,
			_state_id: &str,
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn add_issue_labels(
			&self,
			_issue_id: &str,
			_label_ids: &[String],
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn remove_issue_labels(
			&self,
			_issue_id: &str,
			label_ids: &[String],
		) -> crate::prelude::Result<()> {
			self.label_removals.borrow_mut().push(label_ids.to_vec());

			if let Some(message) = self.label_removal_error.as_ref() {
				eyre::bail!("{message}");
			}

			Ok(())
		}

		fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
			Ok(())
		}
	}

	fn init_git_checkout(temp_dir: &TempDir, directory_name: &str) -> PathBuf {
		let checkout = temp_dir.path().join(directory_name);

		assert!(
			Command::new("git")
				.args(["init", "-b", "main"])
				.current_dir(temp_dir.path())
				.arg(&checkout)
				.status()
				.expect("git init should run")
				.success()
		);
		assert!(
			Command::new("git")
				.args(["config", "user.name", "Decodex Tests"])
				.current_dir(&checkout)
				.status()
				.expect("git config should run")
				.success()
		);
		assert!(
			Command::new("git")
				.args(["config", "user.email", "decodex-tests@example.com"])
				.current_dir(&checkout)
				.status()
				.expect("git config should run")
				.success()
		);
		assert!(
			Command::new("git")
				.args(["config", "commit.gpgsign", "false"])
				.current_dir(&checkout)
				.status()
				.expect("git config should run")
				.success()
		);

		checkout
	}

	fn git_success(cwd: &Path, args: &[&str]) {
		assert!(
			Command::new("git")
				.args(args)
				.current_dir(cwd)
				.status()
				.expect("git command should run")
				.success(),
			"git {:?} should succeed",
			args
		);
	}

	fn git_add_and_commit(cwd: &Path, pathspec: &str, message: &str) {
		assert!(
			Command::new("git")
				.args(["add", pathspec])
				.current_dir(cwd)
				.status()
				.expect("git add should run")
				.success()
		);
		assert!(
			Command::new("git")
				.args(["commit", "-m", message])
				.current_dir(cwd)
				.status()
				.expect("git commit should run")
				.success()
		);
	}

	fn init_git_checkout_with_origin(temp_dir: &TempDir) -> PathBuf {
		let remote_root = temp_dir.path().join("origin.git");
		let checkout = temp_dir.path().join("repo");

		assert!(
			Command::new("git")
				.args(["init", "--bare", "--initial-branch", "main"])
				.arg(&remote_root)
				.status()
				.expect("bare origin should init")
				.success()
		);
		assert!(
			Command::new("git")
				.args(["clone"])
				.arg(&remote_root)
				.arg(&checkout)
				.status()
				.expect("repo should clone")
				.success()
		);

		git_success(&checkout, &["config", "user.name", "Decodex Tests"]);
		git_success(&checkout, &["config", "user.email", "decodex-tests@example.com"]);
		git_success(&checkout, &["config", "commit.gpgsign", "false"]);

		fs::write(checkout.join("README.md"), "bootstrap\n").expect("readme should write");

		git_add_and_commit(&checkout, "README.md", "bootstrap repo");
		git_success(&checkout, &["push", "origin", "main"]);

		checkout
	}

	fn repo_root_manual_land_context(repo_root: &Path, worktree_root: &Path) -> ManualLandContext {
		ManualLandContext {
			cwd: repo_root.to_path_buf(),
			current_branch: String::from("main"),
			worktree_root: repo_root.to_path_buf(),
			project_worktree_root: worktree_root.to_path_buf(),
			canonical_repo_root: repo_root.to_path_buf(),
			authority: ManualAuthority::Manual,
			service_id: String::from("decodex"),
			workflow: sample_workflow(),
			github_token_env_var: String::from("GITHUB_TOKEN"),
			github_token: String::from("test-token"),
			repository: crate::github::RepositoryContext {
				owner: String::from("hack-ink"),
				name: String::from("decodex"),
				default_branch: String::from("main"),
				merge_commit_allowed: true,
			},
			prepared_closeout: None,
			pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
			review_branch: String::from("main"),
		}
	}

	fn merge_manual_land_test_branch(
		repo_root: &Path,
		worktree_root: &Path,
	) -> MergedManualLandBranch {
		let worktree_manager = WorktreeManager::new("decodex", repo_root, worktree_root);
		let worktree = worktree_manager
			.ensure_worktree("manual-land-cleanup", false)
			.expect("manual land worktree should create");

		fs::write(worktree.path.join("feature.txt"), "manual land\n")
			.expect("feature file should write");

		git_add_and_commit(&worktree.path, "feature.txt", "manual land feature");

		let head_oid = manual::run_git_capture(&worktree.path, &["rev-parse", "HEAD"])
			.expect("PR head should read");

		git_success(repo_root, &["merge", "--no-ff", &worktree.branch_name, "-m", "land feature"]);

		let merge_commit =
			manual::run_git_capture(repo_root, &["rev-parse", "HEAD"]).expect("merge head");

		git_success(repo_root, &["push", "origin", "main"]);

		MergedManualLandBranch {
			branch_name: worktree.branch_name,
			head_oid,
			merge_commit,
			worktree_path: worktree.path,
		}
	}

	fn remove_test_lane_checkout(repo_root: &Path, worktree_path: &Path, branch_name: &str) {
		git_success(worktree_path, &["checkout", "--detach"]);
		git_success(repo_root, &["branch", "-D", branch_name]);
		git_success(
			repo_root,
			&[
				"worktree",
				"remove",
				"--force",
				worktree_path.to_str().expect("worktree path should be UTF-8"),
			],
		);
	}

	fn create_dirty_merged_worktree_debt(repo_root: &Path, worktree_root: &Path) {
		let worktree_manager = WorktreeManager::new("decodex", repo_root, worktree_root);
		let worktree =
			worktree_manager.ensure_worktree("XY-999", false).expect("debt worktree should create");

		fs::write(worktree.path.join("debt.txt"), "debt\n").expect("debt file should write");

		git_add_and_commit(&worktree.path, "debt.txt", "debt feature");
		git_success(repo_root, &["merge", "--no-ff", &worktree.branch_name, "-m", "land debt"]);
		git_success(repo_root, &["push", "origin", "main"]);

		fs::write(worktree.path.join("debt.txt"), "dirty debt\n")
			.expect("debt worktree should become dirty");
	}

	fn merged_manual_land_state(branch_name: &str, head_oid: &str) -> PullRequestLandingState {
		let mut landing_state = sample_landing_state();

		landing_state.state = String::from("MERGED");
		landing_state.base_ref_name = String::from("main");
		landing_state.head_ref_name = branch_name.to_owned();
		landing_state.head_ref_oid = head_oid.to_owned();

		landing_state
	}

	fn install_fake_landing_state_gh(
		temp_dir: &TempDir,
		state: &str,
		branch_name: &str,
		head_oid: &str,
		merge_commit: &str,
	) -> TestEnvVarGuard {
		let fake_gh_dir = temp_dir.path().join("fake-recovery-bin");
		let fake_gh_path = fake_gh_dir.join("gh");

		fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
		fs::write(
			&fake_gh_path,
			format!(
				"#!/bin/sh\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"graphql\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
				serde_json::json!({
					"data": {
						"repository": {
							"pullRequest": {
								"url": "https://github.com/hack-ink/decodex/pull/64",
								"state": state,
								"isDraft": false,
								"reviewDecision": "APPROVED",
								"baseRefName": "main",
								"mergeable": "MERGEABLE",
								"mergeStateStatus": "CLEAN",
								"headRefName": branch_name,
								"headRefOid": head_oid,
								"reviewRequests": { "totalCount": 0 },
								"reviewThreads": {
									"nodes": [],
									"pageInfo": { "hasNextPage": false, "endCursor": null },
								},
								"commits": {
									"nodes": [
										{
											"commit": {
												"statusCheckRollup": { "state": "SUCCESS" },
											},
										},
									],
								},
							},
						},
					},
				}),
				serde_json::json!({
					"state": state,
					"headRefOid": head_oid,
					"mergeCommit": { "oid": merge_commit },
				}),
			),
		)
		.expect("fake gh script should write");

		let mut permissions =
			fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

		#[cfg(unix)]
		{
			PermissionsExt::set_mode(&mut permissions, 0o755);
		}

		fs::set_permissions(&fake_gh_path, permissions)
			.expect("fake gh script should become executable");

		let path_env = env::var("PATH").unwrap_or_default();

		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display()))
	}

	fn sample_workflow() -> WorkflowDocument {
		WorkflowDocument::parse_markdown(
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
max_turns = 8
max_retry_backoff_ms = 300000
max_concurrent_agents = 1
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Test workflow.
"#,
		)
		.expect("sample workflow should parse")
	}

	fn install_fake_admin_merge_gh(
		temp_dir: &TempDir,
		merged_head_oid: &str,
	) -> (TestEnvVarGuard, PathBuf) {
		install_fake_admin_merge_gh_with_merge_exit_code(
			temp_dir,
			merged_head_oid,
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
			0,
		)
	}

	fn install_fake_admin_merge_gh_with_merge_exit_code(
		temp_dir: &TempDir,
		merged_head_oid: &str,
		pr_head_oid: &str,
		merge_subject: &str,
		merge_exit_code: i32,
	) -> (TestEnvVarGuard, PathBuf) {
		let fake_gh_dir = temp_dir.path().join("fake-bin");
		let fake_gh_path = fake_gh_dir.join("gh");
		let invocation_log_path = temp_dir.path().join("gh-invocation.log");

		fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
		fs::write(
			&fake_gh_path,
			format!(
				"#!/bin/sh\n\
printf '%s\\n' \"$*\" >> '{}'\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"merge\" ]; then\n\
  exit {}\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"api\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
				invocation_log_path.display(),
				merge_exit_code,
				serde_json::json!({
					"state": "MERGED",
					"headRefOid": pr_head_oid,
					"mergeCommit": { "oid": merged_head_oid },
				}),
				serde_json::json!({
					"commit": { "message": format!("{merge_subject}\n\n") },
				}),
			),
		)
		.expect("fake gh script should write");

		let mut permissions =
			fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

		#[cfg(unix)]
		{
			PermissionsExt::set_mode(&mut permissions, 0o755);
		}

		fs::set_permissions(&fake_gh_path, permissions)
			.expect("fake gh script should become executable");

		let path_env = env::var("PATH").unwrap_or_default();

		(
			TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display())),
			invocation_log_path,
		)
	}

	#[test]
	fn issue_identifier_helpers_recognize_lane_directory_names() {
		let inferred =
			manual::infer_issue_identifier_from_worktree_root(Path::new("/tmp/.worktrees/XY-225"))
				.expect("issue identifier should infer from worktree basename");

		assert_eq!(inferred, "XY-225");
		assert!(!manual::looks_like_issue_identifier("decodex"));
		assert!(!manual::looks_like_issue_identifier("feature-branch"));
		assert!(manual::looks_like_issue_identifier("xy-225"));
	}

	#[test]
	fn landing_cleanliness_ignores_untracked_decodex_runtime_markers() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = init_git_checkout(&temp_dir, "repo");

		fs::write(checkout.join(state::RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
			.expect("activity marker should write");
		manual::ensure_clean_worktree(&checkout)
			.expect("untracked activity marker should not block landing");
	}

	#[test]
	fn landing_cleanliness_rejects_blocking_worktree_statuses() {
		fn assert_blocks(checkout: &Path, case_name: &str) {
			let error = manual::ensure_clean_worktree(checkout).expect_err(case_name);

			assert!(
				error.to_string().contains("uncommitted changes"),
				"unexpected error for `{case_name}`: {error:?}"
			);
		}
		{
			let temp_dir = TempDir::new().expect("temp dir should create");
			let checkout = init_git_checkout(&temp_dir, "repo");

			fs::write(checkout.join("scratch.txt"), "debug\n").expect("scratch file should write");

			assert_blocks(&checkout, "untracked non-runtime files should block landing");
		}
		{
			let temp_dir = TempDir::new().expect("temp dir should create");
			let checkout = init_git_checkout(&temp_dir, "repo");
			let nested_dir = checkout.join("nested");

			fs::create_dir_all(&nested_dir).expect("nested directory should create");
			fs::write(nested_dir.join(state::RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
				.expect("nested activity marker should write");

			assert_blocks(&checkout, "nested runtime marker should still block landing");
		}
		{
			let temp_dir = TempDir::new().expect("temp dir should create");
			let checkout = init_git_checkout(&temp_dir, "repo");
			let marker_path = checkout.join(state::RUN_ACTIVITY_MARKER_FILE);

			fs::write(&marker_path, "idle\n").expect("activity marker should write");

			git_add_and_commit(
				&checkout,
				state::RUN_ACTIVITY_MARKER_FILE,
				"track activity marker for test",
			);

			fs::write(&marker_path, "agent_run\n").expect("activity marker should update");

			assert_blocks(&checkout, "tracked runtime marker changes should block landing");
		}
	}

	#[test]
	fn landing_state_validation_blocks_base_drift_except_after_merge() {
		let error = manual::validate_landing_state(
			&sample_landing_state(),
			"https://github.com/hack-ink/decodex/pull/64",
			"main",
			"XY-225",
			"deadbeef",
		)
		.expect_err("non-default-base PR should be rejected");

		assert!(error.to_string().contains("targets base branch `release/1.x`"));
		assert!(error.to_string().contains("only lands into `main`"));

		let mut landing_state = sample_landing_state();

		landing_state.state = String::from("MERGED");

		let mode = manual::validate_landing_state(
			&landing_state,
			"https://github.com/hack-ink/decodex/pull/64",
			"release/1.x",
			"XY-225",
			"deadbeef",
		)
		.expect("merged PR should resume closeout");

		assert_eq!(mode, LandExecutionMode::CloseoutOnly);
	}

	#[test]
	fn execute_land_merge_uses_admin_merge() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = init_git_checkout(&temp_dir, "repo");
		let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh(&temp_dir, "cafebabe");
		let context = manual::ManualLandContext {
			cwd: checkout.clone(),
			current_branch: String::from("xy-225"),
			worktree_root: temp_dir.path().join(".worktrees"),
			project_worktree_root: temp_dir.path().join(".worktrees"),
			canonical_repo_root: checkout,
			authority: ManualAuthority::Manual,
			service_id: String::from("decodex"),
			workflow: sample_workflow(),
			github_token_env_var: String::from("GITHUB_TOKEN"),
			github_token: String::from("test-token"),
			repository: crate::github::RepositoryContext {
				owner: String::from("hack-ink"),
				name: String::from("decodex"),
				default_branch: String::from("main"),
				merge_commit_allowed: true,
			},
			prepared_closeout: None,
			pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
			review_branch: String::from("xy-225"),
		};
		let merge_commit = manual::execute_land_merge(
			&context,
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
			LandExecutionMode::MergeAndCloseout,
		)
		.expect("manual land should admin-merge successfully");

		assert_eq!(merge_commit, "cafebabe");
		assert_eq!(
			fs::read_to_string(&invocation_log_path)
				.expect("fake gh invocation log should read")
				.lines()
				.collect::<Vec<_>>(),
			vec![
				"pr merge --admin --merge --match-head-commit deadbeefdeadbeefdeadbeefdeadbeefdeadbeef --subject {\"schema\":\"decodex/commit/1\",\"summary\":\"ship hotfix\",\"authority\":\"manual\"} --body  https://github.com/hack-ink/decodex/pull/64",
				"pr view https://github.com/hack-ink/decodex/pull/64 --json state,headRefOid,mergeCommit",
			]
		);
	}

	#[test]
	fn execute_land_merge_tolerates_already_merged_merge_race() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = init_git_checkout(&temp_dir, "repo");
		let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh_with_merge_exit_code(
			&temp_dir,
			"cafebabe",
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
			1,
		);
		let context = manual::ManualLandContext {
			cwd: checkout.clone(),
			current_branch: String::from("xy-225"),
			worktree_root: temp_dir.path().join(".worktrees"),
			project_worktree_root: temp_dir.path().join(".worktrees"),
			canonical_repo_root: checkout,
			authority: ManualAuthority::Manual,
			service_id: String::from("decodex"),
			workflow: sample_workflow(),
			github_token_env_var: String::from("GITHUB_TOKEN"),
			github_token: String::from("test-token"),
			repository: crate::github::RepositoryContext {
				owner: String::from("hack-ink"),
				name: String::from("decodex"),
				default_branch: String::from("main"),
				merge_commit_allowed: true,
			},
			prepared_closeout: None,
			pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
			review_branch: String::from("xy-225"),
		};
		let merge_commit = manual::execute_land_merge(
			&context,
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
			LandExecutionMode::MergeAndCloseout,
		)
		.expect("manual land should accept an already-merged PR race");

		assert_eq!(merge_commit, "cafebabe");
		assert_eq!(
			fs::read_to_string(&invocation_log_path)
				.expect("fake gh invocation log should read")
				.lines()
				.collect::<Vec<_>>(),
			vec![
				"pr merge --admin --merge --match-head-commit deadbeefdeadbeefdeadbeefdeadbeefdeadbeef --subject {\"schema\":\"decodex/commit/1\",\"summary\":\"ship hotfix\",\"authority\":\"manual\"} --body  https://github.com/hack-ink/decodex/pull/64",
				"pr view https://github.com/hack-ink/decodex/pull/64 --json state,headRefOid,mergeCommit",
				"pr view https://github.com/hack-ink/decodex/pull/64 --json state,headRefOid,mergeCommit",
			]
		);
	}

	#[test]
	fn load_authoritative_landed_change_record_uses_merge_commit_subject() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = init_git_checkout(&temp_dir, "repo");
		let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh_with_merge_exit_code(
			&temp_dir,
			"cafebabe",
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			r#"{"schema":"decodex/commit/1","summary":"actual merge subject","authority":"manual"}"#,
			0,
		);
		let context = manual::ManualLandContext {
			cwd: checkout.clone(),
			current_branch: String::from("xy-225"),
			worktree_root: temp_dir.path().join(".worktrees"),
			project_worktree_root: temp_dir.path().join(".worktrees"),
			canonical_repo_root: checkout,
			authority: ManualAuthority::Manual,
			service_id: String::from("decodex"),
			workflow: sample_workflow(),
			github_token_env_var: String::from("GITHUB_TOKEN"),
			github_token: String::from("test-token"),
			repository: crate::github::RepositoryContext {
				owner: String::from("hack-ink"),
				name: String::from("decodex"),
				default_branch: String::from("main"),
				merge_commit_allowed: true,
			},
			prepared_closeout: None,
			pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
			review_branch: String::from("xy-225"),
		};
		let landed_change_record =
			manual::load_authoritative_landed_change_record(&context, "cafebabe")
				.expect("merge commit subject should load");

		assert_eq!(
			landed_change_record,
			r#"{"schema":"decodex/commit/1","summary":"actual merge subject","authority":"manual"}"#
		);
		assert_eq!(
			fs::read_to_string(&invocation_log_path)
				.expect("fake gh invocation log should read")
				.lines()
				.collect::<Vec<_>>(),
			vec!["api repos/hack-ink/decodex/commits/cafebabe"]
		);
	}

	#[test]
	fn land_authority_validates_issue_override_against_lane() {
		let error = manual::resolve_land_authority(
			Some(Path::new("/tmp/project.toml")),
			Some("XY-999"),
			false,
			Path::new("/tmp/.worktrees/XY-225"),
		)
		.expect_err("mismatched explicit authority should be rejected");

		assert!(error.to_string().contains("does not match the current lane issue `XY-225`"));

		let authority = manual::resolve_land_authority(
			Some(Path::new("/tmp/project.toml")),
			Some("XY-225"),
			false,
			Path::new("/tmp/.worktrees/xy-225"),
		)
		.expect("same issue with different casing should be accepted");

		assert_eq!(authority, ManualAuthority::Issue(String::from("xy-225")));
	}

	#[test]
	fn resolve_authority_accepts_manual_authority() {
		let authority = manual::resolve_authority(
			Some(Path::new("/tmp/project.toml")),
			None,
			true,
			Path::new("/tmp/worktree"),
		)
		.expect("manual authority should resolve");

		assert_eq!(authority, ManualAuthority::Manual);
	}

	#[test]
	fn resolve_pr_url_requires_explicit_pr_for_manual_authority() {
		let error = manual::resolve_pr_url(None, None, true)
			.expect_err("manual authority land should require explicit pr");

		assert!(error.to_string().contains("--manual-authority"));
	}

	#[test]
	fn prepare_closeout_matches_authority_case_insensitively() {
		assert_eq!("xy-225".to_ascii_uppercase(), "XY-225");
	}

	#[test]
	fn manual_closeout_scope_requires_service_ownership() {
		let issue = sample_issue("issue-1", "XY-225", false, &[]);
		let error =
			manual::ensure_manual_closeout_issue_scope(&TestTracker::new(), &issue, "pubfi")
				.expect_err("service ownership should be required");

		assert!(error.to_string().contains("decodex:active:pubfi"));

		let issue = sample_issue("issue-1", "XY-225", false, &[]);
		let tracker =
			TestTracker::new().with_label_issues("decodex:active:pubfi", vec![issue.clone()]);

		manual::ensure_manual_closeout_issue_scope(&tracker, &issue, "pubfi")
			.expect("server-confirmed service ownership should pass");
	}

	#[test]
	fn manual_closeout_clear_removes_present_transient_decodex_labels() {
		for (case_name, labels, expected_label_ids) in [
			(
				"all transient labels present",
				&["decodex:active:pubfi", "decodex:queued:pubfi", "decodex:needs-attention"][..],
				&["team-label-0", "team-label-1", "team-label-2"][..],
			),
			(
				"optional transient labels absent",
				&["decodex:active:pubfi"][..],
				&["team-label-0"][..],
			),
		] {
			let issue = sample_issue("issue-1", "XY-225", true, labels);
			let tracker = TestTracker::new();

			manual::clear_manual_closeout_issue_scope(
				&tracker,
				&issue,
				"pubfi",
				"decodex:needs-attention",
			)
			.expect(case_name);

			let expected_removals = expected_label_ids
				.iter()
				.map(|label_id| vec![(*label_id).to_owned()])
				.collect::<Vec<_>>();

			assert_eq!(tracker.label_removals.borrow().as_slice(), expected_removals.as_slice());
		}
	}

	#[test]
	fn manual_closeout_clear_classifies_label_removal_errors() {
		for (case_name, labels, message, expected_label_ids, expected_error) in [
			(
				"missing label removal is idempotent",
				&["decodex:active:pubfi", "decodex:queued:pubfi", "decodex:needs-attention"][..],
				"Linear GraphQL request failed: Label not on issue",
				&["team-label-0", "team-label-1", "team-label-2"][..],
				None,
			),
			(
				"other label removal errors are preserved",
				&["decodex:active:pubfi"][..],
				"Linear GraphQL request failed: Timeout",
				&["team-label-0"][..],
				Some("Timeout"),
			),
		] {
			let issue = sample_issue("issue-1", "XY-225", true, labels);
			let tracker = TestTracker::new().with_label_removal_error(message);
			let result = manual::clear_manual_closeout_issue_scope(
				&tracker,
				&issue,
				"pubfi",
				"decodex:needs-attention",
			);

			if let Some(expected_error) = expected_error {
				let error = result.expect_err(case_name);

				assert!(error.to_string().contains(expected_error));
			} else {
				result.expect(case_name);
			}

			let expected_removals = expected_label_ids
				.iter()
				.map(|label_id| vec![(*label_id).to_owned()])
				.collect::<Vec<_>>();

			assert_eq!(tracker.label_removals.borrow().as_slice(), expected_removals.as_slice());
		}
	}

	#[test]
	fn manual_closeout_runtime_clear_removes_lane_state() {
		let state_store = state::StateStore::open_in_memory().expect("state store should open");
		let issue = sample_issue("issue-1", "XY-225", true, &["decodex:active:pubfi"]);
		let other_issue = sample_issue("issue-2", "XY-226", true, &["decodex:active:pubfi"]);
		let handoff = state::ReviewHandoffMarker::new(
			"run-1",
			1,
			"y/decodex-xy-225",
			"https://github.com/hack-ink/decodex/pull/67",
			"main",
			"y/decodex-xy-225",
			"deadbeef",
		);

		state_store
			.upsert_lease("decodex", &issue.id, "run-1", "In Progress")
			.expect("issue lease should persist");
		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("issue running attempt should persist");
		state_store
			.record_run_attempt("run-1-starting", &issue.id, 2, "starting")
			.expect("issue starting attempt should persist");
		state_store
			.record_run_attempt("run-1-failed", &issue.id, 3, "failed")
			.expect("issue terminal attempt should persist");
		state_store
			.upsert_worktree("decodex", &issue.id, "y/decodex-xy-225", "/tmp/worktrees/xy-225")
			.expect("issue worktree should persist");
		state_store
			.upsert_review_handoff_marker("decodex", &issue.id, &handoff)
			.expect("issue handoff should persist");
		state_store
			.upsert_lease("decodex", &other_issue.id, "run-2", "In Progress")
			.expect("other issue lease should persist");
		state_store
			.record_run_attempt("run-2", &other_issue.id, 1, "running")
			.expect("other issue running attempt should persist");

		manual::clear_manual_closeout_runtime_state(&state_store, &issue.id)
			.expect("manual closeout runtime state should clear");

		assert!(
			state_store
				.list_leases("decodex")
				.expect("leases should list")
				.iter()
				.all(|lease| lease.issue_id() != issue.id)
		);
		assert!(
			state_store
				.list_leases("decodex")
				.expect("leases should list")
				.iter()
				.any(|lease| lease.issue_id() == other_issue.id)
		);
		assert!(
			state_store
				.worktree_for_issue(&issue.id)
				.expect("worktree lookup should succeed")
				.is_none()
		);
		assert!(
			state_store
				.review_handoff_marker("decodex", &issue.id, "y/decodex-xy-225")
				.expect("handoff lookup should succeed")
				.is_none()
		);
		assert_eq!(
			state_store
				.run_attempt("run-1")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should remain")
				.status(),
			"succeeded"
		);
		assert_eq!(
			state_store
				.run_attempt("run-1-starting")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should remain")
				.status(),
			"succeeded"
		);
		assert_eq!(
			state_store
				.run_attempt("run-1-failed")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should remain")
				.status(),
			"failed"
		);
		assert_eq!(
			state_store
				.run_attempt("run-2")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should remain")
				.status(),
			"running"
		);
	}

	#[test]
	fn manual_land_issue_closeout_removes_managed_lane_worktree_and_branch() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let repo_root = init_git_checkout(&temp_dir, "repo");
		let worktree_root = repo_root.join(".worktrees");

		fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");

		git_add_and_commit(&repo_root, "README.md", "bootstrap repo");

		let worktree_manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let worktree =
			worktree_manager.ensure_worktree("XY-225", false).expect("worktree should create");
		let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh(&temp_dir, "cafebabe");
		let context = manual::ManualLandContext {
			cwd: worktree.path.clone(),
			current_branch: worktree.branch_name.clone(),
			worktree_root: worktree.path.clone(),
			project_worktree_root: worktree_root.clone(),
			canonical_repo_root: repo_root.clone(),
			authority: ManualAuthority::Issue(String::from("XY-225")),
			service_id: String::from("pubfi"),
			workflow: sample_workflow(),
			github_token_env_var: String::from("GITHUB_TOKEN"),
			github_token: String::from("test-token"),
			repository: crate::github::RepositoryContext {
				owner: String::from("hack-ink"),
				name: String::from("decodex"),
				default_branch: String::from("main"),
				merge_commit_allowed: true,
			},
			prepared_closeout: None,
			pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
			review_branch: worktree.branch_name.clone(),
		};

		manual::cleanup_manual_land_lane_checkout(&context)
			.expect("manual land cleanup should remove the lane checkout");

		let gh_invocations =
			fs::read_to_string(invocation_log_path).expect("fake gh invocation log should read");

		assert!(
			gh_invocations
				.contains("api --method DELETE --silent repos/hack-ink/decodex/git/refs/heads/"),
			"manual land cleanup should delete the remote branch through gh api"
		);
		assert!(!worktree.path.exists(), "manual land cleanup should remove the worktree");
		assert!(
			manual::run_git_capture(&repo_root, &["branch", "--list", &worktree.branch_name])
				.expect("local branch list should run")
				.is_empty(),
			"manual land cleanup should delete the local lane branch"
		);
	}

	#[test]
	fn manual_land_manual_authority_removes_managed_lane_worktree_and_branch() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let repo_root = init_git_checkout(&temp_dir, "repo");
		let worktree_root = repo_root.join(".worktrees");

		fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");

		git_add_and_commit(&repo_root, "README.md", "bootstrap repo");

		let worktree_manager = WorktreeManager::new("decodex", &repo_root, &worktree_root);
		let worktree = worktree_manager
			.ensure_worktree("manual-land-cleanup", false)
			.expect("worktree should create");
		let (_path_guard, _invocation_log_path) =
			install_fake_admin_merge_gh(&temp_dir, "cafebabe");
		let context = manual::ManualLandContext {
			cwd: worktree.path.clone(),
			current_branch: worktree.branch_name.clone(),
			worktree_root: worktree.path.clone(),
			project_worktree_root: worktree_root.clone(),
			canonical_repo_root: repo_root.clone(),
			authority: ManualAuthority::Manual,
			service_id: String::from("decodex"),
			workflow: sample_workflow(),
			github_token_env_var: String::from("GITHUB_TOKEN"),
			github_token: String::from("test-token"),
			repository: crate::github::RepositoryContext {
				owner: String::from("hack-ink"),
				name: String::from("decodex"),
				default_branch: String::from("main"),
				merge_commit_allowed: true,
			},
			prepared_closeout: None,
			pr_url: String::from("https://github.com/hack-ink/decodex/pull/65"),
			review_branch: worktree.branch_name.clone(),
		};

		manual::cleanup_manual_land_lane_checkout(&context)
			.expect("manual authority cleanup should remove the lane checkout");

		assert!(!worktree.path.exists(), "manual authority cleanup should remove the worktree");
		assert!(
			manual::run_git_capture(&repo_root, &["branch", "--list", &worktree.branch_name])
				.expect("local branch list should run")
				.is_empty(),
			"manual authority cleanup should delete the local lane branch"
		);
	}

	#[test]
	fn manual_land_manual_authority_recovery_accepts_merged_pr_after_cleanup() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let repo_root = init_git_checkout_with_origin(&temp_dir);
		let worktree_root = repo_root.join(".worktrees");
		let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);

		remove_test_lane_checkout(&repo_root, &merged_pr.worktree_path, &merged_pr.branch_name);

		let context = repo_root_manual_land_context(&repo_root, &worktree_root);
		let landing_state = merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);

		manual::ensure_already_merged_manual_land_recovery_ready(
			&context,
			&landing_state,
			&merged_pr.merge_commit,
		)
		.expect("already-merged manual land recovery should succeed after cleanup debt is gone");
	}

	#[test]
	fn manual_land_manual_authority_recovery_entrypoint_accepts_merged_pr() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let repo_root = init_git_checkout_with_origin(&temp_dir);
		let worktree_root = repo_root.join(".worktrees");
		let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);

		remove_test_lane_checkout(&repo_root, &merged_pr.worktree_path, &merged_pr.branch_name);

		let _path_guard = install_fake_landing_state_gh(
			&temp_dir,
			"MERGED",
			&merged_pr.branch_name,
			&merged_pr.head_oid,
			&merged_pr.merge_commit,
		);
		let context = repo_root_manual_land_context(&repo_root, &worktree_root);
		let request = ManualLandRequest {
			summary: String::from("land manual PR"),
			authority: None,
			manual_authority: true,
			pr_url: Some(context.pr_url.clone()),
			related: Vec::new(),
			breaking: false,
		};
		let outcome = manual::finalize_already_merged_manual_land_recovery(&context, &request)
			.expect("entrypoint should accept already-merged PR recovery")
			.expect("manual-authority recovery should run from repo-root main");

		assert_eq!(outcome.merge_commit, merged_pr.merge_commit);
	}

	#[test]
	fn manual_land_manual_authority_recovery_rejects_unmerged_pr() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let repo_root = init_git_checkout_with_origin(&temp_dir);
		let worktree_root = repo_root.join(".worktrees");
		let context = repo_root_manual_land_context(&repo_root, &worktree_root);
		let mut landing_state = sample_landing_state();

		landing_state.base_ref_name = String::from("main");
		landing_state.head_ref_name = String::from("x/decodex-manual-land-cleanup");

		let error = manual::ensure_already_merged_manual_land_recovery_ready(
			&context,
			&landing_state,
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		)
		.expect_err("unmerged PRs must not use default-branch recovery");

		assert!(error.to_string().contains("only accepts already-merged PRs"));
	}

	#[test]
	fn manual_land_manual_authority_recovery_rejects_incomplete_lane_cleanup() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let repo_root = init_git_checkout_with_origin(&temp_dir);
		let worktree_root = repo_root.join(".worktrees");
		let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);
		let context = repo_root_manual_land_context(&repo_root, &worktree_root);
		let landing_state = merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);
		let error = manual::ensure_already_merged_manual_land_recovery_ready(
			&context,
			&landing_state,
			&merged_pr.merge_commit,
		)
		.expect_err("recovery should reject when the landed lane branch remains");

		assert!(error.to_string().contains("landed lane cleanup to be complete"));
		assert!(error.to_string().contains(&merged_pr.branch_name));
	}

	#[test]
	fn manual_land_manual_authority_recovery_rejects_detached_lane_worktree() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let repo_root = init_git_checkout_with_origin(&temp_dir);
		let worktree_root = repo_root.join(".worktrees");
		let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);

		git_success(&merged_pr.worktree_path, &["checkout", "--detach"]);
		git_success(&repo_root, &["branch", "-D", &merged_pr.branch_name]);

		let context = repo_root_manual_land_context(&repo_root, &worktree_root);
		let landing_state = merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);
		let error = manual::ensure_already_merged_manual_land_recovery_ready(
			&context,
			&landing_state,
			&merged_pr.merge_commit,
		)
		.expect_err("recovery should reject a detached worktree at the landed PR head");

		assert!(error.to_string().contains("landed lane cleanup to be complete"));
		assert!(error.to_string().contains(&merged_pr.head_oid));
	}

	#[test]
	fn manual_land_manual_authority_recovery_rejects_remaining_cleanup_debt() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let repo_root = init_git_checkout_with_origin(&temp_dir);
		let worktree_root = repo_root.join(".worktrees");
		let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);

		remove_test_lane_checkout(&repo_root, &merged_pr.worktree_path, &merged_pr.branch_name);
		create_dirty_merged_worktree_debt(&repo_root, &worktree_root);

		let context = repo_root_manual_land_context(&repo_root, &worktree_root);
		let landing_state = merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);
		let error = manual::ensure_already_merged_manual_land_recovery_ready(
			&context,
			&landing_state,
			&merged_pr.merge_commit,
		)
		.expect_err("recovery should reject remaining merged worktree cleanup debt");

		assert!(error.to_string().contains("post-land worktree cleanup debt remains"));
		assert!(error.to_string().contains("XY-999"));
	}

	#[test]
	fn manual_land_issue_closeout_requires_managed_lane_checkout() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let repo_root = init_git_checkout(&temp_dir, "repo");
		let worktree_root = repo_root.join(".worktrees");

		fs::create_dir_all(&worktree_root).expect("worktree root should exist");

		let error = manual::ensure_manual_land_checkout_is_managed_lane(
			&repo_root,
			&worktree_root,
			"XY-225",
		)
		.expect_err("issue closeout should require a managed lane checkout");

		assert!(error.to_string().contains("must run from a managed lane"));
		assert!(error.to_string().contains("XY-225"));
	}

	#[test]
	fn manual_land_closeout_marker_roundtrips() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = init_git_checkout(&temp_dir, "repo");

		manual::write_manual_land_closeout_marker(
			&checkout,
			"https://github.com/hack-ink/decodex/pull/67",
			"deadbeef",
			"xy-225",
			r#"{"schema":"decodex/commit/1"}"#,
		)
		.expect("closeout marker should write");

		assert!(
			manual::manual_land_closeout_matches(
				&checkout,
				"https://github.com/hack-ink/decodex/pull/67",
				"deadbeef",
				"xy-225",
				r#"{"schema":"decodex/commit/1"}"#,
			)
			.expect("closeout marker should read"),
		);

		let marker = manual::read_manual_land_closeout_marker(&checkout)
			.expect("closeout marker should parse")
			.expect("closeout marker should exist");

		assert_eq!(marker.landed_change.as_deref(), Some(r#"{"schema":"decodex/commit/1"}"#));
		assert!(
			!checkout.join(".decodex/manual-land-closeout").exists(),
			"closeout marker should live under git admin state, not the working tree"
		);
	}

	#[test]
	fn manual_land_closeout_marker_rejects_mismatched_receipts() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = init_git_checkout(&temp_dir, "repo");

		manual::write_manual_land_closeout_marker(
			&checkout,
			"https://github.com/hack-ink/decodex/pull/67",
			"deadbeef",
			"xy-225",
			r#"{"schema":"decodex/commit/1"}"#,
		)
		.expect("closeout marker should write");

		assert!(
			!manual::manual_land_closeout_matches(
				&checkout,
				"https://github.com/hack-ink/decodex/pull/67",
				"cafebabe",
				"xy-225",
				r#"{"schema":"decodex/commit/1"}"#,
			)
			.expect("closeout marker should compare"),
		);
	}

	#[test]
	fn manual_land_handoff_lookup_prefers_current_branch_record() {
		let issue = sample_issue("issue-1", "XY-225", true, &["decodex:active:pubfi"]);
		let state_store = state::StateStore::open_in_memory().expect("state store should open");

		state_store
			.upsert_review_handoff_marker(
				"decodex",
				&issue.id,
				&state::ReviewHandoffMarker::new(
					String::from("run-current"),
					2,
					String::from("xy-225"),
					String::from("https://github.com/hack-ink/decodex/pull/67"),
					String::from("main"),
					String::from("xy-225"),
					String::from("deadbeef"),
				),
			)
			.expect("runtime handoff should persist");
		state_store
			.upsert_review_handoff_marker(
				"decodex",
				&issue.id,
				&state::ReviewHandoffMarker::new(
					String::from("run-other"),
					3,
					String::from("xy-225-next"),
					String::from("https://github.com/hack-ink/decodex/pull/99"),
					String::from("main"),
					String::from("xy-225-next"),
					String::from("cafebabe"),
				),
			)
			.expect("unrelated runtime handoff should persist");

		let handoff =
			manual::read_manual_land_handoff(&state_store, "decodex", &issue.id, "xy-225")
				.expect("manual land handoff should read")
				.expect("current branch handoff should be found");

		assert_eq!(handoff.branch_name(), "xy-225");
		assert_eq!(handoff.pr_url(), "https://github.com/hack-ink/decodex/pull/67");
	}

	#[test]
	fn resolve_manual_config_path_uses_registered_project_for_linked_worktree() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let _home_guard = TestEnvVarGuard::set(
			"HOME",
			temp_dir.path().to_str().expect("temp dir path should be utf-8"),
		);
		let repo_root = temp_dir.path().join("target-repo");
		let worktree_root = repo_root.join(".worktrees");
		let config_dir = temp_dir.path().join(".codex/decodex/projects/pubfi");
		let config_path = config_dir.join("project.toml");

		fs::create_dir_all(&repo_root).expect("repo root should exist");
		fs::create_dir_all(&worktree_root).expect("worktree root should exist");
		fs::create_dir_all(&config_dir).expect("config dir should exist");

		assert!(
			Command::new("git")
				.args(["init", "-b", "main"])
				.current_dir(temp_dir.path())
				.arg(&repo_root)
				.status()
				.expect("git init should run")
				.success()
		);
		assert!(
			Command::new("git")
				.args(["config", "user.name", "Decodex Tests"])
				.current_dir(&repo_root)
				.status()
				.expect("git config should run")
				.success()
		);
		assert!(
			Command::new("git")
				.args(["config", "user.email", "decodex-tests@example.com"])
				.current_dir(&repo_root)
				.status()
				.expect("git config should run")
				.success()
		);
		assert!(
			Command::new("git")
				.args(["config", "commit.gpgsign", "false"])
				.current_dir(&repo_root)
				.status()
				.expect("git config should run")
				.success()
		);

		fs::write(
			&config_path,
			format!(
				r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "PATH"

				[paths]
				repo_root = "{}"
				"#,
				repo_root.display()
			),
		)
		.expect("central project config should write");
		fs::write(config_dir.join("WORKFLOW.md"), "test workflow\n")
			.expect("central workflow should write");
		fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");

		git_success(&repo_root, &["add", "README.md"]);
		git_success(&repo_root, &["commit", "-m", "bootstrap repo"]);

		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let worktree = manager.ensure_worktree("XY-225", false).expect("worktree should create");
		let state_store = runtime::open_runtime_store().expect("state store should open");
		let canonical_config =
			fs::canonicalize(&config_path).expect("central config should canonicalize");

		runtime::register_project_config(&state_store, &config_path, true)
			.expect("central config should register");

		assert_eq!(
			manual::resolve_manual_config_path(None, &worktree.path)
				.expect("registered config path should resolve"),
			canonical_config
		);
	}

	#[test]
	fn ensure_cli_repo_context_rejects_foreign_config_repo_root() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let current_repo = init_git_checkout(&temp_dir, "current-repo");
		let foreign_repo = init_git_checkout(&temp_dir, "foreign-repo");
		let config_path = foreign_repo.join("project.toml");

		fs::write(
			&config_path,
			r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "PATH"

				[paths]
				repo_root = "."
				"#,
		)
		.expect("foreign config should write");

		let config = ServiceConfig::from_path(&config_path).expect("config should parse");
		let canonical_repo_root =
			fs::canonicalize(&current_repo).expect("current repo root should canonicalize");
		let error = manual::ensure_cli_repo_context(&current_repo, &config, &canonical_repo_root)
			.expect_err("foreign config repo root should be rejected");

		assert!(error.to_string().contains("does not match loaded config repo root"));
		assert!(error.to_string().contains(&foreign_repo.display().to_string()));
	}

	fn sample_landing_state() -> PullRequestLandingState {
		PullRequestLandingState {
			url: String::from("https://github.com/hack-ink/decodex/pull/64"),
			state: String::from("OPEN"),
			is_draft: false,
			review_decision: Some(String::from("APPROVED")),
			base_ref_name: String::from("release/1.x"),
			pending_review_requests: 0,
			mergeable: String::from("MERGEABLE"),
			merge_state_status: String::from("CLEAN"),
			head_ref_name: String::from("XY-225"),
			head_ref_oid: String::from("deadbeef"),
			status_check_rollup_state: Some(String::from("SUCCESS")),
			unresolved_review_threads: 0,
		}
	}

	fn sample_issue(
		id: &str,
		identifier: &str,
		labels_complete: bool,
		labels: &[&str],
	) -> TrackerIssue {
		TrackerIssue {
			id: id.to_owned(),
			identifier: identifier.to_owned(),
			#[cfg(test)]
			project_slug: None,
			title: String::from("Sample issue"),
			description: String::from(""),
			priority: None,
			created_at: String::from("2026-04-13T00:00:00Z"),
			updated_at: String::from("2026-04-13T00:00:00Z"),
			state: TrackerState { id: String::from("state-1"), name: String::from("In Review") },
			team: TrackerTeam {
				id: String::from("team-1"),
				name: String::from("Core"),
				states: vec![TrackerState {
					id: String::from("state-1"),
					name: String::from("In Review"),
				}],
				labels: labels
					.iter()
					.enumerate()
					.map(|(index, label)| TrackerLabel {
						id: format!("team-label-{index}"),
						name: (*label).to_owned(),
					})
					.collect(),
			},
			labels_complete,
			labels: labels
				.iter()
				.enumerate()
				.map(|(index, label)| TrackerLabel {
					id: format!("issue-label-{index}"),
					name: (*label).to_owned(),
				})
				.collect(),
			blockers: Vec::new(),
		}
	}
}
