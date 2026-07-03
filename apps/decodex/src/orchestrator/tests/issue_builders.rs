use crate::{
	orchestrator::tests::{
		BTreeSet, TrackerIssue, TrackerIssueBlocker, TrackerLabel, TrackerState, TrackerTeam, iter,
	},
	tracker,
};

pub(super) fn sample_issue(state_name: &str, labels: &[&str]) -> TrackerIssue {
	sample_issue_with_project_slug_and_sort_fields(
		"issue-1",
		"PUB-101",
		"pubfi",
		state_name,
		labels,
		Some(3),
		"2026-03-13T04:16:17.133Z",
	)
}

pub(super) fn sample_blocker(id: &str, identifier: &str, state_name: &str) -> TrackerIssueBlocker {
	TrackerIssueBlocker {
		id: id.to_owned(),
		identifier: identifier.to_owned(),
		state: TrackerState { id: format!("state-{id}"), name: state_name.to_owned() },
	}
}

pub(super) fn sample_issue_with_sort_fields(
	id: &str,
	identifier: &str,
	state_name: &str,
	labels: &[&str],
	priority: Option<i64>,
	created_at: &str,
) -> TrackerIssue {
	sample_issue_with_project_slug_and_sort_fields(
		id, identifier, "pubfi", state_name, labels, priority, created_at,
	)
}

pub(super) fn sample_issue_with_project_slug_and_sort_fields(
	id: &str,
	identifier: &str,
	_project_slug: &str,
	state_name: &str,
	labels: &[&str],
	priority: Option<i64>,
	created_at: &str,
) -> TrackerIssue {
	let team_labels = vec![
		TrackerLabel {
			id: String::from("label-queued"),
			name: crate::tracker::automation_queue_label(_project_slug),
		},
		TrackerLabel {
			id: String::from("label-active"),
			name: crate::tracker::automation_active_label(_project_slug),
		},
		TrackerLabel {
			id: String::from("label-manual"),
			name: String::from("decodex:manual-only"),
		},
		TrackerLabel {
			id: String::from("label-needs-attention"),
			name: String::from("decodex:needs-attention"),
		},
	];

	TrackerIssue {
		id: id.to_owned(),
		identifier: identifier.to_owned(),
		#[cfg(test)]
		project_slug: Some(_project_slug.to_owned()),
		title: String::from("Implement orchestration"),
		author: Some(String::from("Yvette")),
		description: String::from("Body"),
		priority,
		created_at: created_at.to_owned(),
		updated_at: created_at.to_owned(),
		state: TrackerState { id: String::from("state-current"), name: state_name.to_owned() },
		team: TrackerTeam {
			id: String::from("team-1"),
			name: String::from("Pubfi"),
			states: vec![
				TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
				TrackerState {
					id: String::from("state-progress"),
					name: String::from("In Progress"),
				},
				TrackerState { id: String::from("state-review"), name: String::from("In Review") },
			],
			labels: team_labels.clone(),
		},
		labels_complete: true,
		labels: labels
			.iter()
			.copied()
			.chain(iter::once(tracker::automation_queue_label(_project_slug).as_str()))
			.collect::<BTreeSet<_>>()
			.into_iter()
			.enumerate()
			.map(|(index, label)| TrackerLabel {
				id: format!("label-{index}"),
				name: label.to_owned(),
			})
			.collect(),
		blockers: Vec::new(),
	}
}

pub(super) fn sample_issue_without_needs_attention_team_label(
	state_name: &str,
	labels: &[&str],
) -> TrackerIssue {
	let mut issue = sample_issue(state_name, labels);

	issue.team.labels.retain(|label| label.name != "decodex:needs-attention");

	issue
}
