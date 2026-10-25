use rusqlite::Connection;
use tempfile::TempDir;

use crate::{AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus, SqliteStore};
use decodex_core::{
	guardian::{GuardianSource, ReviewDetails, ReviewPreference},
	work::{
		CollaborationInstruction, CollaborationRequest, CollaborationState, Material,
		NativeContribution, NativeWorkDelivery, ResultSource, Work, WorkAssessment, WorkCommand,
		WorkPortfolio, WorkRequirements, WorkResult, WorkState,
	},
};

fn requirements(id: &str) -> WorkRequirements {
	WorkRequirements {
		id: id.into(),
		goal: "Prepare an accessible plan".into(),
		criteria: "Sources and costs are checked".into(),
		source: "explicit-user-decision".into(),
	}
}

fn work(id: &str, owner: &str) -> Work {
	Work {
		id: id.into(),
		project_id: None,
		owner_agent_id: owner.into(),
		title: id.into(),
		requirements: requirements("r1"),
		state: WorkState::Open,
		reason: None,
		resume_condition: None,
		user_hold: false,
		assessment: None,
	}
}

fn result(
	id: &str,
	work: &str,
	revision: &str,
	actor: &str,
	instruction: Option<&str>,
) -> WorkResult {
	WorkResult {
		id: id.into(),
		work_id: work.into(),
		requirements_id: revision.into(),
		instruction_id: instruction.map(str::to_owned),
		native_contribution_id: None,
		agent_id: actor.into(),
		summary: "Checked the requested plan".into(),
		source: ResultSource::NativeItem {
			thread_id: actor.into(),
			turn_id: "turn".into(),
			item_id: "item".into(),
		},
		limits: "Fixture evidence; no actual reservation".into(),
	}
}

fn assessment(revision: &str, ids: &[&str]) -> WorkAssessment {
	WorkAssessment {
		requirements_id: revision.into(),
		result_ids: ids.iter().map(|s| (*s).into()).collect(),
		summary: "The current criteria are met".into(),
		limits: "Planning only, not a booking".into(),
	}
}

fn instruction(id: &str) -> CollaborationInstruction {
	CollaborationInstruction {
		id: id.into(),
		requirements_id: "r1".into(),
		text: "Compare public accessible transport routes".into(),
		material_ids: vec![],
		event_id: None,
		delivered_turn_id: None,
	}
}

fn assert_repaired_request(p: &WorkPortfolio) {
	assert_eq!(p.collaborations[0].id, "transport");
	assert_eq!(p.collaborations[0].instruction.id, "i2");
	assert_eq!(p.collaborations[0].state, CollaborationState::Pending);
	assert_eq!(p.results.len(), 2);
}

fn persistent_store() -> (TempDir, SqliteStore) {
	let home = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&home.path().join("work.db")).unwrap();

	(home, store)
}

fn transport_request() -> WorkCommand {
	WorkCommand::Request {
		request: CollaborationRequest {
			id: "transport".into(),
			work_id: "venue".into(),
			recipient_agent_id: "iris".into(),
			instruction: instruction("i1"),
			state: CollaborationState::Pending,
			required: true,
			outcome: None,
			note: None,
		},
	}
}

