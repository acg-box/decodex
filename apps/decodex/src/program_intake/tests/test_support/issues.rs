use crate::tracker::{TrackerIssue, TrackerState, TrackerTeam};

pub(crate) fn issue(identifier: &str, state: &str) -> TrackerIssue {
	TrackerIssue {
		id: format!("id-{identifier}"),
		identifier: identifier.to_owned(),
		project_slug: None,
		title: format!("Issue {identifier}"),
		author: None,
		description: format!("Implement {identifier}."),
		priority: None,
		created_at: String::from("2026-06-01T00:00:00Z"),
		updated_at: String::from("2026-06-01T00:00:00Z"),
		state: TrackerState { id: format!("state-{state}"), name: state.to_owned() },
		team: TrackerTeam {
			id: String::from("team-test"),
			name: String::from("Team"),
			states: vec![
				TrackerState { id: String::from("state-Todo"), name: String::from("Todo") },
				TrackerState {
					id: String::from("state-In Progress"),
					name: String::from("In Progress"),
				},
				TrackerState { id: String::from("state-Done"), name: String::from("Done") },
			],
			labels: Vec::new(),
		},
		labels_complete: true,
		labels: Vec::new(),
		blockers: Vec::new(),
	}
}
