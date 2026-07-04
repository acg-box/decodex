use std::{path::PathBuf, time::Duration};

use crate::{
	commit_message::MANUAL_AUTHORITY,
	git_credentials::GitCredentialSource,
	github::RepositoryContext,
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

pub(in crate::manual) const MANUAL_LAND_CLOSEOUT_MARKER_GIT_PATH: &str =
	"decodex/manual-land-closeout";
pub(in crate::manual) const MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT: Duration =
	Duration::from_secs(15 * 60);
pub(in crate::manual) const MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS: usize = 4;
pub(in crate::manual) const MANUAL_LAND_MERGEABILITY_RETRY_DELAY: Duration = Duration::from_secs(2);

pub(in crate::manual) struct PreparedCloseout {
	pub(in crate::manual) tracker: LinearClient,
	pub(in crate::manual) issue: TrackerIssue,
	pub(in crate::manual) completed_state: String,
	pub(in crate::manual) service_id: String,
	pub(in crate::manual) needs_attention_label: String,
}

pub(in crate::manual) struct ManualLandContext {
	pub(in crate::manual) cwd: PathBuf,
	pub(in crate::manual) current_branch: String,
	pub(in crate::manual) worktree_root: PathBuf,
	pub(in crate::manual) project_worktree_root: PathBuf,
	pub(in crate::manual) canonical_repo_root: PathBuf,
	pub(in crate::manual) authority: ManualAuthority,
	pub(in crate::manual) service_id: String,
	pub(in crate::manual) workflow: Option<WorkflowDocument>,
	pub(in crate::manual) github_token_env_var: String,
	pub(in crate::manual) github_token: String,
	pub(in crate::manual) github_command_path: Option<PathBuf>,
	pub(in crate::manual) repository: RepositoryContext,
	pub(in crate::manual) prepared_closeout: Option<PreparedCloseout>,
	pub(in crate::manual) review_handoff: Option<ReviewHandoffMarker>,
	pub(in crate::manual) pr_url: String,
	pub(in crate::manual) review_branch: String,
	pub(in crate::manual) public_projection_privacy_classifier:
		ConfiguredPublicProjectionPrivacyClassifier,
}
impl ManualLandContext {
	pub(in crate::manual) fn default_branch_git_credentials(&self) -> GitCredentialSource<'_> {
		GitCredentialSource::new(&self.github_token_env_var, &self.github_token)
	}
}

pub(in crate::manual) struct ManualLandRecoveryOutcome {
	pub(in crate::manual) merge_commit: String,
}

#[derive(Default)]
pub(in crate::manual) struct ManualLandCloseoutMarkerRecord {
	pub(in crate::manual) pr_url: Option<String>,
	pub(in crate::manual) merge_commit: Option<String>,
	pub(in crate::manual) branch_name: Option<String>,
	pub(in crate::manual) landed_change: Option<String>,
}

pub(in crate::manual) struct ManualLandLedgerContext<'a> {
	pub(in crate::manual) service_id: &'a str,
	pub(in crate::manual) issue: &'a TrackerIssue,
	pub(in crate::manual) state_store: &'a StateStore,
	pub(in crate::manual) handoff: &'a ReviewHandoffMarker,
	pub(in crate::manual) pr_url: &'a str,
	pub(in crate::manual) merge_commit: &'a str,
	pub(in crate::manual) branch_name: &'a str,
	pub(in crate::manual) worktree_path: &'a str,
	pub(in crate::manual) completed_state: &'a str,
	pub(in crate::manual) default_branch: &'a str,
	pub(in crate::manual) privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::manual) enum LandExecutionMode {
	MergeAndCloseout,
	CloseoutOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::manual) enum ManualAuthority {
	Issue(String),
	Manual,
}
impl ManualAuthority {
	pub(in crate::manual) fn commit_message_value(&self) -> &str {
		match self {
			Self::Issue(identifier) => identifier.as_str(),
			Self::Manual => MANUAL_AUTHORITY,
		}
	}

	pub(in crate::manual) fn issue_identifier(&self) -> Option<&str> {
		match self {
			Self::Issue(identifier) => Some(identifier.as_str()),
			Self::Manual => None,
		}
	}

	pub(in crate::manual) fn is_manual(&self) -> bool {
		matches!(self, Self::Manual)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::manual) struct ManualCommitActiveLaneBlocker {
	pub(in crate::manual) issue_id: String,
	pub(in crate::manual) branch_name: String,
	pub(in crate::manual) worktree_path: PathBuf,
}