async fn agents(store: &SqliteStore) {
	for id in ["sage", "iris"] {
		store
			.create_agent_work_item(AgentWorkItem {
				id: id.into(),
				parent_goal_id: None,
				kind: AgentWorkKind::Goal,
				title: id.into(),
				instructions: "Help within the user mandate".into(),
				codex_thread_id: None,
				active_turn_id: None,
				status: AgentWorkStatus::Open,
				dispatch_state: AgentDispatchState::Idle,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		store.bind_agent_thread(id.into(), id.into()).await.unwrap();
	}
}

async fn apply(store: &SqliteStore, actor: &str, id: &str, command: WorkCommand) {
	store.apply_work_command(actor.into(), id.into(), command, true).await.unwrap();
}

#[tokio::test]
async fn agent_scope_precedes_work_and_material_page_limits() {
	let home = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&home.path().join("scope.db")).unwrap();

	agents(&store).await;

	for i in 0..100 {
		let id = format!("other-{i:03}");

		apply(&store, "sage", &id, WorkCommand::Create { work: work(&id, "sage") }).await;
	}

	apply(&store, "iris", "iris-work", WorkCommand::Create { work: work("z-iris", "iris") }).await;
	apply(&store, "sage", "shared-work", WorkCommand::Create { work: work("shared", "sage") })
		.await;

	for i in 0..101 {
		let id = if i == 0 { "shared-material".into() } else { format!("private-{i}") };

		apply(
			&store,
			"sage",
			&format!("material-{i}"),
			WorkCommand::AddMaterial {
				material: Material {
					id,
					work_id: "shared".into(),
					title: "Evidence".into(),
					reference: "https://example.com/evidence".into(),
					sha256: None,
				},
			},
		)
		.await;
	}

	apply(
		&store,
		"sage",
		"request",
		WorkCommand::Request {
			request: CollaborationRequest {
				id: "request".into(),
				work_id: "shared".into(),
				recipient_agent_id: "iris".into(),
				instruction: CollaborationInstruction {
					id: "instruction".into(),
					requirements_id: "r1".into(),
					text: "Inspect only the selected evidence".into(),
					material_ids: vec!["shared-material".into()],
					event_id: None,
					delivered_turn_id: None,
				},
				state: CollaborationState::Pending,
				required: true,
				outcome: None,
				note: None,
			},
		},
	)
	.await;

	let visible = store.agent_work_portfolio("iris".into(), None).await.unwrap();

	assert!(
		visible.works.iter().any(|w| w.id == "z-iris"),
		"unrelated Work cannot consume this Agent's page"
	);
	assert_eq!(visible.works.len(), 2);
	assert_eq!(visible.materials.len(), 1, "private evidence cannot consume the granted page");
	assert_eq!(visible.materials[0].id, "shared-material");
	assert!(!visible.has_more);
}

#[tokio::test]
async fn results_cannot_complete_another_work_or_stale_requirements() {
	let home = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&home.path().join("work.db")).unwrap();

	agents(&store).await;

	for id in ["venue", "budget"] {
		apply(&store, "sage", id, WorkCommand::Create { work: work(id, "sage") }).await;
	}

	apply(
		&store,
		"sage",
		"result",
		WorkCommand::RecordResult { result: result("e1", "venue", "r1", "sage", None) },
	)
	.await;

	assert!(
		store
			.apply_work_command(
				"sage".into(),
				"wrong-work".into(),
				WorkCommand::Complete {
					work_id: "budget".into(),
					expected_requirements: "r1".into(),
					assessment: assessment("r1", &["e1"])
				},
				true
			)
			.await
			.is_err()
	);

	apply(
		&store,
		"sage",
		"revise",
		WorkCommand::Revise {
			work_id: "venue".into(),
			expected_requirements: "r1".into(),
			requirements: requirements("r2"),
		},
	)
	.await;

	assert!(
		store
			.apply_work_command(
				"sage".into(),
				"stale".into(),
				WorkCommand::Complete {
					work_id: "venue".into(),
					expected_requirements: "r2".into(),
					assessment: assessment("r2", &["e1"])
				},
				true
			)
			.await
			.is_err()
	);

	apply(
		&store,
		"sage",
		"recheck",
		WorkCommand::ReuseResult {
			work_id: "venue".into(),
			expected_requirements: "r2".into(),
			result_id: "e1".into(),
			assessment: "Unchanged route remains valid for the larger venue".into(),
		},
	)
	.await;
	apply(
		&store,
		"sage",
		"complete",
		WorkCommand::Complete {
			work_id: "venue".into(),
			expected_requirements: "r2".into(),
			assessment: assessment("r2", &["e1"]),
		},
	)
	.await;

	let p = store.work_portfolio(None).await.unwrap();

	assert_eq!(p.works.iter().find(|w| w.id == "venue").unwrap().state, WorkState::Completed);
	assert_eq!(p.works.iter().find(|w| w.id == "budget").unwrap().state, WorkState::Open);
	assert_eq!(p.results[0].requirements_id, "r1");
}

