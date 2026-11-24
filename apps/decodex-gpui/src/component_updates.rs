//! The desktop checks opted-in components while running. The service owns Work and delivery.
use decodex_components::ragflow::Update;
use decodex_protocol::{
	AgentActionDto, AgentClient, AgentCommandResponse, ClientProfile, EntityId, IdempotencyKey,
	work::{Work, WorkCommand, WorkRequirements, WorkState},
};
use std::time::{Duration, Instant};

use crate::ui_preferences;

pub(super) struct Schedule {
	key: &'static str,
	pub enabled: bool,
	next: Option<Instant>,
}

impl Schedule {
	pub fn new(key: &'static str) -> Self {
		Self {
			key,
			enabled: ui_preferences::string(key, None).as_deref() == Some("enabled"),
			next: None,
		}
	}

	pub fn set(&mut self, enabled: bool) {
		ui_preferences::string(self.key, Some(if enabled { "enabled" } else { "disabled" }));
		self.enabled = enabled;
		self.next = None;
	}

	pub fn due(&self) -> bool {
		self.enabled && self.next.is_none_or(|next| Instant::now() >= next)
	}

	pub fn check_now(&mut self) {
		self.next = None;
	}

	pub fn checked(&mut self) {
		self.next = Some(Instant::now() + Duration::from_secs(6 * 3600));
	}
}

pub(super) fn work_id(target: &Update) -> String {
	format!("ragflow-update-{}", target.digest.trim_start_matches("sha256:"))
}

pub(super) fn review_assignment(owner: &str, target: &Update) -> Work {
	let id = work_id(target);
	Work {
		requirements: WorkRequirements {
			id: format!("{id}-r1"),
			goal: format!("Review the RAGFlow migration from installed version {} to infiniflow/ragflow@{}. Use the upstream release notes and migration guidance. Identify compatibility, backup and rollback steps for Assistant Memory. This assignment authorizes analysis only. Leave all components, configuration, containers and data untouched.", target.version, target.digest),
			criteria: "Save a Result with sources, migration steps, relevant unknowns and an update recommendation. Ask the user to apply the update from Settings after reviewing the plan.".into(),
			source: "Settings > Components > RAGFlow > Review updates with Assistant".into(),
		},
		id,
		owner_agent_id: owner.into(),
		project_id: None,
		title: format!("Review RAGFlow update ({})", &target.digest[7..19]),
		state: WorkState::Open,
		reason: None,
		resume_condition: None,
		user_hold: false,
		assessment: None,
	}
}

pub(super) fn assign_review(
	profile: ClientProfile,
	owner: &str,
	target: &Update,
) -> Result<(), String> {
	let work = review_assignment(owner, target);
	let key = IdempotencyKey::new(work.id.clone()).map_err(|_| "Invalid review identity.")?;
	let action = AgentActionDto::ManageWork {
		agent_id: EntityId::new(owner).map_err(|_| "Invalid Assistant identity.")?,
		command: WorkCommand::Assign { work },
	};
	let runtime = tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
		.map_err(|e| e.to_string())?;
	let client = AgentClient::new(profile);
	let id = EntityId::new(key.as_str()).map_err(|_| "Invalid review identity.")?;
	if let Ok(portfolio) = runtime.block_on(client.work_detail(id))
		&& portfolio.works.iter().any(|w| w.id == key.as_str())
	{
		return Ok(());
	}
	match runtime.block_on(client.execute(action, key)) {
		Ok(AgentCommandResponse::Accepted { .. }) => Ok(()),
		Ok(AgentCommandResponse::Rejected { error }) =>
			Err(format!("Assistant could not accept the review ({error:?}).")),
		_ => Err("Review delivery is unconfirmed. Check the connection and try again.".into()),
	}
}
