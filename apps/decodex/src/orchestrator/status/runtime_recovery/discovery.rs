use crate::{
	commit_message,
	orchestrator::status::{
		BTreeSet, Instant, IssueTracker, Path, RecoverableWorktreeSkipCache, ServiceConfig,
		TrackerIssue, fs, runtime_recovery::issue, slice,
	},
	prelude::Result,
	tracker,
};

pub(crate) fn refresh_recoverable_runtime_issues<T>(
	tracker: &T,
	issue_ids: &[String],
) -> Result<Vec<TrackerIssue>>
where
	T: IssueTracker,
{
	match tracker.refresh_issues(issue_ids) {
		Ok(issues) => Ok(issues),
		Err(error)
			if issue_ids.iter().any(|issue_id| {
				tracker::issue_lookup_missing_error_for_candidate(&error, issue_id)
			}) =>
		{
			let mut issues = Vec::new();

			for issue_id in issue_ids {
				match tracker.refresh_issues(slice::from_ref(issue_id)) {
					Ok(mut refreshed) => issues.append(&mut refreshed),
					Err(error)
						if tracker::issue_lookup_missing_error_for_candidate(&error, issue_id) => {},
					Err(error) => return Err(error),
				}
			}

			Ok(issues)
		},
		Err(error) => Err(error),
	}
}

pub(crate) fn append_recoverable_tracker_issue<T>(
	tracker: &T,
	project: &ServiceConfig,
	issue_identifier: &str,
	known_identifiers: &mut BTreeSet<String>,
	issues: &mut Vec<TrackerIssue>,
	mut recoverable_worktree_skip_cache: Option<&mut RecoverableWorktreeSkipCache>,
) -> Result<()>
where
	T: IssueTracker,
{
	let canonical_identifier = issue_identifier.to_ascii_uppercase();

	if known_identifiers.contains(&canonical_identifier) {
		return Ok(());
	}

	let now = Instant::now();

	if let Some(cache) = recoverable_worktree_skip_cache.as_deref_mut()
		&& cache.is_suppressed(&canonical_identifier, now)
	{
		tracing::debug!(
			issue = canonical_identifier,
			"Skipped retained worktree tracker lookup because a recent recovery probe already found no service ownership."
		);

		return Ok(());
	}

	let issue = match tracker.get_issue_by_identifier(issue_identifier) {
		Ok(issue) => issue,
		Err(error)
			if tracker::issue_lookup_missing_error_for_candidate(&error, issue_identifier) =>
			None,
		Err(error) => return Err(error),
	};
	let Some(issue) = issue else {
		if let Some(cache) = recoverable_worktree_skip_cache {
			cache.remember(&canonical_identifier, now);
		}

		return Ok(());
	};

	if !issue::issue_has_recovered_service_ownership(tracker, &issue, project.service_id())? {
		tracing::warn!(
			issue = issue.identifier,
			active_label = tracker::automation_active_label(project.service_id()),
			labels_complete = issue.labels_complete,
			"Skipping retained worktree recovery because the tracker issue is not explicitly owned by this service."
		);

		if let Some(cache) = recoverable_worktree_skip_cache {
			cache.remember(&canonical_identifier, now);
		}

		return Ok(());
	}

	known_identifiers.insert(issue.identifier.to_ascii_uppercase());
	issues.push(issue);

	Ok(())
}

pub(crate) fn recoverable_worktree_identifiers(worktree_root: &Path) -> Result<Vec<String>> {
	if !worktree_root.exists() {
		return Ok(Vec::new());
	}

	let mut issue_identifiers = fs::read_dir(worktree_root)?
		.filter_map(|entry| entry.ok())
		.filter_map(|entry| {
			entry
				.file_type()
				.ok()
				.filter(|file_type| file_type.is_dir())
				.and_then(|_| entry.file_name().into_string().ok())
		})
		.filter(|name| commit_message::looks_like_issue_identifier(name))
		.collect::<Vec<_>>();

	issue_identifiers.sort();
	issue_identifiers.dedup();

	Ok(issue_identifiers)
}