#[tokio::test]
async fn busy_request_and_lost_ack_survive_restart_without_false_acceptance() {
	let (home, store) = persistent_store();
	let path = home.path().join("work.db");

	agents(&store).await;
	apply(&store, "sage", "venue", WorkCommand::Create { work: work("venue", "sage") }).await;

	let command = transport_request();

	apply(&store, "sage", "request", command.clone()).await;
	apply(&store, "sage", "request", command).await;

	let events = store.list_agent_wake_events("iris".into(), 100).await.unwrap();

	assert_eq!(events.len(), 1);

	assert_unadmitted_response_fails(&store).await;

	store.begin_agent_dispatch_with_events("iris".into(), vec![events[0].id]).await.unwrap();

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();

	assert_eq!(
		store.work_portfolio(None).await.unwrap().collaborations[0].state,
		CollaborationState::Pending
	);
	assert!(
		store.begin_agent_dispatch_with_events("iris".into(), vec![events[0].id]).await.is_err()
	);

	store.reconcile_agent_dispatch("iris".into(), "exact-turn".into()).await.unwrap();

	apply(
		&store,
		"iris",
		"accept",
		WorkCommand::Respond {
			request_id: "transport".into(),
			instruction_id: "i1".into(),
			accept: true,
			note: "Will compare public routes".into(),
		},
	)
	.await;
	apply(
		&store,
		"iris",
		"return",
		WorkCommand::RecordResult { result: result("routes", "venue", "r1", "iris", Some("i1")) },
	)
	.await;
	apply(
		&store,
		"sage",
		"repair",
		WorkCommand::Repair {
			request_id: "transport".into(),
			expected_instruction: "i1".into(),
			instruction: instruction("i2"),
		},
	)
	.await;
	apply(
		&store,
		"iris",
		"late",
		WorkCommand::RecordResult {
			result: result("late-routes", "venue", "r1", "iris", Some("i1")),
		},
	)
	.await;
	assert_repaired_request(&store.work_portfolio(None).await.unwrap());

	assert!(
		store
			.apply_work_command(
				"sage".into(),
				"unfinished".into(),
				WorkCommand::Complete {
					work_id: "venue".into(),
					expected_requirements: "r1".into(),
					assessment: assessment("r1", &["routes"])
				},
				true
			)
			.await
			.is_err()
	);

	apply(
		&store,
		"sage",
		"cancel",
		WorkCommand::SetState {
			work_id: "venue".into(),
			expected_requirements: "r1".into(),
			state: WorkState::Cancelled,
			reason: Some("User cancelled venue research".into()),
			resume_condition: None,
			user_hold: false,
		},
	)
	.await;

	assert_eq!(
		store.get_agent_work_item("iris".into()).await.unwrap().active_turn_id.as_deref(),
		Some("exact-turn")
	);
}

#[tokio::test]
async fn user_holds_and_scoped_materials_do_not_grant_private_history() {
	let home = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&home.path().join("work.db")).unwrap();

	agents(&store).await;
	apply(&store, "sage", "work", WorkCommand::Create { work: work("venue", "sage") }).await;

	for id in ["public", "private"] {
		apply(
			&store,
			"sage",
			id,
			WorkCommand::AddMaterial {
				material: Material {
					id: id.into(),
					work_id: "venue".into(),
					title: id.into(),
					reference: format!("decision:{id}"),
					sha256: None,
				},
			},
		)
		.await;
	}

	let mut i = instruction("i1");

	i.material_ids = vec!["public".into()];

	apply(
		&store,
		"sage",
		"request",
		WorkCommand::Request {
			request: CollaborationRequest {
				id: "transport".into(),
				work_id: "venue".into(),
				recipient_agent_id: "iris".into(),
				instruction: i,
				state: CollaborationState::Pending,
				required: false,
				outcome: None,
				note: None,
			},
		},
	)
	.await;

	let p = store.agent_work_portfolio("iris".into(), None).await.unwrap();

	assert_eq!(p.materials.len(), 1);
	assert_eq!(p.materials[0].id, "public");
	assert!(
		store.agent_task_reference_target("iris".into(), "sage".into()).await.unwrap().is_none()
	);

	apply(
		&store,
		"sage",
		"hold",
		WorkCommand::SetState {
			work_id: "venue".into(),
			expected_requirements: "r1".into(),
			state: WorkState::Blocked,
			reason: Some("User paused".into()),
			resume_condition: Some("Explicit user release".into()),
			user_hold: true,
		},
	)
	.await;

	assert!(
		store
			.apply_work_command(
				"sage".into(),
				"auto-resume".into(),
				WorkCommand::SetState {
					work_id: "venue".into(),
					expected_requirements: "r1".into(),
					state: WorkState::Open,
					reason: None,
					resume_condition: None,
					user_hold: false
				},
				false
			)
			.await
			.is_err()
	);
}

