use crate::{
	prelude::{Result, eyre},
	tracker::{
		TrackerIssue, TrackerIssueBlocker, TrackerLabel, TrackerState, TrackerTeam,
		linear::schema::{LinearIssue, LinearIssueRelation, LinearUser, PageInfo},
	},
};

pub(super) fn require_end_cursor(page_info: PageInfo, message: &str) -> Result<String> {
	page_info.end_cursor.ok_or_else(|| eyre::eyre!(message.to_owned()))
}

pub(super) fn map_blockers(relations: &[LinearIssueRelation]) -> Vec<TrackerIssueBlocker> {
	relations
		.iter()
		.filter(|relation| relation.relation_type == "blocks")
		.map(|relation| TrackerIssueBlocker {
			id: relation.issue.id.clone(),
			identifier: relation.issue.identifier.clone(),
			state: TrackerState {
				id: relation.issue.state.id.clone(),
				name: relation.issue.state.name.clone(),
			},
		})
		.collect()
}

pub(super) fn map_issue(issue: LinearIssue, blockers: Vec<TrackerIssueBlocker>) -> TrackerIssue {
	let author = linear_user_display_name(issue.creator.as_ref());

	TrackerIssue {
		id: issue.id,
		identifier: issue.identifier,
		#[cfg(test)]
		project_slug: None,
		title: issue.title,
		author,
		description: issue.description.unwrap_or_default(),
		priority: issue.priority,
		created_at: issue.created_at,
		updated_at: issue.updated_at,
		state: TrackerState { id: issue.state.id, name: issue.state.name },
		team: TrackerTeam {
			id: issue.team.id,
			name: issue.team.name,
			states: issue
				.team
				.states
				.nodes
				.into_iter()
				.map(|state| TrackerState { id: state.id, name: state.name })
				.collect(),
			labels: issue
				.team
				.labels
				.nodes
				.into_iter()
				.map(|label| TrackerLabel { id: label.id, name: label.name })
				.collect(),
		},
		labels_complete: issue.labels.page_info.is_none_or(|page_info| !page_info.has_next_page),
		labels: issue
			.labels
			.nodes
			.into_iter()
			.map(|label| TrackerLabel { id: label.id, name: label.name })
			.collect(),
		blockers,
	}
}

fn linear_user_display_name(user: Option<&LinearUser>) -> Option<String> {
	let user = user?;

	[&user.display_name, &user.name, &user.email]
		.into_iter()
		.filter_map(|value| value.as_deref())
		.map(str::trim)
		.find(|value| !value.is_empty())
		.map(str::to_owned)
}
