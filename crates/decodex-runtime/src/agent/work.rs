//! Business coordination through the existing native input and inbox owners.
use super::{AgentCoordinator, AgentError, AgentWorkItem, Value, exact};
use decodex_core::work::{ResultSource, WorkCommand, WorkResult};
use sha2::{Digest as _, Sha256};

impl AgentCoordinator {
	pub(super) async fn work_tool(
		&mut self,
		agent: &AgentWorkItem,
		name: &str,
		args: &Value,
	) -> Result<Value, AgentError> {
		if name == "work_read" {
			if let Some(id) = args["nativeContributionId"].as_str() {
				return self.native_contribution_outputs(agent, id, args["cursor"].as_str()).await;
			}
			let work = args["workId"].as_str().map(str::to_owned);
			let portfolio = self.store.agent_work_portfolio(agent.id.clone(), work).await?;
			let turn = agent
				.active_turn_id
				.clone()
				.ok_or_else(|| AgentError::Invalid("Agent has no active turn".into()))?;
			return Ok(
				serde_json::json!({"portfolio":portfolio,"nativeIdentity":{"threadId":agent.codex_thread_id,"turnId":agent.active_turn_id},"publicOutputs":self.store.read_agent_transcript(agent.id.clone(),None,20).await?.1.into_iter().map(|output|serde_json::json!({"turnId":output.turn_id,"itemId":output.item_id,"kind":output.kind,"text":output.text,"truncated":output.truncated})).collect::<Vec<_>>(),"agents":self.store.agents().await?,"projects":self.store.projects().await?.into_iter().map(|p|serde_json::json!({"id":p.id,"name":p.name,"leadAgentId":p.lead_agent_id,"agentIds":p.agent_ids})).collect::<Vec<_>>(),"inbox":self.store.list_agent_events_for_turn(turn,100).await?,"decisionEvents":self.store.read_agent_transcript(agent.id.clone(),None,20).await?.0.into_iter().filter(|e|matches!(e.event_kind.as_str(),"user_message"|"async_question_answer")).map(|e|serde_json::json!({"eventId":e.id,"kind":e.event_kind,"deliveredTurnId":e.delivered_turn_id})).collect::<Vec<_>>(),"nativeExecution":"Use native spawn_agent and native messages for bounded helpers. Do not create a managed executor for a Work. Collaboration Request targets an existing independent Agent."}),
			);
		}
		let command: WorkCommand = serde_json::from_value(args["command"].clone())
			.map_err(|e| AgentError::Invalid(format!("invalid Work command: {e}")))?;
		let native_prompt = match &command {
			WorkCommand::PrepareSubagent { contribution } => Some(contribution.native_prompt()),
			_ => None,
		};
		let command_id = exact(args, "/commandId")?;
		if !self
			.store
			.work_command_applied(agent.id.clone(), command_id.clone(), command.clone())
			.await?
		{
			self.verify_work_command(&agent.id, &command).await?;
			self.store.apply_work_command(agent.id.clone(), command_id, command, false).await?;
		}
		Ok(
			serde_json::json!({"recorded":true,"nativePrompt":native_prompt,"completion":"Only a current owner assessment completes Work. Native delivery, acceptance and returned evidence remain separate."}),
		)
	}

