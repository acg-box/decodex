mod authority;
mod closeout;
mod commit_guard;
mod context;
mod git;
mod landing;
mod recovery;

use std::{
	env,
	path::{Path, PathBuf},
};

use self::{
	authority::resolve_land_authority,
	closeout::{ensure_manual_land_left_no_merged_worktree_cleanup_debt, prepare_closeout},
	git::{
		current_branch_name, current_branch_name_if_attached, ensure_clean_worktree,
		paths_match_for_manual_commit_guard, run_git_capture,
	},
};
#[cfg(test)]
use self::{
	authority::{infer_issue_identifier_from_worktree_root, looks_like_issue_identifier},
	closeout::{
		apply_closeout, cleanup_manual_land_lane_checkout, clear_manual_closeout_issue_scope,
		clear_manual_closeout_runtime_state, ensure_manual_closeout_issue_scope,
		manual_land_closeout_matches, read_manual_land_closeout_marker,
		write_manual_land_cleanup_complete_event, write_manual_land_closeout_marker,
	},
	commit_guard::manual_commit_active_lane_blocker,
	context::{
		ensure_cli_repo_context, prepare_configured_manual_land_context,
		prepare_unregistered_manual_land_context, read_manual_land_handoff,
		resolve_manual_config_path, resolve_pr_url,
	},
	recovery::ensure_already_merged_manual_land_recovery_ready,
};
#[cfg(test)] use crate::pull_request::PullRequestLandingState;
use crate::{
	commit_message::{self, MANUAL_AUTHORITY},
	default_branch_sync,
	git_credentials::GitCredentialSource,
	github::{self, RepositoryContext},
	prelude::{Result, eyre},
	state::{ReviewHandoffMarker, StateStore},
	tracker::{
		TrackerIssue,
		linear::LinearClient,
		privacy_classifier::{
			ConfiguredPublicProjectionPrivacyClassifier, PublicProjectionPrivacyClassifier,
		},
	},
	workflow::WorkflowDocument,
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
	let worktree_root = self::git::current_worktree_root(&cwd)?;
	let authority = self::authority::resolve_authority(
		config_path,
		request.authority.as_deref(),
		request.manual_authority,
		&worktree_root,
	)?;

	self::commit_guard::ensure_manual_commit_not_claimed_by_active_lane(
		config_path,
		&cwd,
		&worktree_root,
	)?;

	let message = commit_message::build_commit_message(
		&request.summary,
		authority.commit_message_value(),
		&request.related,
		request.breaking,
	)?;

	self::git::run_git_checked_with_stdio(&cwd, &["commit", "-S", "-m", message.as_str()])
}

pub(crate) fn run_land(config_path: Option<&Path>, request: &ManualLandRequest) -> Result<()> {
	let context = self::context::prepare_manual_land_context(config_path, request)?;

	if !github::pull_request_matches_repository(&context.pr_url, &context.repository)? {
		eyre::bail!(
			"Pull request `{}` does not belong to the current repository `{}/{}`.",
			context.pr_url,
			context.repository.owner,
			context.repository.name,
		);
	}

	if let Some(recovery) =
		self::recovery::finalize_already_merged_manual_land_recovery(&context, request)?
	{
		println!(
			"land ok: pr={} merge_commit={} default_branch={} local_default_branch_synced=true",
			context.pr_url, recovery.merge_commit, context.repository.default_branch
		);

		return Ok(());
	}

	self::closeout::ensure_manual_land_checkout_is_managed_lane(
		&context.worktree_root,
		&context.project_worktree_root,
		self::closeout::manual_land_cleanup_identifier(&context.authority, &context.current_branch),
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
	let landing_state = self::landing::inspect_pull_request_landing_state_for_manual_land(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		context.github_command_path.as_deref(),
	)?;
	let current_head = self::git::current_head_oid(&context.cwd)?;
	let execution_mode = self::landing::validate_landing_state(
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
	let merge_commit = self::landing::execute_land_merge(
		&context,
		&current_head,
		landed_change_record.as_str(),
		execution_mode,
	)?;
	let landed_change_record =
		self::landing::load_authoritative_landed_change_record(&context, &merge_commit)?;

	self::closeout::finalize_land_closeout(
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

#[cfg(test)]
fn resolve_authority(
	config_path: Option<&Path>,
	explicit: Option<&str>,
	manual_authority: bool,
	worktree_root: &Path,
) -> Result<ManualAuthority> {
	authority::resolve_authority(config_path, explicit, manual_authority, worktree_root)
}

#[cfg(test)]
fn ensure_manual_land_checkout_is_managed_lane(
	repo_root: &Path,
	worktree_root: &Path,
	identifier: &str,
) -> Result<()> {
	closeout::ensure_manual_land_checkout_is_managed_lane(repo_root, worktree_root, identifier)
}

#[cfg(test)]
fn execute_land_merge(
	context: &ManualLandContext,
	current_head: &str,
	landed_change_record: &str,
	execution_mode: LandExecutionMode,
) -> Result<String> {
	landing::execute_land_merge(context, current_head, landed_change_record, execution_mode)
}

#[cfg(test)]
fn load_authoritative_landed_change_record(
	context: &ManualLandContext,
	merge_commit: &str,
) -> Result<String> {
	landing::load_authoritative_landed_change_record(context, merge_commit)
}

#[cfg(test)]
fn validate_landing_state(
	landing_state: &PullRequestLandingState,
	pr_url: &str,
	expected_base_branch: &str,
	current_branch: &str,
	current_head: &str,
) -> Result<LandExecutionMode> {
	landing::validate_landing_state(
		landing_state,
		pr_url,
		expected_base_branch,
		current_branch,
		current_head,
	)
}

#[cfg(test)]
fn finalize_already_merged_manual_land_recovery(
	context: &ManualLandContext,
	request: &ManualLandRequest,
) -> Result<Option<ManualLandRecoveryOutcome>> {
	recovery::finalize_already_merged_manual_land_recovery(context, request)
}

#[cfg(test)] mod tests;
