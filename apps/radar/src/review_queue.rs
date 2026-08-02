//! Upstream review queue source discovery and subject classification.

mod bundles;
mod commits;
mod published;
mod subjects;

pub(crate) use self::commits::RecentCommit;

use std::{collections::BTreeMap, path::Path};

use serde_json::{self, Value};

use crate::{
	GitHubApi, RadarLedger, RadarRefreshQueueRequest, UPSTREAM_REVIEW_QUEUE_SCHEMA, prelude::Result,
};

#[derive(Debug)]
pub(super) struct QueueBuild {
	pub(super) queue: Value,
	pub(super) ledger_enabled: bool,
}

pub(super) fn build_review_queue(
	request: &RadarRefreshQueueRequest,
	root: &Path,
	api: &GitHubApi,
) -> Result<QueueBuild> {
	let (default_branch, upstream_head, commits) =
		commits::recent_commits(api, &request.repo, request.search_limit)?;
	let recent_commits_scanned = commits.len();
	let (published_prs, published_shas) =
		published::published_subjects(&crate::absolute_repo_path(root, &request.signals_dir))?;
	let ledger_path = crate::ledger_path(root, request);
	let mut ledger = ledger_path.as_deref().map(RadarLedger::open).transpose()?;
	let mut subjects = BTreeMap::<(String, String), Value>::new();
	let mut published_seen = 0_usize;

	for commit in commits {
		let pr_number = commits::maybe_promote_commit_to_pr(api, &request.repo, &commit.sha)?;
		let subject_kind = if pr_number.is_some() { "pr" } else { "commit" };
		let subject_id = pr_number.map_or_else(|| commit.sha.clone(), |number| number.to_string());

		if let Some(ledger) = &mut ledger {
			ledger.record_commit(&request.repo, &commit, pr_number)?;
		}

		if published_shas.contains(&commit.sha)
			|| pr_number.is_some_and(|number| published_prs.contains(&number))
		{
			published_seen += 1;

			if let Some(ledger) = &mut ledger {
				ledger.record_review(
					&request.repo,
					subject_kind,
					&subject_id,
					"signal",
					"Already present in published signal collection.",
					Some("confirmed"),
				)?;
			}

			continue;
		}

		let key = (subject_kind.to_owned(), subject_id.clone());

		if let Some(current) = subjects.get_mut(&key) {
			subjects::append_commit_sha(current, &commit.sha);

			continue;
		}

		let bundle = match pr_number {
			Some(number) => bundles::build_pr_bundle(api, &request.repo, number)?,
			None => bundles::build_commit_bundle(api, &request.repo, &commit.sha)?,
		};

		subjects.insert(
			key,
			subjects::subject_from_bundle(&bundle, subject_kind, &subject_id, &commit),
		);

		if let Some(ledger) = &mut ledger {
			ledger.record_review(
				&request.repo,
				subject_kind,
				&subject_id,
				"watch",
				"Queued for AI upstream review by deterministic Radar sync.",
				Some("likely"),
			)?;
		}
	}

	if let Some(ledger) = ledger {
		ledger.commit()?;
	}

	let ordered_subjects = subjects::sort_queue_subjects(subjects.into_values().collect());
	let queue = review_queue_payload(
		request,
		&default_branch,
		&upstream_head,
		recent_commits_scanned,
		published_seen,
		ordered_subjects,
	)?;

	Ok(QueueBuild { queue, ledger_enabled: !request.no_ledger })
}

fn review_queue_payload(
	request: &RadarRefreshQueueRequest,
	default_branch: &str,
	upstream_head: &str,
	recent_commits_scanned: usize,
	published_seen: usize,
	subjects: Vec<Value>,
) -> Result<Value> {
	let critical = count_priority(&subjects, "critical");
	let high = count_priority(&subjects, "high");
	let normal = count_priority(&subjects, "normal");
	let low = count_priority(&subjects, "low");

	Ok(serde_json::json!({
		"schema": UPSTREAM_REVIEW_QUEUE_SCHEMA,
		"repo": request.repo,
		"generated_at": crate::utc_now_iso()?,
		"source": {
			"default_branch": default_branch,
			"upstream_head": upstream_head,
			"search_limit": request.search_limit,
			"signals_dir": request.signals_dir.to_string_lossy(),
		},
		"subjects": subjects,
		"counts": {
			"recent_commits_scanned": recent_commits_scanned,
			"published_subjects_seen": published_seen,
			"subjects_queued": critical + high + normal + low,
			"critical": critical,
			"high": high,
			"normal": normal,
			"low": low,
		},
	}))
}

fn count_priority(subjects: &[Value], priority: &str) -> usize {
	subjects
		.iter()
		.filter(|subject| {
			subject
				.get("review_priority")
				.and_then(Value::as_str)
				.is_some_and(|value| value == priority)
		})
		.count()
}