	pub(crate) async fn verify_work_command(
		&self,
		actor: &str,
		command: &WorkCommand,
	) -> Result<(), AgentError> {
		if let WorkCommand::Complete { work_id, assessment, .. } = command {
			let portfolio =
				self.store.agent_work_portfolio(actor.into(), Some(work_id.clone())).await?;
			if !portfolio.works.iter().any(|w| w.owner_agent_id == actor) {
				return Err(AgentError::Rejected("Work belongs to another owner".into()));
			}
			if assessment.result_ids.is_empty() || assessment.result_ids.len() > 100 {
				return Err(AgentError::Invalid("Select related current Result evidence".into()));
			}
			for id in &assessment.result_ids {
				let result = self.store.work_result(work_id.clone(), id.clone()).await?;
				self.verify_work_result(&result.agent_id, &result).await?;
			}
		}
		if let WorkCommand::RecordResult { result } = command {
			self.verify_work_result(actor, result).await?;
		}
		if let WorkCommand::BindSubagent {
			contribution_id,
			parent_turn_id,
			parent_item_id,
			child_thread_id,
		} = command
		{
			let b = self.store.native_contribution(contribution_id.clone()).await?;
			let portfolio =
				self.store.agent_work_portfolio(actor.into(), Some(b.work_id.clone())).await?;
			if !portfolio
				.works
				.iter()
				.any(|w| w.owner_agent_id == actor && w.requirements.id == b.requirements_id)
			{
				return Err(AgentError::Rejected(
					"Contribution belongs to another owner or requirements revision".into(),
				));
			}
			let parent = self
				.store
				.get_agent_work_item(actor.into())
				.await?
				.codex_thread_id
				.ok_or_else(|| AgentError::Rejected("Agent has no native thread".into()))?;
			let actual =
				super::native_subagents::request_owner(&self.store, &self.client, child_thread_id)
					.await?;
			if actual.id != actor {
				return Err(AgentError::Rejected("Native child belongs to another Agent".into()));
			}
			let history = self.client.thread_read_turn(&parent, parent_turn_id).await?;
			let dispatch = self.store.native_work_dispatch(contribution_id.clone()).await?;
			let delivered = dispatch.as_ref().is_some_and(|(root, turn, call)| {
				root == &parent && turn == parent_turn_id && call == parent_item_id
			});

			let found = history["thread"]["turns"].as_array().is_some_and(|turns| {
				turns.iter().any(|turn| {
					turn["id"] == *parent_turn_id
						&& turn["items"].as_array().is_some_and(|items| {
							items.iter().any(|item| {
								item["id"] == *parent_item_id
									&& assignment_activity(
										item,
										&parent,
										child_thread_id,
										&b.native_prompt(),
										delivered,
									)
							})
						})
				})
			});

			if history.pointer("/thread/id").and_then(Value::as_str) != Some(&parent) || !found {
				return Err(AgentError::Rejected("Exact native assignment evidence is unavailable. Reconcile history before another spawn.".into()));
			}
		}
		Ok(())
	}

	pub(crate) async fn verify_work_result(
		&self,
		agent: &str,
		result: &WorkResult,
	) -> Result<(), AgentError> {
		if result.agent_id != agent {
			return Err(AgentError::Rejected("Result contributor differs from caller".into()));
		}
		let owner = self.store.get_agent_work_item(agent.into()).await?;
		let root = owner
			.codex_thread_id
			.ok_or_else(|| AgentError::Rejected("Agent has no native conversation".into()))?;
		match &result.source {
			ResultSource::NativeItem { thread_id, turn_id, item_id } => {
				if thread_id != &root
					&& !self.store.agent_previous_threads(agent.into()).await?.contains(thread_id)
				{
					if self.verified_legacy_result(agent, result, thread_id, turn_id).await? {
						return self.verify_native_result_item(thread_id, turn_id, item_id).await;
					}
					let id = result.native_contribution_id.as_ref().ok_or_else(|| {
						AgentError::Rejected(
							"Native Subagent Results require a bound Work contribution".into(),
						)
					})?;
					let binding = self.store.native_contribution(id.clone()).await?;
					if binding.work_id != result.work_id
						|| binding.requirements_id != result.requirements_id
						|| binding.child_thread_id.as_ref() != Some(thread_id)
					{
						return Err(AgentError::Rejected(
							"Result differs from its native Work assignment".into(),
						));
					}
					let history = self.client.thread_read_turn(thread_id, turn_id).await?;
					let dispatch = self.store.native_work_dispatch(id.clone()).await?;
					let delivery = self.store.native_work_delivery(id.clone()).await?;
					if !history["thread"]["turns"].as_array().is_some_and(|turns| {
						turns.iter().any(|t| {
							t["id"] == *turn_id
								&& (assignment_turn(t, &binding.native_prompt())
									|| dispatch
										.as_ref()
										.is_some_and(|(_, parent, _)| t["rootTurnId"] == *parent))
								&& delivery.as_ref().is_none_or(|d| &d.child_turn_id == turn_id)
						})
					}) {
						return Err(AgentError::Rejected(
							"Result turn has no exact instruction for this Work contribution"
								.into(),
						));
					}
					let actual = super::native_subagents::history_owner(
						&self.store,
						&self.client,
						thread_id,
					)
					.await?;
					if actual.id != agent {
						return Err(AgentError::Rejected(
							"Result is not from this Agent or a genuine native Subagent".into(),
						));
					}
				}
				self.verify_native_result_item(thread_id, turn_id, item_id).await?;
			},
			ResultSource::Artifact { path, sha256 } => {
				let directory = self
					.store
					.work_directory(agent.into())
					.await?
					.unwrap_or_else(|| self.config.cwd.clone());
				let root = std::path::Path::new(&directory)
					.canonicalize()
					.map_err(|_| AgentError::Rejected("Agent directory is unavailable".into()))?;
				let path = std::path::Path::new(path)
					.canonicalize()
					.map_err(|_| AgentError::Rejected("Result artifact is unavailable".into()))?;
				if !path.starts_with(root) || !path.is_file() {
					return Err(AgentError::Rejected(
						"Result artifact is outside this Agent's directory".into(),
					));
				}
				let file = std::fs::File::open(path)
					.map_err(|_| AgentError::Rejected("Result artifact is unavailable".into()))?;
				let mut digest = Sha256::new();
				let mut reader = std::io::BufReader::new(file);
				let mut buffer = [0u8; 65536];
				loop {
					let count = std::io::Read::read(&mut reader, &mut buffer).map_err(|_| {
						AgentError::Rejected("Result artifact could not be read".into())
					})?;
					if count == 0 {
						break;
					}
					digest.update(&buffer[..count]);
				}
				let actual: String = digest.finalize().iter().map(|b| format!("{b:02x}")).collect();
				if &actual != sha256 {
					return Err(AgentError::Rejected("Result artifact version changed".into()));
				}
			},
		}
		Ok(())
	}
}

