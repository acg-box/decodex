use std::{
	env, fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process::{Command, Stdio},
	thread,
};

use color_eyre::{Report, eyre::WrapErr};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	commit_message::{self, MANUAL_AUTHORITY},
	config::{self, ServiceConfig},
	default_branch_sync,
	git_credentials::GitCredentialSource,
	github::{self, RepositoryContext},
	orchestrator,
	prelude::{Result, eyre},
	pull_request::{self, LandingGateMode, PullRequestLandingGateView, PullRequestLandingState},
	runtime,
	state::{self, ReviewHandoffMarker, StateStore, WorktreeMapping},
	tracker::{
		self, IssueTracker, TrackerIssue,
		linear::LinearClient,
		privacy_classifier::{
			ConfiguredPublicProjectionPrivacyClassifier, PublicProjectionPrivacyClassifier,
		},
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
	workflow::WorkflowDocument,
	worktree::{self, WorktreeManager},
};

const MANUAL_LAND_CLOSEOUT_MARKER_GIT_PATH: &str = "decodex/manual-land-closeout";
const MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT: std::time::Duration =
	std::time::Duration::from_secs(15 * 60);
const MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS: usize = 4;
const MANUAL_LAND_MERGEABILITY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

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
	workflow: Option<WorkflowDocument>,
	github_token_env_var: String,
	github_token: String,
	github_command_path: Option<PathBuf>,
	repository: RepositoryContext,
	prepared_closeout: Option<PreparedCloseout>,
	review_handoff: Option<ReviewHandoffMarker>,
	pr_url: String,
	review_branch: String,
	public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier,
}
impl ManualLandContext {
	fn default_branch_git_credentials(&self) -> GitCredentialSource<'_> {
		GitCredentialSource::new(&self.github_token_env_var, &self.github_token)
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

struct ManualLandLedgerContext<'a> {
	service_id: &'a str,
	issue: &'a TrackerIssue,
	state_store: &'a StateStore,
	handoff: &'a ReviewHandoffMarker,
	pr_url: &'a str,
	merge_commit: &'a str,
	branch_name: &'a str,
	worktree_path: &'a str,
	completed_state: &'a str,
	default_branch: &'a str,
	privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct ManualCommitActiveLaneBlocker {
	issue_id: String,
	branch_name: String,
	worktree_path: PathBuf,
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

	ensure_manual_commit_not_claimed_by_active_lane(config_path, &cwd, &worktree_root)?;

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
	if context.prepared_closeout.is_some() && context.review_handoff.is_none() {
		eyre::bail!(
			"`decodex land` issue closeout requires a retained review handoff marker so it can write deterministic Linear execution ledger events. Run `decodex recover review-handoff rebind` for `{}` before retrying.",
			context.current_branch
		);
	}

	let default_branch = context.repository.default_branch.clone();
	let landing_state = inspect_pull_request_landing_state_for_manual_land(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		context.github_command_path.as_deref(),
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

fn inspect_pull_request_landing_state_for_manual_land(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<PullRequestLandingState> {
	let mut last_landing_state = None;

	for attempt in 1..=MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS {
		let landing_state =
			github::inspect_pull_request_landing_state(cwd, pr_url, github_token, gh_command_path)?;

		if landing_state.state == "MERGED"
			|| !pull_request::mergeability_unknown(landing_state.gate_view())
		{
			return Ok(landing_state);
		}

		last_landing_state = Some(landing_state);

		if attempt < MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS {
			tracing::info!(
				pr_url = %pr_url,
				attempt,
				mergeable = "UNKNOWN",
				merge_state_status = "UNKNOWN",
				"Pull request mergeability is unresolved; waiting for GitHub to recompute before validating manual land gates."
			);

			thread::sleep(MANUAL_LAND_MERGEABILITY_RETRY_DELAY);
		}
	}

	last_landing_state
		.ok_or_else(|| eyre::eyre!("Pull request `{pr_url}` landing state was unavailable."))
}

fn prepare_manual_land_context(
	config_path: Option<&Path>,
	request: &ManualLandRequest,
) -> Result<ManualLandContext> {
	let cwd = env::current_dir()?;
	let worktree_root = current_worktree_root(&cwd)?;
	let current_branch = current_branch_name(&cwd)?;

	if request.manual_authority && config_path.is_none() {
		return prepare_unregistered_manual_land_context(
			cwd,
			worktree_root,
			current_branch,
			request,
		);
	}

	let resolved_config_path = resolve_manual_config_path(config_path, &cwd)?;

	prepare_configured_manual_land_context(
		cwd,
		worktree_root,
		current_branch,
		&resolved_config_path,
		request,
	)
}

fn prepare_configured_manual_land_context(
	cwd: PathBuf,
	worktree_root: PathBuf,
	current_branch: String,
	resolved_config_path: &Path,
	request: &ManualLandRequest,
) -> Result<ManualLandContext> {
	let config = ServiceConfig::from_path(resolved_config_path)?;
	let canonical_repo_root = config::canonical_repo_root_for_checkout(&cwd)?
		.unwrap_or_else(|| config.repo_root().to_path_buf());

	ensure_cli_repo_context(&cwd, &config, &canonical_repo_root)?;

	let authority = resolve_land_authority(
		Some(resolved_config_path),
		request.authority.as_deref(),
		request.manual_authority,
		&worktree_root,
	)?;
	let github_token = config.github().resolve_token()?;
	let github_command_path = config.github().command_path().map(Path::to_path_buf);
	let repository = github::inspect_repository_context(
		&canonical_repo_root,
		&github_token,
		github_command_path.as_deref(),
	)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let public_projection_privacy_classifier =
		ConfiguredPublicProjectionPrivacyClassifier::from_config(config.privacy_classifier())?;
	let prepared_closeout =
		prepare_manual_land_closeout(&config, &canonical_repo_root, workflow.clone(), &authority)?;
	let handoff = match prepared_closeout.as_ref() {
		Some(prepared_closeout) => {
			let state_store = runtime::open_runtime_store()?;

			runtime::register_project_config(&state_store, resolved_config_path, true)?;

			read_manual_land_handoff(
				&state_store,
				config.service_id(),
				&prepared_closeout.issue.id,
				&current_branch,
			)?
		},
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
		workflow: Some(workflow),
		github_token_env_var: config.github().token_env_var().to_owned(),
		github_token,
		github_command_path,
		repository,
		prepared_closeout,
		review_handoff: handoff,
		pr_url,
		review_branch,
		public_projection_privacy_classifier,
	})
}

fn prepare_unregistered_manual_land_context(
	cwd: PathBuf,
	worktree_root: PathBuf,
	current_branch: String,
	request: &ManualLandRequest,
) -> Result<ManualLandContext> {
	let authority = resolve_land_authority(
		None,
		request.authority.as_deref(),
		request.manual_authority,
		&worktree_root,
	)?;
	let canonical_repo_root =
		config::canonical_repo_root_for_checkout(&cwd)?.unwrap_or_else(|| worktree_root.clone());
	let (github_token_env_var, github_token) = resolve_unregistered_github_token(&cwd, None)?;
	let repository = github::inspect_repository_context(&canonical_repo_root, &github_token, None)?;
	let pr_url = resolve_pr_url(request.pr_url.as_deref(), None, authority.is_manual())?;
	let project_worktree_root =
		infer_unregistered_manual_land_worktree_root(&canonical_repo_root, &worktree_root);

	Ok(ManualLandContext {
		cwd,
		current_branch: current_branch.clone(),
		worktree_root,
		project_worktree_root,
		canonical_repo_root,
		authority,
		service_id: repository.name.clone(),
		workflow: None,
		github_token_env_var,
		github_token,
		github_command_path: None,
		repository,
		prepared_closeout: None,
		review_handoff: None,
		pr_url,
		review_branch: current_branch,
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	})
}

fn resolve_unregistered_github_token(
	cwd: &Path,
	gh_command_path: Option<&Path>,
) -> Result<(String, String)> {
	for env_var in ["GH_TOKEN", "GITHUB_TOKEN"] {
		if let Some(token) = nonempty_env_var(env_var) {
			return Ok((env_var.to_owned(), token));
		}
	}

	let mut command = github::gh_command_with_config(gh_command_path);

	command.args(["auth", "token"]);
	command.current_dir(cwd);
	command
		.env("GH_PROMPT_DISABLED", "1")
		.env("GIT_TERMINAL_PROMPT", "0")
		.env("GCM_INTERACTIVE", "never");

	let output = command.output().wrap_err("Failed to run `gh auth token`.")?;

	if output.status.success() {
		let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();

		if !token.is_empty() {
			return Ok((String::from("GH_TOKEN"), token));
		}
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let detail = stderr.trim();

	eyre::bail!(
		"`decodex land --manual-authority --pr` needs GitHub credentials when no Decodex project config is provided. Set `GH_TOKEN`/`GITHUB_TOKEN`, authenticate `gh auth token`, or pass `--config <PROJECT_DIR>`.{}",
		if detail.is_empty() {
			String::new()
		} else {
			format!(" `gh auth token` failed: {detail}")
		}
	);
}

fn nonempty_env_var(name: &str) -> Option<String> {
	env::var(name).ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
}

fn infer_unregistered_manual_land_worktree_root(
	canonical_repo_root: &Path,
	worktree_root: &Path,
) -> PathBuf {
	let conventional_worktree_root = canonical_repo_root.join(".worktrees");

	if paths_match_for_manual_commit_guard(worktree_root, canonical_repo_root)
		|| paths_match_for_manual_land_root(worktree_root, &conventional_worktree_root)
	{
		return conventional_worktree_root;
	}

	worktree_root.parent().map_or_else(|| worktree_root.to_path_buf(), Path::to_path_buf)
}

fn paths_match_for_manual_land_root(path: &Path, root: &Path) -> bool {
	let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
	let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

	path.starts_with(&root) && path != root
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
				context.github_command_path.as_deref(),
			) {
				if matches!(
					github::pull_request_is_merged_at_head(
						&context.canonical_repo_root,
						&context.pr_url,
						current_head,
						&context.github_token,
						context.github_command_path.as_deref(),
					),
					Ok(true)
				) {
					return github::wait_for_pull_request_merge_commit(
						&context.canonical_repo_root,
						&context.pr_url,
						&context.github_token,
						MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
						context.github_command_path.as_deref(),
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
		context.github_command_path.as_deref(),
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
		context.github_command_path.as_deref(),
	)
}

fn finalize_land_closeout(
	context: &ManualLandContext,
	merge_commit: &str,
	default_branch: &str,
	landed_change_record: &str,
) -> Result<()> {
	let state_store = if context.prepared_closeout.is_some() {
		Some(runtime::open_runtime_store()?)
	} else {
		None
	};
	let worktree_path_for_event = manual_land_relative_worktree_path(context);

	if let Some(prepared_closeout) = context.prepared_closeout.as_ref() {
		let state_store = state_store
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Manual closeout state store was not opened."))?;
		let handoff = context.review_handoff.as_ref().ok_or_else(|| {
			eyre::eyre!("`decodex land` issue closeout requires a retained review handoff marker.")
		})?;
		let ledger = ManualLandLedgerContext {
			service_id: &prepared_closeout.service_id,
			issue: &prepared_closeout.issue,
			state_store,
			handoff,
			pr_url: &context.pr_url,
			merge_commit,
			branch_name: &context.current_branch,
			worktree_path: &worktree_path_for_event,
			completed_state: &prepared_closeout.completed_state,
			default_branch,
			privacy_classifier: &context.public_projection_privacy_classifier,
		};

		apply_closeout(
			&context.cwd,
			&prepared_closeout.tracker,
			&prepared_closeout.completed_state,
			&ledger,
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
		let state_store = state_store
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Manual closeout state store was not opened."))?;
		let handoff = context.review_handoff.as_ref().ok_or_else(|| {
			eyre::eyre!("`decodex land` issue cleanup requires a retained review handoff marker.")
		})?;

		clear_manual_closeout_runtime_state(
			state_store,
			&prepared_closeout.issue.id,
			handoff.run_id(),
		)?;
		clear_manual_closeout_issue_scope(
			&prepared_closeout.tracker,
			&prepared_closeout.issue,
			&prepared_closeout.service_id,
			&prepared_closeout.needs_attention_label,
		)?;

		let ledger = ManualLandLedgerContext {
			service_id: &prepared_closeout.service_id,
			issue: &prepared_closeout.issue,
			state_store,
			handoff,
			pr_url: &context.pr_url,
			merge_commit,
			branch_name: &context.current_branch,
			worktree_path: &worktree_path_for_event,
			completed_state: &prepared_closeout.completed_state,
			default_branch,
			privacy_classifier: &context.public_projection_privacy_classifier,
		};

		write_manual_land_cleanup_complete_event(&prepared_closeout.tracker, &ledger)?;
	}

	Ok(())
}

fn manual_land_relative_worktree_path(context: &ManualLandContext) -> String {
	if let Ok(relative_path) = context.worktree_root.strip_prefix(&context.canonical_repo_root) {
		if relative_path.as_os_str().is_empty() {
			return String::from(".");
		}

		return relative_path.display().to_string();
	}
	if let Some(root_name) = context.project_worktree_root.file_name()
		&& let Ok(relative_path) =
			context.worktree_root.strip_prefix(&context.project_worktree_root)
	{
		return Path::new(root_name).join(relative_path).display().to_string();
	}

	context.worktree_root.file_name().map_or_else(
		|| context.worktree_root.display().to_string(),
		|path| path.to_string_lossy().into_owned(),
	)
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
		context.github_command_path.as_deref(),
	)?;
	orchestrator::detach_worktree_head_from_branch_if_checked_out(
		&context.worktree_root,
		&context.current_branch,
	)?;
	orchestrator::delete_local_branch_if_present(
		&context.canonical_repo_root,
		&context.current_branch,
	)?;

	if let Some(workflow) = context.workflow.as_ref() {
		worktree_manager.remove_worktree_path_with_hooks(
			manual_land_cleanup_identifier(&context.authority, &context.current_branch),
			&context.current_branch,
			&context.worktree_root,
			workflow.frontmatter().execution().workspace_hooks(),
		)?;
	} else {
		worktree_manager.remove_worktree_path(&context.worktree_root)?;
	}

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
		"Decodex project config is required for this command. Pass this command's `--config <PROJECT_DIR>` or register one with `decodex project add <PROJECT_DIR>`."
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

fn ensure_manual_commit_not_claimed_by_active_lane(
	config_path: Option<&Path>,
	cwd: &Path,
	worktree_root: &Path,
) -> Result<()> {
	let Some(blocker) =
		manual_commit_active_lane_blocker_from_runtime(config_path, cwd, worktree_root)?
	else {
		return Ok(());
	};

	eyre::bail!(
		"`decodex commit` refused to write inside active Decodex-owned lane worktree `{}` on branch `{}` for issue `{}` because the issue has a live runtime claim. Wait for the lane to finish, steer or interrupt the owning run, or clear retained ownership before using the manual commit helper.",
		blocker.worktree_path.display(),
		blocker.branch_name,
		blocker.issue_id,
	)
}

fn manual_commit_active_lane_blocker_from_runtime(
	config_path: Option<&Path>,
	cwd: &Path,
	worktree_root: &Path,
) -> Result<Option<ManualCommitActiveLaneBlocker>> {
	let state_store = match runtime::open_runtime_store() {
		Ok(state_store) => state_store,
		Err(_error) if config_path.is_none() => return Ok(None),
		Err(error) => return Err(error),
	};
	let Some(config_path) = manual_commit_project_config_path(config_path, cwd, &state_store)?
	else {
		return Ok(None);
	};
	let config = ServiceConfig::from_path(&config_path)?;

	if !manual_commit_checkout_matches_project(worktree_root, &config)? {
		return Ok(None);
	}

	let current_branch = current_branch_name_if_attached(cwd)?;

	manual_commit_active_lane_blocker(
		&state_store,
		config.service_id(),
		worktree_root,
		current_branch.as_deref(),
	)
}

fn manual_commit_project_config_path(
	config_path: Option<&Path>,
	cwd: &Path,
	state_store: &StateStore,
) -> Result<Option<PathBuf>> {
	if let Some(config_path) = config_path {
		return Ok(Some(ServiceConfig::resolve_project_config_path(config_path)?));
	}

	runtime::registered_config_path_for_cwd(state_store, cwd)
}

fn manual_commit_checkout_matches_project(
	worktree_root: &Path,
	config: &ServiceConfig,
) -> Result<bool> {
	Ok(worktree_root == config.repo_root()
		|| config::checkouts_share_repository(worktree_root, config.repo_root())?)
}

fn manual_commit_active_lane_blocker(
	state_store: &StateStore,
	service_id: &str,
	worktree_root: &Path,
	current_branch: Option<&str>,
) -> Result<Option<ManualCommitActiveLaneBlocker>> {
	for mapping in state_store.list_worktrees(service_id)? {
		if !manual_commit_matches_worktree_mapping(&mapping, worktree_root, current_branch) {
			continue;
		}
		if !state_store.issue_has_active_shared_claim(service_id, mapping.issue_id())? {
			continue;
		}

		return Ok(Some(ManualCommitActiveLaneBlocker {
			issue_id: mapping.issue_id().to_owned(),
			branch_name: mapping.branch_name().to_owned(),
			worktree_path: mapping.worktree_path().to_path_buf(),
		}));
	}

	Ok(None)
}

fn manual_commit_matches_worktree_mapping(
	mapping: &WorktreeMapping,
	worktree_root: &Path,
	current_branch: Option<&str>,
) -> bool {
	paths_match_for_manual_commit_guard(worktree_root, mapping.worktree_path())
		&& current_branch.is_none_or(|branch| branch == mapping.branch_name())
}

fn paths_match_for_manual_commit_guard(left: &Path, right: &Path) -> bool {
	let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
	let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());

	left == right
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

fn current_branch_name_if_attached(cwd: &Path) -> Result<Option<String>> {
	let branch = run_git_capture(cwd, &["branch", "--show-current"])?;

	Ok((!branch.is_empty()).then_some(branch))
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

	!line.is_empty() && !state::is_untracked_decodex_runtime_artifact_status_line(line)
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

	let decision = pull_request::classify_landing_gate(gate_view, LandingGateMode::ManualLand);

	match decision {
		pull_request::LandingGateDecision::Satisfied => {
			debug_assert!(pull_request::manual_landing_gates_satisfied(gate_view));

			Ok(LandExecutionMode::MergeAndCloseout)
		},
		pull_request::LandingGateDecision::CloseoutOnly => Ok(LandExecutionMode::CloseoutOnly),
		decision => manual_landing_gate_error(decision, gate_view, pr_url),
	}
}

fn manual_landing_gate_error(
	decision: pull_request::LandingGateDecision,
	gate_view: PullRequestLandingGateView<'_>,
	pr_url: &str,
) -> Result<LandExecutionMode> {
	match decision {
		pull_request::LandingGateDecision::Satisfied => Ok(LandExecutionMode::MergeAndCloseout),
		pull_request::LandingGateDecision::CloseoutOnly => Ok(LandExecutionMode::CloseoutOnly),
		pull_request::LandingGateDecision::Block("pull_request_not_open") => {
			eyre::bail!("Pull request `{pr_url}` is `{}` and cannot be landed.", gate_view.state)
		},
		pull_request::LandingGateDecision::Block("pull_request_is_draft") => {
			eyre::bail!("Pull request `{pr_url}` is still draft.")
		},
		pull_request::LandingGateDecision::Wait("pending_review_requests") => {
			eyre::bail!(
				"Pull request `{pr_url}` still has {} pending review request(s).",
				gate_view.pending_review_requests
			)
		},
		pull_request::LandingGateDecision::Repair("unresolved_review_threads") => {
			eyre::bail!(
				"Pull request `{pr_url}` still has {} unresolved review thread(s).",
				gate_view.unresolved_review_threads
			)
		},
		pull_request::LandingGateDecision::Repair("review_changes_requested") => {
			eyre::bail!("Pull request `{pr_url}` still has active change requests.")
		},
		pull_request::LandingGateDecision::Repair(reason)
			if matches!(
				reason,
				"pull_request_merge_conflict" | "pull_request_branch_behind_base"
			) =>
		{
			eyre::bail!("Pull request `{pr_url}` requires review repair: {reason}.")
		},
		pull_request::LandingGateDecision::Repair("required_checks_failed") => {
			eyre::bail!("Pull request `{pr_url}` has failed required checks that need repair.")
		},
		pull_request::LandingGateDecision::Wait("checks_waiting") => {
			let check_state = gate_view.status_check_rollup_state.unwrap_or("unknown");

			eyre::bail!(
				"Pull request `{pr_url}` is still waiting on checks: statusCheckRollup=`{check_state}`."
			)
		},
		pull_request::LandingGateDecision::Wait("mergeability_unknown") => {
			eyre::bail!(
				"Pull request `{pr_url}` mergeability is still unknown after retry; wait for GitHub to recompute mergeability and retry `decodex land`."
			)
		},
		pull_request::LandingGateDecision::Block("merge_state_not_ready") => {
			eyre::bail!(
				"Pull request `{pr_url}` is not ready to land: mergeStateStatus=`{}`.",
				gate_view.merge_state_status
			)
		},
		pull_request::LandingGateDecision::Block("not_mergeable") => {
			eyre::bail!(
				"Pull request `{pr_url}` is not mergeable: mergeable=`{}`.",
				gate_view.mergeable
			)
		},
		pull_request::LandingGateDecision::Wait("checks_non_green") => {
			let check_state = gate_view.status_check_rollup_state.unwrap_or("unknown");

			eyre::bail!(
				"Pull request `{pr_url}` still has non-green checks: statusCheckRollup=`{check_state}`."
			)
		},
		pull_request::LandingGateDecision::Wait(reason)
		| pull_request::LandingGateDecision::Repair(reason)
		| pull_request::LandingGateDecision::Block(reason) => {
			eyre::bail!("Pull request `{pr_url}` is not ready to land: {reason}.")
		},
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
		context.github_command_path.as_deref(),
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
		context.github_command_path.as_deref(),
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

fn apply_closeout<T>(
	checkout_root: &Path,
	tracker: &T,
	completed_state: &str,
	ledger: &ManualLandLedgerContext<'_>,
	landed_change_record: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if ledger.issue.state.name != completed_state {
		let state_id = ledger.issue.state_id_for_name(completed_state).ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` does not expose tracker state `{}` on its team.",
				ledger.issue.identifier,
				completed_state
			)
		})?;

		tracker.update_issue_state(ledger.issue.id.as_str(), state_id)?;
	}
	if !manual_land_closeout_matches(
		checkout_root,
		ledger.pr_url,
		ledger.merge_commit,
		ledger.branch_name,
		landed_change_record,
	)? {
		tracker::create_public_comment(
			tracker,
			ledger.issue.id.as_str(),
			format!(
				"decodex land completed\n\n- pr_url: `{}`\n- merge_commit: `{}`\n- branch: `{}`\n- landed_change: `{landed_change_record}`",
				ledger.pr_url, ledger.merge_commit, ledger.branch_name
			)
			.as_str(),
		)?;

		write_manual_land_closeout_marker(
			checkout_root,
			ledger.pr_url,
			ledger.merge_commit,
			ledger.branch_name,
			landed_change_record,
		)?;
	}

	write_manual_land_landed_and_closeout_events(tracker, ledger)?;
	succeed_manual_land_handoff_attempt(
		ledger.state_store,
		&ledger.issue.id,
		ledger.handoff.run_id(),
	)?;

	Ok(())
}

fn write_manual_land_landed_and_closeout_events<T>(
	tracker: &T,
	ledger: &ManualLandLedgerContext<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let landed = manual_land_landed_event(ledger);
	let closeout = manual_land_closeout_event(ledger);

	write_manual_land_lifecycle_event(tracker, ledger, &landed)?;

	write_manual_land_lifecycle_event(tracker, ledger, &closeout)
}

fn write_manual_land_cleanup_complete_event<T>(
	tracker: &T,
	ledger: &ManualLandLedgerContext<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let cleanup_complete = manual_land_cleanup_complete_event(ledger);

	write_manual_land_lifecycle_event(tracker, ledger, &cleanup_complete)
}

fn write_manual_land_lifecycle_event<T>(
	tracker: &T,
	ledger: &ManualLandLedgerContext<'_>,
	record: &LinearExecutionEventRecord,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let retry_budget_attempt_count =
		ledger.state_store.retry_budget_attempt_count(&ledger.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body =
		records::render_linear_execution_event_comment_body(record, retry_budget_attempt_count);
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, record, ledger.privacy_classifier)?;

	if ledger.state_store.record_linear_execution_event(&projection.record)?
		&& let Err(error) =
			tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
				tracker,
				&ledger.issue.id,
				&projection,
			) {
		ledger.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(())
}

fn manual_land_landed_event(ledger: &ManualLandLedgerContext<'_>) -> LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[
		ledger.pr_url,
		ledger.handoff.pr_head_oid(),
		ledger.merge_commit,
		"manual_land_landed",
	]);
	let mut record = LinearExecutionEventRecord::new(
		manual_land_lifecycle_identity(ledger),
		"landed",
		manual_land_ordered_event_timestamp(-2),
		&anchor,
	);

	record.branch = Some(ledger.branch_name.to_owned());
	record.pr_url = Some(ledger.pr_url.to_owned());
	record.pr_head_sha = Some(ledger.handoff.pr_head_oid().to_owned());
	record.pr_base_ref =
		Some(ledger.handoff.target_base_ref_name().unwrap_or(ledger.default_branch).to_owned());
	record.commit_sha = Some(ledger.merge_commit.to_owned());
	record.summary =
		Some(format!("Manual land merged {} for {}.", ledger.pr_url, ledger.issue.identifier));

	record
}

fn manual_land_closeout_event(ledger: &ManualLandLedgerContext<'_>) -> LinearExecutionEventRecord {
	let anchor =
		records::stable_event_anchor(&[ledger.pr_url, ledger.merge_commit, "manual_land_closeout"]);
	let mut record = LinearExecutionEventRecord::new(
		manual_land_lifecycle_identity(ledger),
		"closeout",
		manual_land_ordered_event_timestamp(-1),
		&anchor,
	);

	record.branch = Some(ledger.branch_name.to_owned());
	record.worktree_path = Some(ledger.worktree_path.to_owned());
	record.pr_url = Some(ledger.pr_url.to_owned());
	record.commit_sha = Some(ledger.merge_commit.to_owned());
	record.validation_result = Some(String::from("passed"));
	record.target_state = Some(ledger.completed_state.to_owned());
	record.summary = Some(format!(
		"Manual land closed out {} after merge {}.",
		ledger.issue.identifier, ledger.merge_commit
	));

	record
}

fn manual_land_cleanup_complete_event(
	ledger: &ManualLandLedgerContext<'_>,
) -> LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[
		ledger.branch_name,
		ledger.merge_commit,
		"manual_land_cleanup_complete",
	]);
	let mut record = LinearExecutionEventRecord::new(
		manual_land_lifecycle_identity(ledger),
		"cleanup_complete",
		manual_land_ordered_event_timestamp(0),
		&anchor,
	);

	record.branch = Some(ledger.branch_name.to_owned());
	record.worktree_path = Some(ledger.worktree_path.to_owned());
	record.cleanup_status = Some(String::from("completed"));
	record.summary = Some(String::from("Manual land cleaned up the retained lane."));
	record.pr_url = Some(ledger.pr_url.to_owned());
	record.commit_sha = Some(ledger.merge_commit.to_owned());

	record
}

fn manual_land_lifecycle_identity<'a>(
	ledger: &'a ManualLandLedgerContext<'_>,
) -> LinearExecutionEventIdentity<'a> {
	LinearExecutionEventIdentity {
		service_id: ledger.service_id,
		issue_id: &ledger.issue.id,
		issue_identifier: &ledger.issue.identifier,
		run_id: ledger.handoff.run_id(),
		attempt_number: ledger.handoff.attempt_number(),
	}
}

fn manual_land_ordered_event_timestamp(offset_seconds: i64) -> String {
	(OffsetDateTime::now_utc() + time::Duration::seconds(offset_seconds))
		.format(&Rfc3339)
		.expect("timestamp formatting should succeed")
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

fn clear_manual_closeout_runtime_state(
	state_store: &StateStore,
	issue_id: &str,
	handoff_run_id: &str,
) -> Result<()> {
	state_store.succeed_running_run_attempts_for_issue(issue_id).wrap_err_with(|| {
		format!("Failed to finalize running runtime attempts for issue `{issue_id}`.")
	})?;

	succeed_manual_land_handoff_attempt(state_store, issue_id, handoff_run_id)?;

	state_store
		.clear_lease(issue_id)
		.wrap_err_with(|| format!("Failed to clear runtime lease for issue `{issue_id}`."))?;
	state_store.clear_worktree(issue_id).wrap_err_with(|| {
		format!("Failed to clear runtime worktree state for issue `{issue_id}`.")
	})?;

	Ok(())
}

fn succeed_manual_land_handoff_attempt(
	state_store: &StateStore,
	issue_id: &str,
	handoff_run_id: &str,
) -> Result<()> {
	let Some(attempt) = state_store.run_attempt(handoff_run_id)? else {
		return Ok(());
	};

	if attempt.issue_id() != issue_id {
		eyre::bail!(
			"Manual land handoff run `{handoff_run_id}` belongs to issue `{}`, not `{issue_id}`.",
			attempt.issue_id()
		);
	}
	if attempt.status() != "succeeded" {
		state_store.update_run_status(handoff_run_id, "succeeded")?;
	}

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

#[cfg(test)] mod tests;