#[tokio::test]
async fn native_binding_and_completion_intents_stay_separate_after_restart() {
	let home = tempfile::tempdir().unwrap();
	let path = home.path().join("work.db");
	let store = SqliteStore::open_test(&path).unwrap();

	agents(&store).await;
	apply(&store, "sage", "work", WorkCommand::Create { work: work("venue", "sage") }).await;
	apply(
		&store,
		"sage",
		"intent",
		WorkCommand::PrepareSubagent {
			contribution: NativeContribution {
				id: "native-routes".into(),
				work_id: "venue".into(),
				requirements_id: "r1".into(),
				instruction: "Check the accessible route".into(),
				child_thread_id: None,
				parent_turn_id: None,
				parent_item_id: None,
			},
		},
	)
	.await;

	store
		.record_native_work_dispatch(
			"native-routes".into(),
			"sage".into(),
			"parent-turn".into(),
			"spawn".into(),
		)
		.await
		.unwrap();

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();
	let p = store.work_portfolio(None).await.unwrap();

	assert!(p.native_contributions[0].child_thread_id.is_none());
	assert_eq!(p.works[0].state, WorkState::Open);
	assert!(store.native_work_delivery("native-routes".into()).await.unwrap().is_none());

	let delivery = NativeWorkDelivery {
		parent_thread_id: "sage".into(),
		parent_turn_id: "parent-turn".into(),
		child_thread_id: "child".into(),
		child_turn_id: "child-turn".into(),
		message_id: "native-message".into(),
		assignment_proof: Default::default(),
	};

	for _ in 0..2 {
		store.record_native_work_delivery("native-routes".into(), delivery.clone()).await.unwrap();
	}

	let mut changed = delivery.clone();

	changed.child_turn_id = "another-turn".into();

	assert!(store.record_native_work_delivery("native-routes".into(), changed).await.is_err());

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();

	assert_eq!(store.native_work_delivery("native-routes".into()).await.unwrap(), Some(delivery));
	assert!(
		store.native_contribution("native-routes".into()).await.unwrap().child_thread_id.is_none()
	);

	apply(
		&store,
		"sage",
		"binding",
		WorkCommand::BindSubagent {
			contribution_id: "native-routes".into(),
			parent_turn_id: "parent-turn".into(),
			parent_item_id: "spawn".into(),
			child_thread_id: "child".into(),
		},
	)
	.await;

	let mut r = result("native-result", "venue", "r1", "sage", None);

	r.native_contribution_id = Some("native-routes".into());

	apply(&store, "sage", "returned", WorkCommand::RecordResult { result: r }).await;
	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();
	let p = store.work_portfolio(None).await.unwrap();

	assert_eq!(p.results.len(), 1);
	assert_eq!(p.works[0].state, WorkState::Open);
	assert_eq!(p.native_contributions[0].child_thread_id.as_deref(), Some("child"));
}