pub(super) fn tools() -> Vec<Value> {
	vec![
		serde_json::json!({"type":"function","name":"work_read","description":"Read your current Works, their requirements, Materials, Results and Collaboration Requests, plus the exact current delivered inbox. Before assigning, recovering or assessing, read this current context. Other Agents expose only the bounded contribution and explicitly shared Materials, never their private conversation.","inputSchema":{"type":"object","properties":{"workId":{"type":"string"},"nativeContributionId":{"type":"string"},"cursor":{"type":"string"}},"additionalProperties":false}}),
		serde_json::json!({"type":"function","name":"work_manage","description":"Manage Work and independent Agent Collaboration. Stable commandId deduplicates exact retries. Use the documented WorkCommand operation with current expected requirements. Create/revise cite source event:<id> from a current delivered user decision. Native Subagents remain native; Work creates no executor. A Result needs exact native item or a file digest; Complete needs related current evidence and handled required requests. Ask Me approval remains human-owned.","inputSchema":{"type":"object","properties":{"commandId":{"type":"string"},"command":{"type":"object"}},"required":["commandId","command"],"additionalProperties":false}}),
	]
}

impl AgentCoordinator {
	pub(super) async fn recover_native_contribution(
		&self,
		thread: &str,
		turn: &str,
		item: &Value,
	) -> Result<(), AgentError> {
		if !((item["type"] == "collabAgentToolCall" && item["status"] == "completed")
			|| (item["type"] == "subAgentActivity"
				&& matches!(item["kind"].as_str(), Some("started" | "interacted" | "completed"))))
		{
			return Ok(());
		}

		let Some(owner) = self
			.store
			.list_agent_work_items()
			.await?
			.into_iter()
			.find(|a| a.codex_thread_id.as_deref() == Some(thread))
		else {
			return Ok(());
		};
		let child = if item["type"] == "subAgentActivity" {
			item["agentThreadId"].as_str()
		} else {
			item["receiverThreadIds"]
				.as_array()
				.filter(|ids| ids.len() == 1)
				.and_then(|ids| ids[0].as_str())
		};
		let Some(child) = child else { return Ok(()) };
		let portfolio = self.store.agent_work_portfolio(owner.id.clone(), None).await?;
		let mut matches = Vec::new();
		for c in &portfolio.native_contributions {
			if c.child_thread_id.is_some() {
				continue;
			}
			let dispatch = self.store.native_work_dispatch(c.id.clone()).await?;
			let call = if item["prompt"] == c.native_prompt()
				|| dispatch.as_ref().is_some_and(|(root, parent_turn, call)| {
					root == thread && parent_turn == turn && item["id"] == *call
				}) {
				Some(super::exact(item, "/id")?)
			} else {
				self.native_delivery_activity(&c.id, thread, turn, child)
					.await?
					.and_then(|activity| activity["id"].as_str().map(str::to_owned))
			};
			if let Some(call) = call {
				matches.push((c, call));
			}
		}

		if matches.len() != 1 {
			return Ok(());
		}
		let (contribution, call) = &matches[0];
		let command = WorkCommand::BindSubagent {
			contribution_id: contribution.id.clone(),
			parent_turn_id: turn.into(),
			parent_item_id: call.clone(),
			child_thread_id: child.into(),
		};
		if self.verify_work_command(&owner.id, &command).await.is_ok() {
			self.store
				.apply_work_command(
					owner.id,
					format!("native-binding:{}", contribution.id),
					command,
					false,
				)
				.await?;
		}
		Ok(())
	}
}

