//! Archive candidate planning and skip decisions.

use std::collections::{BTreeMap, BTreeSet};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	tracker::{self, IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ArchivePlan {
	pub(super) candidates: Vec<ArchiveCandidate>,
	pub(super) skipped: Vec<ArchiveSkip>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ArchiveCandidate {
	pub(super) id: String,
	pub(super) identifier: String,
	pub(super) title: String,
	pub(super) state: String,
	pub(super) updated_at: String,
	pub(super) repo_labels: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ArchiveSkip {
	pub(super) identifier: String,
	pub(super) reason: String,
}
pub(super) fn build_archive_plan<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	repo_labels: &[String],
	updated_before: &str,
) -> Result<ArchivePlan>
where
	T: IssueTracker + ?Sized,
{
	let issues = collect_repo_labeled_issues(tracker, repo_labels)?;
	let mut candidates = Vec::new();
	let mut skipped = Vec::new();

	for (issue, matched_repo_labels) in issues {
		match archive_skip_reason(tracker, project, workflow, &issue, updated_before)? {
			Some(reason) => skipped.push(ArchiveSkip { identifier: issue.identifier, reason }),
			None => candidates.push(ArchiveCandidate {
				id: issue.id,
				identifier: issue.identifier,
				title: issue.title,
				state: issue.state.name,
				updated_at: issue.updated_at,
				repo_labels: matched_repo_labels,
			}),
		}
	}

	candidates.sort_by(|left, right| left.identifier.cmp(&right.identifier));
	skipped.sort_by(|left, right| left.identifier.cmp(&right.identifier));

	Ok(ArchivePlan { candidates, skipped })
}

fn collect_repo_labeled_issues<T>(
	tracker: &T,
	repo_labels: &[String],
) -> Result<Vec<(TrackerIssue, Vec<String>)>>
where
	T: IssueTracker + ?Sized,
{
	let mut issues_by_id: BTreeMap<String, (TrackerIssue, BTreeSet<String>)> = BTreeMap::new();

	for repo_label in repo_labels {
		for issue in tracker.list_issues_with_label(repo_label)? {
			let entry = issues_by_id.entry(issue.id.clone()).or_insert((issue, BTreeSet::new()));

			entry.1.insert(repo_label.clone());
		}
	}

	Ok(issues_by_id
		.into_values()
		.map(|(issue, labels)| (issue, labels.into_iter().collect()))
		.collect())
}

fn archive_skip_reason<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	updated_before: &str,
) -> Result<Option<String>>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();

	if !tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(Some(format!(
			"state `{}` is not a configured terminal state",
			issue.state.name
		)));
	}
	if issue_updated_at_is_not_older_than_cutoff(issue, updated_before)? {
		return Ok(Some(format!(
			"updated at `{}` is not older than cutoff `{updated_before}`",
			issue.updated_at
		)));
	}

	for label in protected_labels(project.service_id(), workflow) {
		if tracker::issue_has_label_with_server_confirmation(tracker, issue, &label)? {
			return Ok(Some(format!("protected label `{label}` is present")));
		}
	}

	Ok(None)
}

fn issue_updated_at_is_not_older_than_cutoff(
	issue: &TrackerIssue,
	updated_before: &str,
) -> Result<bool> {
	let issue_updated_at = OffsetDateTime::parse(&issue.updated_at, &Rfc3339).map_err(|error| {
		eyre::eyre!(
			"Failed to parse Linear updatedAt `{}` for issue `{}`: {error}",
			issue.updated_at,
			issue.identifier
		)
	})?;
	let cutoff = OffsetDateTime::parse(updated_before, &Rfc3339).map_err(|error| {
		eyre::eyre!("Failed to parse archive cutoff `{updated_before}`: {error}")
	})?;

	Ok(issue_updated_at >= cutoff)
}

fn protected_labels(service_id: &str, workflow: &WorkflowDocument) -> Vec<String> {
	let tracker_policy = workflow.frontmatter().tracker();

	vec![
		tracker::automation_active_label(service_id),
		tracker::automation_queue_label(service_id),
		tracker_policy.needs_attention_label().to_owned(),
		tracker_policy.opt_out_label().to_owned(),
	]
}
