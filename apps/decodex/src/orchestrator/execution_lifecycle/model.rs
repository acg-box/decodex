use crate::orchestrator::PublicProjectionPrivacyClassifier;

pub(crate) struct TerminalFailureLifecycle<'a> {
	pub(crate) error_class: &'a str,
	pub(crate) next_action: &'a str,
	pub(crate) pr_url: Option<&'a str>,
	pub(crate) target_state: &'a str,
	pub(crate) worktree_path: &'a str,
	pub(crate) manual_attention_requested: bool,
	pub(crate) retained_source_error_class: Option<&'a str>,
}

pub(crate) struct RunStartedLifecycleFields<'a> {
	pub(crate) worktree_path: &'a str,
	pub(crate) commit_sha: &'a str,
	pub(crate) privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
}
