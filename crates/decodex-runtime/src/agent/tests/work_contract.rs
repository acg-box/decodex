//! Work tools enforce user authority and exact artifact identity across Projects.
use std::{fs, path::Path};

use sha2::{Digest as _, Sha256};

use crate::agent::tests::{self, AgentCoordinator, AgentError, Value};
use decodex_core::work::{
	ResultSource, Work, WorkAssessment, WorkCommand, WorkRequirements, WorkResult, WorkState,
};

async fn manage(
	agent: &mut AgentCoordinator,
	command_id: &str,
	command: WorkCommand,
) -> Result<Value, AgentError> {
	let caller = agent
		.store
		.get_agent_work_item("sage".into())
		.await
		.expect("Project and Work qualification fixture");

	agent.tool(&caller,&serde_json::json!({"tool":"work_manage","arguments":{"commandId":command_id,"command":command}})).await
}
#[tokio::test]
async fn four_works_in_two_projects_share_an_agent_without_sharing_results() {
	let (mut agent, _sent, home) = tests::fixture().await;

	agent.config.cwd = home
		.path()
		.canonicalize()
		.expect("Project and Work qualification fixture")
		.display()
		.to_string();

	agent
		.start_agent("sage", "Plan the market and trip")
		.await
		.expect("Project and Work qualification fixture");

	tests::complete(&mut agent, "sage").await;

	agent
		.enqueue_user_message(
			"sage",
			"decision",
			"Prepare four separate outcomes for the market and trip",
		)
		.await
		.expect("Project and Work qualification fixture");
	agent.wake_pending().await.expect("Project and Work qualification fixture");

	let decision = agent
		.store
		.list_agent_events_for_turn(
			agent
				.store
				.get_agent_work_item("sage".into())
				.await
				.expect("Project and Work qualification fixture")
				.active_turn_id
				.expect("Project and Work qualification fixture"),
			100,
		)
		.await
		.expect("Project and Work qualification fixture")
		.into_iter()
		.find(|e| e.event_kind == "user_message")
		.expect("Project and Work qualification fixture");

	create_four_works(&mut agent, decision.id).await;
	qualify_result_isolation(&mut agent, home.path()).await;

	let state =
		agent.store.work_portfolio(None).await.expect("Project and Work qualification fixture");

	assert_eq!(state.works.len(), 4);
	assert_eq!(state.works.iter().filter(|w| w.state == WorkState::Completed).count(), 1);
	assert!(state.works.iter().all(|w| w.owner_agent_id == "sage"));

	let caller = agent
		.store
		.get_agent_work_item("sage".into())
		.await
		.expect("Project and Work qualification fixture");

	for retired in ["agent_create_work", "agent_create_manager", "agent_create_goal"] {
		assert!(
			agent.tool(&caller, &serde_json::json!({"tool":retired,"arguments":{}})).await.is_err()
		);
	}

	assert!(
		super::super::tools()
			.as_array()
			.expect("Project and Work qualification fixture")
			.iter()
			.all(|s| !matches!(
				s["name"].as_str(),
				Some(
					"agent_create_work"
						| "agent_continue_worker"
						| "agent_create_manager"
						| "agent_create_goal"
				)
			))
	);
}

async fn create_four_works(agent: &mut AgentCoordinator, decision: i64) {
	for project in ["market", "trip"] {
		agent
			.store
			.register_project(project.into(), project.into(), format!("/{project}"))
			.await
			.expect("Project and Work qualification fixture");
	}
	for (id, project) in
		[("venue", "market"), ("insurance", "market"), ("route", "trip"), ("hotel", "trip")]
	{
		let work = Work {
			id: id.into(),
			project_id: Some(project.into()),
			owner_agent_id: "sage".into(),
			title: id.into(),
			requirements: WorkRequirements {
				id: format!("{id}-r1"),
				goal: format!("Research {id}"),
				criteria: "Return a sourced recommendation, not a purchase".into(),
				source: format!("event:{}", decision),
			},
			state: WorkState::Open,
			reason: None,
			resume_condition: None,
			user_hold: false,
			assessment: None,
		};

		manage(agent, &format!("create-{id}"), WorkCommand::Create { work })
			.await
			.expect("Project and Work qualification fixture");
	}
}

async fn qualify_result_isolation(agent: &mut AgentCoordinator, home: &Path) {
	let path = home.join("venue.txt");

	fs::write(&path, "Public venue evidence").expect("Project and Work qualification fixture");

	let digest = Sha256::digest(b"Public venue evidence")
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect::<String>();
	let result = WorkResult {
		id: "venue-result".into(),
		work_id: "venue".into(),
		requirements_id: "venue-r1".into(),
		instruction_id: None,
		native_contribution_id: None,
		agent_id: "sage".into(),
		summary: "A suitable public venue".into(),
		source: ResultSource::Artifact { path: path.display().to_string(), sha256: digest },
		limits: "Research only; no reservation".into(),
	};

	manage(agent, "record-result", WorkCommand::RecordResult { result: result.clone() })
		.await
		.expect("Project and Work qualification fixture");

	fs::write(path, "Changed venue evidence").expect("Project and Work qualification fixture");

	manage(agent, "record-result", WorkCommand::RecordResult { result: result.clone() })
		.await
		.expect("an exact committed receipt survives a later artifact change");

	let mut changed = result;

	changed.id = "changed-result".into();

	assert!(
		manage(agent, "changed-result", WorkCommand::RecordResult { result: changed })
			.await
			.is_err()
	);
	assert!(
		manage(
			agent,
			"foreign-result",
			WorkCommand::Complete {
				work_id: "hotel".into(),
				expected_requirements: "hotel-r1".into(),
				assessment: WorkAssessment {
					requirements_id: "hotel-r1".into(),
					result_ids: vec!["venue-result".into()],
					summary: "Wrongly reused venue".into(),
					limits: "No hotel proof".into()
				}
			}
		)
		.await
		.is_err()
	);

	fs::write(home.join("venue.txt"), "Public venue evidence")
		.expect("Project and Work qualification fixture");

	manage(
		agent,
		"complete-venue",
		WorkCommand::Complete {
			work_id: "venue".into(),
			expected_requirements: "venue-r1".into(),
			assessment: WorkAssessment {
				requirements_id: "venue-r1".into(),
				result_ids: vec!["venue-result".into()],
				summary: "Venue research meets the request".into(),
				limits: "No reservation".into(),
			},
		},
	)
	.await
	.expect("Project and Work qualification fixture");
}