#[tokio::test]
async fn global_guardian_unknown_writes_and_observation_opt_out_survive_restart() {
	let home = tempfile::tempdir().unwrap();
	let path = home.path().join("work.db");
	let store = SqliteStore::open_test(&path).unwrap();

	agents(&store).await;

	store.set_guardian_settings(1, ReviewPreference::AutomaticReview, true).await.unwrap();

	let mut source = GuardianSource {
		source_id: "native-generation-one".into(),
		agent_id: "sage".into(),
		thread_id: "sage".into(),
		revision: 2,
		outcome: "pending".into(),
		effective_reviewer: None,
		detail: "Reserved".into(),
	};

	assert!(store.reserve_guardian_source(source.clone()).await.unwrap());

	source.outcome = "unknown".into();

	store.observe_guardian_source(source.clone()).await.unwrap();

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();

	assert!(!store.reserve_guardian_source(source.clone()).await.unwrap());

	source.source_id = "native-generation-two".into();

	assert!(store.reserve_guardian_source(source).await.unwrap());
	assert_eq!(
		store.guardian_state().await.unwrap().settings.reviewer,
		ReviewPreference::AutomaticReview
	);

	let details = ReviewDetails {
		parent_thread_id: Some("sage".into()),
		truncated: false,
		reviewer_thread_id: "private".into(),
		reviewer_turn_id: "review-turn".into(),
		item_id: "output".into(),
		review_id: None,
		record: "{\"text\":\"synthetic review\"}".into(),
	};

	store.record_review_details(Some("sage".into()), details.clone()).await.unwrap();
	store.record_review_details(Some("sage".into()), details.clone()).await.unwrap();

	assert_eq!(store.guardian_state().await.unwrap().details.len(), 1);

	store.set_guardian_settings(2, ReviewPreference::AskMe, false).await.unwrap();

	let mut other = details;

	other.item_id = "later".into();

	store.record_review_details(Some("sage".into()), other).await.unwrap();

	assert_eq!(store.guardian_state().await.unwrap().details.len(), 1);
}

#[tokio::test]
async fn native_tool_replacement_keeps_creation_and_retirement_separate_after_restart() {
	let (home, store) = persistent_store();

	agents(&store).await;

	let path = home.path().join("work.db");
	let connection = Connection::open(&path).unwrap();

	connection
		.execute("UPDATE agent_tool_versions SET version=3 WHERE work_id IN('sage','iris')", [])
		.unwrap();

	drop(connection);

	assert!(
		store
			.reserve_agent_tool_migration("sage".into(), "sage".into(), "{}".into())
			.await
			.unwrap()
	);

	store.record_agent_tool_context("sage".into(), Some("replacement".into())).await.unwrap();

	assert!(
		store
			.finish_agent_tool_migration("sage".into(), "sage".into(), Some("replacement".into()))
			.await
			.is_err()
	);
	assert!(
		store
			.reserve_agent_tool_migration("iris".into(), "iris".into(), "{}".into())
			.await
			.unwrap()
	);

	store.record_agent_tool_context("iris".into(), None).await.unwrap();

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();

	assert_eq!(
		store.get_agent_work_item("sage".into()).await.unwrap().codex_thread_id.as_deref(),
		Some("sage")
	);
	assert_eq!(
		store.agent_tool_migration("sage".into()).await.unwrap().unwrap().1.as_deref(),
		Some("replacement")
	);
	assert!(
		!store
			.reserve_agent_tool_migration("iris".into(), "iris".into(), "{}".into())
			.await
			.unwrap(),
		"A missing creation receipt never permits another context"
	);

	store.retire_agent_tool_context("sage".into()).await.unwrap();
	store
		.finish_agent_tool_migration("sage".into(), "sage".into(), Some("replacement".into()))
		.await
		.unwrap();

	assert_eq!(store.agent_previous_threads("sage".into()).await.unwrap(), vec!["sage".to_owned()]);
	assert_eq!(store.agent_tool_version("sage".into()).await.unwrap(), 4);
}

async fn assert_unadmitted_response_fails(store: &SqliteStore) {
	assert!(
		store
			.apply_work_command(
				"iris".into(),
				"early".into(),
				WorkCommand::Respond {
					request_id: "transport".into(),
					instruction_id: "i1".into(),
					accept: true,
					note: "Ready".into()
				},
				false
			)
			.await
			.is_err()
	);
}
