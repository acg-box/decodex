use serde_json::json;

use crate::tracker::linear::{
	mapping,
	schema::{
		GraphqlError, IssueRelationConnection, LabelConnection, LinearIssue, LinearIssueRelation,
		LinearLabel, LinearRelatedIssue, LinearState, LinearTeam, LinearUser, PageInfo,
		StateConnection,
	},
	transport,
};

#[test]
fn map_issue_preserves_priority_and_created_at() {
	let issue = LinearIssue {
		id: String::from("issue-1"),
		identifier: String::from("PUB-101"),
		title: String::from("Implement ordering"),
		creator: Some(LinearUser {
			display_name: Some(String::from("Yvette")),
			name: Some(String::from("yvette")),
			email: Some(String::from("yvette@example.com")),
		}),
		description: Some(String::from("Body")),
		priority: Some(2),
		created_at: String::from("2026-03-13T04:16:17.133Z"),
		updated_at: String::from("2026-03-14T04:16:17.133Z"),
		state: LinearState { id: String::from("state-todo"), name: String::from("Todo") },
		team: LinearTeam {
			id: String::from("team-1"),
			name: String::from("Pubfi"),
			states: StateConnection {
				nodes: vec![LinearState {
					id: String::from("state-todo"),
					name: String::from("Todo"),
				}],
			},
			labels: LabelConnection {
				nodes: vec![LinearLabel {
					id: String::from("label-needs"),
					name: String::from("decodex:needs-attention"),
				}],
				page_info: None,
			},
		},
		labels: LabelConnection {
			nodes: vec![LinearLabel {
				id: String::from("label-manual"),
				name: String::from("decodex:manual-only"),
			}],
			page_info: Some(PageInfo { has_next_page: false, end_cursor: None }),
		},
		inverse_relations: IssueRelationConnection {
			nodes: vec![LinearIssueRelation {
				relation_type: String::from("blocks"),
				issue: LinearRelatedIssue {
					id: String::from("issue-2"),
					identifier: String::from("PUB-102"),
					state: LinearState {
						id: String::from("state-progress"),
						name: String::from("In Progress"),
					},
				},
			}],
			page_info: PageInfo { has_next_page: false, end_cursor: None },
		},
	};
	let blockers = mapping::map_blockers(&issue.inverse_relations.nodes);
	let mapped = mapping::map_issue(issue, blockers);

	assert_eq!(mapped.priority, Some(2));
	assert_eq!(mapped.author.as_deref(), Some("Yvette"));
	assert_eq!(mapped.created_at, "2026-03-13T04:16:17.133Z");
	assert_eq!(mapped.updated_at, "2026-03-14T04:16:17.133Z");
	assert_eq!(mapped.blockers.len(), 1);
	assert_eq!(mapped.blockers[0].identifier, "PUB-102");
	assert_eq!(mapped.blockers[0].state.name, "In Progress");
}

#[test]
fn map_blockers_filters_non_blocking_relations() {
	let blockers = mapping::map_blockers(&[
		LinearIssueRelation {
			relation_type: String::from("blocks"),
			issue: LinearRelatedIssue {
				id: String::from("issue-2"),
				identifier: String::from("PUB-102"),
				state: LinearState {
					id: String::from("state-progress"),
					name: String::from("In Progress"),
				},
			},
		},
		LinearIssueRelation {
			relation_type: String::from("related"),
			issue: LinearRelatedIssue {
				id: String::from("issue-3"),
				identifier: String::from("PUB-103"),
				state: LinearState { id: String::from("state-done"), name: String::from("Done") },
			},
		},
	]);

	assert_eq!(blockers.len(), 1);
	assert_eq!(blockers[0].identifier, "PUB-102");
}

#[test]
fn rate_limited_error_message_uses_typed_linear_extensions() {
	let errors = vec![GraphqlError {
		message: String::from("Too many requests"),
		extensions: Some(json!({
			"code": "RATELIMITED",
			"userPresentableMessage": "API rate limit exceeded",
			"reset": 1_777_392_000
		})),
	}];
	let message =
		transport::rate_limited_error_message(&errors).expect("rate limit should classify");

	assert!(message.contains("rate limited"));
	assert!(message.contains("1777392000"));
	assert!(message.contains("API rate limit exceeded"));
}