impl AgentCoordinator {
	async fn verify_native_result_item(
		&self,
		thread_id: &str,
		turn_id: &str,
		item_id: &str,
	) -> Result<(), AgentError> {
		let history = self.client.thread_read_turn(thread_id, turn_id).await?;
		let exact = history.pointer("/thread/id").and_then(Value::as_str) == Some(thread_id)
			&& history.pointer("/thread/turns").and_then(Value::as_array).is_some_and(|turns| {
				turns.iter().any(|t| {
					t["id"] == turn_id
						&& t["items"].as_array().is_some_and(|items| {
							items.iter().any(|i| {
								i["id"] == item_id
									&& matches!(
										i["type"].as_str(),
										Some(
											"agentMessage"
												| "plan"
												| "fileChange"
												| "commandExecution"
												| "mcpToolCall"
												| "webSearch"
										)
									)
							})
						})
				})
			});
		if !exact {
			return Err(AgentError::Rejected("Exact native Result item was not available".into()));
		}
		Ok(())
	}

	async fn verified_legacy_result(
		&self,
		actor: &str,
		result: &WorkResult,
		thread: &str,
		turn: &str,
	) -> Result<bool, AgentError> {
		let portfolio =
			self.store.agent_work_portfolio(actor.into(), Some(result.work_id.clone())).await?;
		let Some(work) = portfolio.works.first() else { return Ok(false) };
		let Some(event) = work
			.requirements
			.source
			.strip_prefix("legacy-instruction:")
			.and_then(|s| s.parse::<i64>().ok())
		else {
			return Ok(false);
		};
		if result.requirements_id != work.requirements.id
			|| result.native_contribution_id.is_some()
			|| result.instruction_id.is_some()
		{
			return Ok(false);
		}
		let instruction = self.store.get_agent_inbox_event(event).await?;
		let legacy = self.store.get_agent_work_item(work.id.clone()).await?;
		let all = self.store.list_agent_work_items().await?;
		let managers = self.store.agent_manager_ids().await?;
		Ok(legacy.kind == super::AgentWorkKind::Task
			&& legacy.codex_thread_id.as_deref() == Some(thread)
			&& instruction.delivered_turn_id.as_deref() == Some(turn)
			&& super::belongs_to(&legacy, actor, &all, &managers))
	}
}

/// Match the actual native user input, never a quotation in an assistant answer.
fn assignment_turn(turn: &Value, prompt: &str) -> bool {
	turn["items"].as_array().is_some_and(|items| {
		items.iter().any(|i| {
			i["type"] == "userMessage"
				&& i["content"].as_array().is_some_and(|parts| {
					parts.iter().any(|p| p["type"] == "text" && p["text"] == prompt)
				})
		})
	})
}
impl AgentCoordinator {
	async fn native_contribution_outputs(
		&self,
		agent: &AgentWorkItem,
		id: &str,
		cursor: Option<&str>,
	) -> Result<Value, AgentError> {
		let mut binding = self.store.native_contribution(id.into()).await?;
		let portfolio = self
			.store
			.agent_work_portfolio(agent.id.clone(), Some(binding.work_id.clone()))
			.await?;
		if !portfolio.works.iter().any(|w| w.owner_agent_id == agent.id) {
			return Err(AgentError::Rejected(
				"Native contribution belongs to another Agent".into(),
			));
		}
		let delivery = self.store.native_work_delivery(binding.id.clone()).await?;
		if binding.child_thread_id.is_none()
			&& let Some(d) = &delivery
			&& let Some(activity) = self
				.native_delivery_activity(
					&binding.id,
					&d.parent_thread_id,
					&d.parent_turn_id,
					&d.child_thread_id,
				)
				.await?
		{
			self.recover_native_contribution(&d.parent_thread_id, &d.parent_turn_id, &activity)
				.await?;
			binding = self.store.native_contribution(id.into()).await?;
		}
		let dispatch = self.store.native_work_dispatch(binding.id.clone()).await?;
		let Some(child) = binding.child_thread_id.as_ref() else {
			return Ok(
				serde_json::json!({"contribution":binding,"dispatch":dispatch,"delivery":delivery,"outputs":[],"limits":"Child binding is unconfirmed. Reconcile exact native activity before another spawn."}),
			);
		};

		if super::native_subagents::history_owner(&self.store, &self.client, child).await?.id
			!= agent.id
		{
			return Err(AgentError::Rejected("Native child ownership changed".into()));
		}
		let page = self.client.thread_history_page(child, cursor, 5).await?;
		let mut outputs = Vec::new();
		for turn in page["turns"].as_array().into_iter().flatten().filter(|t| {
			delivery.as_ref().is_none_or(|d| t["id"] == d.child_turn_id)
				&& (assignment_turn(t, &binding.native_prompt())
					|| dispatch.as_ref().is_some_and(|(_, parent, _)| t["rootTurnId"] == *parent))
		}) {
			for item in turn["items"].as_array().into_iter().flatten().filter(|i| {
				matches!(
					i["type"].as_str(),
					Some(
						"agentMessage"
							| "plan"
							| "fileChange"
							| "commandExecution"
							| "mcpToolCall"
							| "webSearch"
					)
				)
			}) {
				outputs.push(serde_json::json!({"threadId":child,"turnId":turn["id"],"itemId":item["id"],"kind":item["type"],"status":turn["status"],"text":item["text"].as_str().map(|s|s.chars().take(4096).collect::<String>())}));
			}
		}
		Ok(
			serde_json::json!({"contribution":binding,"outputs":outputs,"nextCursor":page["nextCursor"],"limits":"Exact native instruction-turn attribution only. Output text is a display preview; read the exact native item or artifact before assessment. A native completed turn does not complete Work."}),
		)
	}
}

pub(super) fn retired_tool(name: &str) -> Option<AgentError> {
	["agent_create_work","agent_create_manager","agent_create_goal"].contains(&name).then(||AgentError::Rejected("Use native spawn_agent for bounded execution. Persistent Agents are created only by the user. Work is an outcome, not an executor.".into()))
}

fn assignment_activity(
	item: &Value,
	parent: &str,
	child: &str,
	prompt: &str,
	delivered: bool,
) -> bool {
	if item["type"] == "collabAgentToolCall" {
		item["status"] == "completed"
			&& matches!(
				item["tool"].as_str(),
				Some("spawnAgent" | "sendInput" | "followupTask" | "sendMessage")
			)
			&& item["senderThreadId"] == parent
			&& item["receiverThreadIds"]
				.as_array()
				.is_some_and(|ids| ids.iter().any(|id| id == child))
			&& item["prompt"] == prompt
	} else {
		item["type"] == "subAgentActivity"
			&& matches!(item["kind"].as_str(), Some("started" | "interacted" | "completed"))
			&& item["agentThreadId"] == child
			&& delivered
	}
}

impl AgentCoordinator {
	pub(super) async fn observe_native_work_dispatch(
		&self,
		params: &Value,
	) -> Result<(), AgentError> {
		let item = &params["item"];
		if item["type"] == "agent_message" {
			return self.observe_native_work_delivery(params).await;
		}
		if item["type"] != "function_call"
			|| !matches!(
				item["name"].as_str(),
				Some("spawn_agent" | "send_input" | "send_message" | "followup_task")
			) {
			return Ok(());
		}
		let Some(thread) = params["threadId"].as_str() else { return Ok(()) };
		let Some(turn) = params["turnId"].as_str() else { return Ok(()) };
		let Some(arguments) = item["arguments"].as_str().filter(|s| s.len() <= 65536) else {
			return Ok(());
		};
		let Ok(args) = serde_json::from_str::<Value>(arguments) else { return Ok(()) };
		let Some(prompt) = args["message"].as_str() else { return Ok(()) };
		let Some(owner) = self
			.store
			.list_agent_work_items()
			.await?
			.into_iter()
			.find(|a| a.codex_thread_id.as_deref() == Some(thread))
		else {
			return Ok(());
		};
		let portfolio = self.store.agent_work_portfolio(owner.id, None).await?;
		for contribution in
			portfolio.native_contributions.iter().filter(|c| c.native_prompt() == prompt)
		{
			self.store
				.record_native_work_dispatch(
					contribution.id.clone(),
					thread.into(),
					turn.into(),
					super::exact(item, "/call_id")?,
				)
				.await?;
		}
		Ok(())
	}
}
