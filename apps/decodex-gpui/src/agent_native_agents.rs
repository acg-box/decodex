//! Observe native descendants without creating duplicate local workers.
use std::{
	collections::{BTreeMap, BTreeSet},
	time::{Duration, Instant},
};

use gpui::{AnyElement, AppContext as _, StatefulInteractiveElement as _};
use tokio::runtime::Builder;
use ui_theme::TREE_ROW_HEIGHT;

#[cfg(test)] use crate::shell::agent_surface::{AgentSnapshotResult, ClientProfile};
use crate::{
	shell::{
		agent_surface,
		agent_surface::{
			AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
			ComposerInput, Context, Entity, EntityId, HistoryText, IdempotencyKey, IntoElement,
			ParentElement, SharedString, Styled, Task, WireText, agent_tree,
			agent_tree::DISCLOSURE, ui_theme,
		},
	},
	ui_motion,
};
use decodex_protocol::{NativeAgentDto, NativeAgentsResult};

#[derive(Default)]
pub(super) struct NativeAgents {
	pub lists: BTreeMap<String, Vec<NativeAgentDto>>,
	pub selected: Option<(String, String)>,
	pub pages: BTreeMap<String, (String, String)>,
	pub detail: Option<NativeAgentsResult>,
	pub connection: NativeConnection,
	parent_timeline: Option<Box<super::TimelineView>>,
	pub timelines: BTreeMap<(String, String), super::TimelineView>,
	task: Option<Task<()>>,
	detail_task: Option<Task<()>>,
	next: Option<Instant>,
	next_detail: Option<Instant>,
	input: Option<Entity<ComposerInput>>,
	editor: Option<NativeAgentTarget>,
	drafts: BTreeMap<NativeAgentTarget, String>,
	feedback: String,
	pending: Option<NativeAgentTarget>,
	uncertain: BTreeSet<NativeAgentTarget>,
	send_task: Option<Task<()>>,
}

#[derive(Clone, Debug)]
pub(super) enum NativeConnection {
	Checking { started: Instant, resumed: bool },
	Ready,
	// Codex multi-agent v2 children reject direct input; unknown capability is separate.
	ParentManaged,
	Failed(String),
}
impl Default for NativeConnection {
	fn default() -> Self {
		Self::Checking { started: Instant::now(), resumed: false }
	}
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct NativeAgentTarget {
	profile: String,
	source: Option<String>,
	root: String,
	work: String,
	thread: String,
}

impl AgentSurface {
	/// A conversation has one presentation regardless of which agent owns execution.
	pub(super) fn conversation_work(&self) -> Option<super::AgentWorkItemDto> {
		let mut work = self
			.snapshot
			.as_ref()?
			.work_items
			.iter()
			.find(|w| Some(&w.id) == self.selected.as_ref())?
			.clone();
		if let Some((owner, thread)) = &self.native_agents.selected {
			if &work.id != owner {
				return None;
			}
			if work.codex_thread_id.as_ref() == Some(thread) {
				return Some(work);
			}
			work.codex_thread_id = Some(thread.clone());
			work.title = self
				.native_agents
				.lists
				.get(owner)
				.into_iter()
				.flatten()
				.find(|a| &a.thread_id == thread)
				.map(|a| a.title.clone())
				.unwrap_or_else(|| work.title.clone());
			work.active_turn_id = match &self.native_agents.detail {
				Some(NativeAgentsResult::Conversation { active_turn, .. }) => active_turn.clone(),
				_ => None,
			};
			work.dispatch_state = if work.active_turn_id.is_some() {
				super::AgentDispatchStateDto::Running
			} else {
				super::AgentDispatchStateDto::Idle
			};
		}
		Some(work)
	}

	pub(super) fn conversation_page(&self) -> Option<String> {
		self.native_agents
			.selected
			.as_ref()
			.and_then(|target| {
				self.native_agents
					.pages
					.iter()
					.find(|(_, t)| *t == target)
					.map(|(id, _)| id.clone())
			})
			.or_else(|| self.selected.clone())
	}

	pub(super) fn native_page_label(&self, id: &str) -> Option<String> {
		let (owner, thread) = self.native_agents.pages.get(id)?;
		Some(
			self.native_agents
				.lists
				.get(owner)
				.into_iter()
				.flatten()
				.find(|a| &a.thread_id == thread)
				.map(|a| a.title.clone())
				.unwrap_or_else(|| "Agent".into()),
		)
	}

	pub(super) fn conversation_matches(&self, work: &str, thread: &str) -> bool {
		self.conversation_work()
			.is_some_and(|w| w.id == work && w.codex_thread_id.as_deref() == Some(thread))
	}

	pub(super) fn conversation_composer(&self) -> &Entity<ComposerInput> {
		if self.native_agents.selected.is_some() {
			self.native_agents.input.as_ref().unwrap_or(&self.composer)
		} else {
			&self.composer
		}
	}

	pub(super) fn native_feedback(&self) -> &str {
		&self.native_agents.feedback
	}

	pub(super) fn native_input_available(&self) -> bool {
		self.command_connection_ready()
			&& !self.connection_initializing()
			&& self.native_agents.pending.is_none()
			&& self.native_agents.editor.as_ref().is_some_and(|t| {
				!self.native_agents.uncertain.contains(t)
					&& self
						.native_agents
						.selected
						.as_ref()
						.and_then(|(o, t)| self.native_agent_target(o, t))
						.as_ref()
						== Some(t)
			})
			&& matches!(
				&self.native_agents.detail,
				Some(NativeAgentsResult::Conversation { thread_id, can_input: Some(true), .. })
				if self.native_agents.selected.as_ref().is_some_and(|(_, t)| t == thread_id)
			)
	}

	fn native_agent_target(&self, owner: &str, thread: &str) -> Option<NativeAgentTarget> {
		let snapshot = self.snapshot.as_ref()?;
		let work = snapshot.work_items.iter().find(|work| work.id == owner)?;

		Some(NativeAgentTarget {
			profile: self.profile.as_ref()?.draft_scope_key(),
			source: snapshot.runtime_source.as_ref().map(|source| source.as_str().into()),
			root: work.codex_thread_id.clone()?,
			work: owner.into(),
			thread: thread.into(),
		})
	}

	pub(super) fn reset_native_agents(&mut self) {
		self.native_agents.task = None;
		self.native_agents.detail_task = None;

		self.native_agents.lists.clear();

		self.native_agents.detail = None;
		self.native_agents.connection = NativeConnection::default();
		self.native_agents.next = None;
		self.native_agents.next_detail = None;
		self.native_agents.send_task = None;

		if let Some(target) = self.native_agents.pending.take() {
			self.native_agents.uncertain.insert(target);

			self.native_agents.feedback = "Delivery was not confirmed. Inspect the previous conversation before sending again.".into();
		}
	}

	pub(super) fn invalidate_native_agents(&mut self, next: &AgentSnapshotDto) {
		if self.snapshot.as_ref().is_some_and(|previous| {
			previous.runtime_source != next.runtime_source
				|| previous.work_items.iter().any(|old| {
					old.codex_thread_id.is_some()
						&& next
							.work_items
							.iter()
							.find(|new| new.id == old.id)
							.is_none_or(|new| new.codex_thread_id != old.codex_thread_id)
				})
		}) {
			self.reset_native_agents();

			self.restore_native_parent();
			self.native_agents.timelines.clear();
			self.workspace.pages.retain(|p| !self.native_agents.pages.contains_key(p));
			self.native_agents.pages.clear();
		}
	}

	fn restore_native_parent(&mut self) {
		if let Some(target) = self.native_agents.selected.take()
			&& let Some(parent) = self.native_agents.parent_timeline.take()
		{
			self.timeline.native.task = None;
			let child = std::mem::replace(&mut self.timeline, *parent);
			self.native_agents.timelines.insert(target, child);
		}
	}

	pub(super) fn close_native_agent(&mut self, cx: &mut Context<Self>) {
		if let (Some(target), Some(input)) = (&self.native_agents.editor, &self.native_agents.input)
		{
			self.native_agents.drafts.insert(target.clone(), input.read(cx).content().into());
		}

		self.restore_native_parent();
		self.native_agents.detail = None;
		self.native_agents.detail_task = None;
		self.native_agents.next_detail = None;
	}

	pub(super) fn poll_native_agents(&mut self, cx: &mut Context<Self>) {
		if self.native_agents.selected.is_some() {
			if !self.command_connection_ready()
				&& matches!(self.native_agents.connection, NativeConnection::Ready)
			{
				self.native_agents.connection = NativeConnection::default();
				self.native_agents.detail = None;
			}
			if matches!(self.native_agents.connection, NativeConnection::Checking { started, .. } if started.elapsed() > Duration::from_secs(45))
			{
				self.native_agents.detail_task = None;
				self.native_agents.detail = None;
				self.native_agents.connection =
					NativeConnection::Failed("Connecting took too long. Try again.".into());
				cx.notify();
			}
		}
		if !self.command_connection_ready() || self.connection_initializing() {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			return;
		};

		if (self.workspace.agent_tree_visible
			|| self.workspace.browsing
			|| self.workspace.graph_visible)
			&& self.native_agents.task.is_none()
			&& self.native_agents.next.is_none_or(|t| t <= Instant::now())
		{
			let owners: Vec<_> = self
				.snapshot
				.iter()
				.flat_map(|s| s.work_items.iter())
				.filter(|w| w.codex_thread_id.is_some())
				.map(|w| w.id.clone())
				.collect();

			if !owners.is_empty() {
				let read = cx.background_executor().spawn(async move {
					let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
						return Vec::new();
					};

					runtime.block_on(async move {
						let client = AgentClient::new(profile);
						let mut result = Vec::new();

						for owner in owners {
							let Ok(id) = EntityId::new(owner.clone()) else {
								continue;
							};
							let mut list = Vec::new();
							let mut cursor = None;
							let mut complete = false;

							for _ in 0..10 {
								match client.native_agents(id.clone(), None, cursor).await {
									Ok(NativeAgentsResult::Available { agents, next_cursor }) => {
										list.extend(agents);

										if next_cursor.is_none() {
											complete = true;

											break;
										}

										cursor = next_cursor.and_then(|c| WireText::new(c).ok());

										if cursor.is_none() {
											break;
										}
									},
									_ => break,
								}
							}

							if complete {
								result.push((owner, Some(list)));
							} else {
								result.push((owner, None));
							}
						}

						result
					})
				});

				self.native_agents.task = Some(cx.spawn(async move |surface, cx| {
					let result = read.await;
					let _ = surface.update(cx, |s, cx| {
						let mut changed = false;

						for (owner, list) in result {
							let Some(list) = list else {
								if let Some(previous) = s.native_agents.lists.get_mut(&owner) {
									for agent in previous {
										changed |= agent.status != "unknown";
										agent.status = "unknown".into();
									}
								}
								continue;
							};
							if s.native_agents.lists.get(&owner) != Some(&list) {
								s.native_agents.lists.insert(owner, list);

								changed = true;
							}
						}

						s.native_agents.task = None;
						s.native_agents.next = Some(Instant::now() + Duration::from_secs(5));

						if changed {
							cx.notify();
						}
					});
				}));
			}
		}
		if self.native_agents.selected.is_some()
			&& matches!(
				self.native_agents.connection,
				NativeConnection::Checking { .. } | NativeConnection::Ready
			)
			&& self.native_agents.detail_task.is_none()
			&& self.native_agents.next_detail.is_none_or(|t| t <= Instant::now())
		{
			self.read_native_agent(cx);
		}
	}

	pub(super) fn open_native_agent(&mut self, owner: &str, thread: &str, cx: &mut Context<Self>) {
		self.workspace.browsing = false;
		if self.native_agents.pending.is_some() || !self.command_connection_ready() {
			return;
		}

		if self.native_agents.selected.as_ref() == Some(&(owner.into(), thread.into())) {
			return;
		}
		self.open_page(owner, cx);
		if self.native_agents.selected.as_ref() == Some(&(owner.into(), thread.into())) {
			return;
		}
		self.enter_native_conversation(owner, thread, cx);
	}

	pub(super) fn enter_native_conversation(
		&mut self,
		owner: &str,
		thread: &str,
		cx: &mut Context<Self>,
	) {
		let Some(target) = self.native_agent_target(owner, thread) else {
			return;
		};
		self.close_native_agent(cx);
		self.stop_voice(cx);
		self.reset_recap();
		self.composer_menu = None;
		self.composer_menu_content = None;

		self.history_task = None;
		self.older_task = None;
		self.timeline.native.task = None;
		let view =
			self.native_agents.timelines.remove(&(owner.into(), thread.into())).unwrap_or_default();
		self.native_agents.parent_timeline =
			Some(Box::new(std::mem::replace(&mut self.timeline, view)));
		self.native_agents.selected = Some((owner.into(), thread.into()));
		self.native_agents.connection = NativeConnection::default();
		if target.root != thread {
			let page = format!("native:{owner}:{thread}");
			self.native_agents.pages.insert(page.clone(), (owner.into(), thread.into()));
			self.workspace.closing_pages.remove(&page);
			if !self.workspace.pages.contains(&page) {
				self.workspace.pages.push(page);
			}
		}

		self.native_agents.detail = None;
		self.native_agents.feedback = if self.native_agents.uncertain.contains(&target) {
			"A previous message has an unconfirmed outcome. Inspect its history before continuing."
				.into()
		} else {
			String::new()
		};
		self.native_agents.next_detail = None;

		let input = self
			.native_agents
			.input
			.get_or_insert_with(|| {
				let input = cx.new(|cx| {
					ComposerInput::message(35, super::prompts::session_quote(), "Agent message", cx)
				});
				cx.observe(&input, |_, _, cx| cx.notify()).detach();
				input
			})
			.clone();
		let draft = self.native_agents.drafts.get(&target).cloned().unwrap_or_default();

		self.native_agents.editor = Some(target);

		input.update(cx, |i, cx| i.set_content(&draft, cx));
		self.refresh_open_native_history(cx);
		self.read_native_agent(cx);
		cx.notify();
	}

	fn read_native_agent(&mut self, cx: &mut Context<Self>) {
		if !self.command_connection_ready() || self.connection_initializing() {
			return;
		}
		let (Some(profile), Some((owner, thread))) =
			(self.profile.clone(), self.native_agents.selected.clone())
		else {
			return;
		};
		let Some(target) = self.native_agent_target(&owner, &thread) else {
			return;
		};
		let read = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			runtime
				.block_on(AgentClient::new(profile).native_agents(
					EntityId::new(owner).ok()?,
					Some(WireText::new(thread).ok()?),
					None,
				))
				.ok()
		});
		self.native_agents.detail_task = Some(cx.spawn(async move |surface, cx| {
            let result = read.await;
            let _ = surface.update(cx, |s, cx| {
                let current = s.native_agents.selected.as_ref().and_then(|(owner, thread)| s.native_agent_target(owner, thread));
                if current.as_ref() != Some(&target) || s.native_agents.editor.as_ref().is_some_and(|editor| editor != &target) {
                    return;
                }
                s.native_agents.detail_task = None;
                let result = result.filter(|result| matches!(result, NativeAgentsResult::Conversation { thread_id, .. } if thread_id == &target.thread));
                s.native_agents.next_detail = Some(Instant::now() + Duration::from_secs(3));
                // A missed background observation does not revoke a confirmed capability.
                // Actual sending still checks the live native capability in the service.
                if result.is_none() && matches!(s.native_agents.connection, NativeConnection::Ready) {
                    return;
                }
                let result = result.unwrap_or(NativeAgentsResult::Unavailable);
                s.native_agents.detail = Some(result.clone());
                match result {
                    NativeAgentsResult::Conversation { can_input: Some(true), .. } => {
                        s.native_agents.connection = NativeConnection::Ready;
                    },
                    NativeAgentsResult::Conversation { can_input: Some(false), .. } => {
                        s.native_agents.connection = NativeConnection::ParentManaged;
                    },
                    NativeAgentsResult::Conversation { can_input: None, .. } => {
                        if matches!(s.native_agents.connection, NativeConnection::Checking { resumed: true, .. }) {
                            s.native_agents.connection = NativeConnection::Failed("Could not confirm that this conversation is ready.".into());
                        } else {
                            s.prepare_native_connection(target, cx);
                        }
                    },
                    _ if matches!(s.native_agents.connection, NativeConnection::Checking { started, .. } if started.elapsed() < Duration::from_secs(2)) => {
                        s.native_agents.next_detail = Some(Instant::now() + Duration::from_millis(500));
                    },
                    _ => {
                        s.native_agents.connection = NativeConnection::Failed("Could not connect to this conversation.".into());
                    },
                }
                cx.notify();
            });
        }));
	}

	fn prepare_native_connection(&mut self, target: NativeAgentTarget, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let started = match self.native_agents.connection {
			NativeConnection::Checking { started, .. } => started,
			_ => Instant::now(),
		};
		self.native_agents.connection = NativeConnection::Checking { started, resumed: true };
		let request_target = target.clone();
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			runtime
				.block_on(AgentClient::new(profile).execute(
					AgentActionDto::PrepareNativeAgent {
						work_id: EntityId::new(request_target.work).ok()?,
						thread_id: WireText::new(request_target.thread).ok()?,
					},
					IdempotencyKey::new(agent_surface::unique_command()).ok()?,
				))
				.ok()
		});
		self.native_agents.detail_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.native_agents
					.selected
					.as_ref()
					.and_then(|(o, t)| s.native_agent_target(o, t))
					.as_ref()
					!= Some(&target)
				{
					return;
				}
				s.native_agents.detail_task = None;
				match result {
					Some(AgentCommandResponse::Accepted { .. }) => s.read_native_agent(cx),
					Some(AgentCommandResponse::Rejected {
						error: decodex_protocol::CommandError::ApplicationUnavailable { message },
					}) => {
						s.native_agents.connection = if message.as_str()
							== "Open the parent agent to reconnect this conversation."
						{
							NativeConnection::ParentManaged
						} else {
							NativeConnection::Failed(message.as_str().into())
						};
					},
					_ =>
						s.native_agents.connection = NativeConnection::Failed(
							"Could not connect to this conversation.".into(),
						),
				}
				cx.notify();
			});
		}));
	}

	pub(super) fn retry_native_connection(&mut self, cx: &mut Context<Self>) {
		if self.native_agents.detail_task.is_some() {
			return;
		}
		self.native_agents.detail = None;
		self.native_agents.connection = NativeConnection::default();
		self.native_agents.next_detail = None;
		self.read_native_agent(cx);
		cx.notify();
	}

	pub(super) fn open_native_parent(&mut self, cx: &mut Context<Self>) {
		let Some((owner, thread)) = self.native_agents.selected.clone() else {
			return;
		};
		let Some(work) =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == owner))
		else {
			return;
		};
		if work.codex_thread_id.as_ref() == Some(&thread) {
			if let Some(parent) = work.parent_goal_id.clone() {
				self.open_page(&parent, cx);
			}
		} else {
			let parent = self
				.native_agents
				.lists
				.get(&owner)
				.into_iter()
				.flatten()
				.find(|a| a.thread_id == thread)
				.map(|a| a.parent_thread_id.clone());
			if let Some(parent) = parent.filter(|p| work.codex_thread_id.as_ref() != Some(p)) {
				self.open_native_agent(&owner, &parent, cx);
			} else {
				self.open_page(&owner, cx);
			}
		}
	}

	pub(super) fn native_branches(
		&self,
		owner: &str,
		parent: &str,
		depth: usize,
		cx: &mut Context<Self>,
	) -> (AnyElement, usize) {
		let mut rows = gpui::div().flex().flex_col();
		let mut count = 0;

		if depth > 24 {
			return (rows.into_any_element(), 0);
		}

		for agent in self
			.native_agents
			.lists
			.get(owner)
			.into_iter()
			.flatten()
			.filter(|a| a.parent_thread_id == parent)
		{
			if self.snapshot.as_ref().is_some_and(|s| {
				s.work_items.iter().any(|w| w.codex_thread_id.as_deref() == Some(&agent.thread_id))
			}) {
				continue;
			}

			let work = owner.to_owned();
			let thread = agent.thread_id.clone();
			let label = agent.title.clone();
			let key = format!("native:{owner}:{}", agent.thread_id);
			let expanded = !self.workspace.agent_tree_collapsed.contains(&key);
			let has_children = self.native_agents.lists.get(owner).is_some_and(|list| {
				list.iter().any(|child| child.parent_thread_id == agent.thread_id)
			});
			let selected = self
				.native_agents
				.selected
				.as_ref()
				.is_some_and(|(o, t)| o == owner && t == &agent.thread_id);
			let row_work = work.clone();
			let row_thread = thread.clone();
			let row = agent_tree::tree_row(
				format!("native-agent-row-{}", agent.thread_id),
				depth,
				selected,
				has_children,
				expanded,
			)
			.on_click(
				cx.listener(move |s, _, _, cx| s.open_native_agent(&row_work, &row_thread, cx)),
			)
			.child(if has_children {
				self.tree_toggle(key.clone(), &label, expanded, cx)
			} else {
				gpui::div().w(gpui::px(DISCLOSURE)).flex_none().into_any_element()
			})
			.child(agent_tree::tree_identity(
				self.workspace_action(
					format!("native-agent-open-{thread}"),
					label,
					move |s, cx| s.open_native_agent(&work, &thread, cx),
					cx,
				),
				format!("native-agent-signal-{}", agent.thread_id),
				&agent.status,
			));
			let (children, n) = self.native_branches(owner, &agent.thread_id, depth + 1, cx);

			rows = rows.child(row).child(ui_motion::reveal(
				SharedString::from(format!("tree-children-{key}")),
				if expanded { n as f32 * TREE_ROW_HEIGHT } else { 0. },
				false,
				agent_tree::tree_children(depth).child(children),
			));
			count += 1 + if expanded { n } else { 0 };
		}

		(rows.into_any_element(), count)
	}

	pub(super) fn send_native_agent(&mut self, cx: &mut Context<Self>) {
		if !self.native_input_available() {
			return;
		}

		let (
			Some(profile),
			Some((owner, thread)),
			Some(input),
			Some(NativeAgentsResult::Conversation {
				thread_id: observed,
				can_input: Some(true),
				active_turn,
				..
			}),
		) = (
			self.profile.clone(),
			self.native_agents.selected.clone(),
			self.native_agents.input.clone(),
			self.native_agents.detail.clone(),
		)
		else {
			return;
		};
		let Some(target) = self.native_agent_target(&owner, &thread) else {
			return;
		};

		if observed != thread
			|| self.native_agents.editor.as_ref() != Some(&target)
			|| self.native_agents.uncertain.contains(&target)
		{
			return;
		}

		let text = input.read(cx).content().to_owned();

		if text.trim().is_empty() {
			return;
		}

		let (Ok(work_id), Ok(thread_id), Ok(message)) =
			(EntityId::new(owner), WireText::new(thread), HistoryText::new(text.clone()))
		else {
			return;
		};

		self.native_agents.pending = Some(target.clone());
		self.native_agents.feedback = "Sending…".into();

		let send = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime
				.block_on(AgentClient::new(profile).execute(
					AgentActionDto::NativeAgentInput {
						work_id,
						thread_id,
						text: message,
						expected_turn: active_turn.and_then(|t| WireText::new(t).ok()),
					},
					IdempotencyKey::new(agent_surface::unique_command()).ok()?,
				))
				.ok()
		});

		self.native_agents.send_task = Some(cx.spawn(async move |surface, cx| {
			let result = send.await;
			let _ =
				surface.update(cx, |s, cx| s.finish_native_agent_send(target, text, result, cx));
		}));

		cx.notify();
	}

	fn finish_native_agent_send(
		&mut self,
		target: NativeAgentTarget,
		text: String,
		result: Option<AgentCommandResponse>,
		cx: &mut Context<Self>,
	) {
		if self.native_agents.pending.as_ref() != Some(&target) {
			return;
		}

		self.native_agents.pending = None;
		self.native_agents.send_task = None;

		let feedback = match result {
			Some(AgentCommandResponse::Accepted { .. }) => {
				if self.native_agents.drafts.get(&target) == Some(&text) {
					self.native_agents.drafts.remove(&target);
				}
				if self.native_agents.editor.as_ref() == Some(&target)
					&& let Some(input) = &self.native_agents.input
					&& input.read(cx).content() == text
				{
					input.update(cx, |input, cx| input.set_content("", cx));
				}
				"Sent"
			},
			// AgentClient reports every failure after its send boundary as a response.
			None | Some(AgentCommandResponse::Rejected { .. }) =>
				"Message was not sent. Refresh the conversation and check its availability.",
			_ => {
				self.native_agents.uncertain.insert(target.clone());
				"Delivery was not confirmed. Inspect the conversation before sending again."
			},
		};

		if self
			.native_agents
			.selected
			.as_ref()
			.and_then(|(owner, thread)| self.native_agent_target(owner, thread))
			.as_ref()
			== Some(&target)
		{
			self.native_agents.feedback = feedback.into();
			self.native_agents.next_detail = None;
			self.refresh_open_native_history(cx);
		}

		cx.notify();
	}
}

#[cfg(test)]
mod tests {
	use std::future;

	use futures_util::{SinkExt as _, StreamExt as _};
	use gpui::AppContext as _;
	use tokio_tungstenite::tungstenite::Message;

	use crate::shell::agent_surface::{
		native_agents::{
			AgentActionDto, AgentCommandResponse, AgentSnapshotResult, AgentSurface, ClientProfile,
			ComposerInput, Context, EntityId, NativeAgentDto, NativeAgentsResult,
		},
		wire_test_support,
	};
	#[cfg(test)] use decodex_protocol::CommandPayload;
	use decodex_protocol::{
		CURRENT_VERSION, ClientMessage, QueryPayload, QueryResultEnvelope, QueryResultPayload,
		ServerId, ServerMessage,
	};

	fn conversation(thread: &str) -> NativeAgentsResult {
		NativeAgentsResult::Conversation {
			thread_id: thread.into(),
			can_input: Some(true),
			active_turn: None,
		}
	}

	#[gpui::test]
	fn unified_conversations_keep_parent_and_child_drafts_and_viewports_separate(
		cx: &mut gpui::TestAppContext,
	) {
		let (_root, profile, server) = wire_test_support::fixture(|_| async {});
		server.join().unwrap();
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			bind_fixture(s, profile, cx);
			s.composer.update(cx, |i, cx| i.set_content("Parent draft", cx));
			s.timeline.follow_paused.insert("parent-marker".into());
			s.enter_native_conversation("agent", "child-a", cx);
			assert!(s.workspace_connecting());
			s.timeline.native.binding =
				Some(crate::shell::agent_surface::native_timeline::Binding {
					work: "agent".into(),
					thread: "child-a".into(),
					account: "account".into(),
				});
			assert!(!s.workspace_connecting(), "retained history must not wait for send readiness");
			assert!(!s.native_input_available(), "retained content must not grant send permission");
			s.snapshot.as_mut().unwrap().connection_initializing = true;
			assert!(s.workspace_connecting(), "global connection still blocks the workspace");
			s.snapshot.as_mut().unwrap().connection_initializing = false;

			assert_eq!(s.conversation_work().unwrap().codex_thread_id.as_deref(), Some("child-a"));
			assert_eq!(s.conversation_page().as_deref(), Some("native:agent:child-a"));
			assert_eq!(s.navigation_work().as_deref(), Some("native:agent:child-a"));
			assert!(s.can_restore_work(s.navigation_work().as_deref()));
			assert!(!s.timeline.follow_paused.contains("parent-marker"));
			s.conversation_composer()
				.clone()
				.update(cx, |i, cx| i.set_content("Child A draft", cx));
			s.timeline.follow_paused.insert("child-a-marker".into());
			s.native_agents.detail = Some(conversation("child-a"));
			assert!(s.native_input_available());
			s.native_agents.detail = Some(conversation("foreign"));
			assert!(!s.native_input_available());
			s.enter_native_conversation("agent", "child-b", cx);
			assert_eq!(s.conversation_composer().read(cx).content(), "");
			assert_eq!(s.composer.read(cx).content(), "Parent draft");
			assert!(!s.timeline.follow_paused.contains("child-a-marker"));
			s.conversation_composer()
				.clone()
				.update(cx, |i, cx| i.set_content("Child B draft", cx));
			s.enter_native_conversation("agent", "child-a", cx);
			assert_eq!(s.conversation_composer().read(cx).content(), "Child A draft");
			assert!(s.timeline.follow_paused.contains("child-a-marker"));
			// Esc in a child conversation must never cancel its parent's turn.
			assert!(s.running_turn().is_none());
			s.close_native_agent(cx);
			assert_eq!(s.conversation_composer().read(cx).content(), "Parent draft");
			assert_eq!(s.conversation_work().unwrap().codex_thread_id.as_deref(), Some("parent"));
			assert!(s.timeline.follow_paused.contains("parent-marker"));
			s.enter_native_conversation("agent", "child-b", cx);
			assert_eq!(s.conversation_composer().read(cx).content(), "Child B draft");
			s.close_native_agent(cx);
		});
	}

	#[gpui::test]
	fn source_changes_invalidate_native_observations(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		for change in ["thread", "source", "removed", "disconnect", "failed", "profile"] {
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.snapshot
					.as_mut()
					.unwrap()
					.work_items
					.iter_mut()
					.find(|w| w.id == "agent")
					.unwrap()
					.codex_thread_id = Some("parent".into());
				s.native_agents.selected = Some(("agent".into(), "child".into()));
				s.native_agents.detail = Some(conversation("child"));

				s.native_agents.lists.insert(
					"agent".into(),
					vec![NativeAgentDto {
						task: String::new(),
						thread_id: "child".into(),
						parent_thread_id: "parent".into(),
						title: "Old child".into(),
						status: "idle".into(),
					}],
				);

				s.native_agents.task = Some(cx.spawn(async |_, _| future::pending::<()>().await));
				s.native_agents.detail_task =
					Some(cx.spawn(async |_, _| future::pending::<()>().await));

				let mut next = s.snapshot.clone().unwrap();

				match change {
					"thread" =>
						next.work_items
							.iter_mut()
							.find(|w| w.id == "agent")
							.unwrap()
							.codex_thread_id = Some("replacement".into()),
					"source" =>
						next.runtime_source = Some(EntityId::new("replacement-source").unwrap()),
					"removed" => next.work_items.retain(|w| w.id != "agent"),
					"disconnect" => s.mark_stale(cx),
					"failed" => s.apply_result(Err(())),
					"profile" => s.bind_profile(None, cx),
					_ => unreachable!(),
				}

				if matches!(change, "thread" | "source" | "removed") {
					s.apply_result(Ok(AgentSnapshotResult::Available(next)));
				}

				assert!(s.native_agents.detail.is_none(), "old input capability after {change}");
				assert!(s.native_agents.lists.is_empty(), "old tree after {change}");
				assert!(s.native_agents.task.is_none(), "old list request after {change}");
				assert!(s.native_agents.detail_task.is_none(), "old detail request after {change}");
			});
		}
	}
	fn bind_fixture(s: &mut AgentSurface, profile: ClientProfile, cx: &mut Context<AgentSurface>) {
		s.visual_workspace_fixture(cx);

		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| w.id == "agent")
			.unwrap()
			.codex_thread_id = Some("parent".into());
		s.profile = Some(profile);
		s.native_agents = Default::default();
	}

	#[gpui::test]
	fn native_connection_prepares_once_preserves_draft_and_respects_parent_ownership(
		cx: &mut gpui::TestAppContext,
	) {
		use decodex_protocol::{
			CommandOutcome, CommandResultEnvelope, EntityRevision, ResultPayload,
		};
		for (managed, fail) in [(false, false), (true, false), (false, true)] {
			let (_root, profile, server) = wire_test_support::fixture(move |listener| async move {
				for step in 0..if managed { 1 } else { 3 } {
					let mut socket = wire_test_support::accept(&listener).await;
					let request: ClientMessage = serde_json::from_str(
						socket.next().await.unwrap().unwrap().to_text().unwrap(),
					)
					.unwrap();
					let response = if step == 1 {
						let ClientMessage::Command(command) = request else {
							panic!("expected preparation")
						};
						assert!(
							matches!(command.payload, CommandPayload::Agent { action } if matches!(*action, AgentActionDto::PrepareNativeAgent { ref work_id, ref thread_id } if work_id.as_str() == "agent" && thread_id.as_str() == "child"))
						);
						let receipt =
							ServerMessage::CommandReceipt(decodex_protocol::CommandReceipt {
								version: CURRENT_VERSION,
								server_id: ServerId::new(wire_test_support::SERVER).unwrap(),
								client_command_id: command.client_command_id.clone(),
								idempotency_key: command.idempotency_key.clone(),
								disposition: decodex_protocol::ReceiptDisposition::Executed,
								original_client_command_id: command.client_command_id.clone(),
							});
						socket
							.send(Message::Text(serde_json::to_string(&receipt).unwrap().into()))
							.await
							.unwrap();
						ServerMessage::CommandResult(CommandResultEnvelope {
							version: CURRENT_VERSION,
							server_id: ServerId::new(wire_test_support::SERVER).unwrap(),
							client_command_id: command.client_command_id,
							idempotency_key: command.idempotency_key,
							outcome: if fail {
								CommandOutcome::Rejected
							} else {
								CommandOutcome::Succeeded
							},
							entity_revision: (!fail).then_some(EntityRevision(0)),
							payload: (!fail).then_some(ResultPayload::AgentAccepted {
								work_id: EntityId::new("agent").unwrap(),
							}),
							error: fail.then(|| {
								decodex_protocol::CommandError::ApplicationUnavailable {
									message: decodex_protocol::WireText::new("Connection failed.")
										.unwrap(),
								}
							}),
						})
					} else {
						let ClientMessage::Query(query) = request else {
							panic!("expected capability read, never draft submission")
						};
						assert!(
							matches!(query.payload, QueryPayload::GetNativeAgents { thread_id: Some(ref id), .. } if id.as_str() == "child")
						);
						ServerMessage::QueryResult(QueryResultEnvelope {
							version: CURRENT_VERSION,
							server_id: ServerId::new(wire_test_support::SERVER).unwrap(),
							query_id: query.query_id,
							payload: QueryResultPayload::NativeAgents(
								NativeAgentsResult::Conversation {
									thread_id: "child".into(),
									can_input: if managed {
										Some(false)
									} else if step == 0 {
										None
									} else {
										Some(true)
									},
									active_turn: None,
								},
							),
						})
					};
					socket
						.send(Message::Text(serde_json::to_string(&response).unwrap().into()))
						.await
						.unwrap();
				}
			});
			let surface = cx.new(AgentSurface::new);
			surface.update(cx, |s, cx| {
				bind_fixture(s, profile, cx);
				s.native_agents.selected = Some(("agent".into(), "child".into()));
				s.native_agents.editor = s.native_agent_target("agent", "child");
				let input = cx.new(|cx| ComposerInput::message(35, "Message", "Agent message", cx));
				input.update(cx, |i, cx| i.set_content("Keep this draft", cx));
				s.native_agents.input = Some(input);
				s.read_native_agent(cx);
			});
			cx.run_until_parked();
			if fail {
				surface.update(cx, |s, cx| {
                    assert!(matches!(&s.native_agents.connection, super::NativeConnection::Failed(reason) if reason == "Connection failed."));
                    assert!(!s.native_input_available());
                    assert!(s.native_agents.detail_task.is_none());
                    assert_eq!(s.native_agents.input.as_ref().unwrap().read(cx).content(), "Keep this draft");
                    s.retry_native_connection(cx);
                });
				cx.run_until_parked();
			}
			server.join().unwrap();
			surface.read_with(cx, |s, cx| {
				assert_eq!(
					s.native_agents.input.as_ref().unwrap().read(cx).content(),
					"Keep this draft"
				);
				assert!(s.native_agents.pending.is_none() && s.native_agents.detail_task.is_none());
				assert_eq!(s.native_input_available(), !managed);
				assert!(if managed {
					matches!(s.native_agents.connection, super::NativeConnection::ParentManaged)
				} else {
					matches!(s.native_agents.connection, super::NativeConnection::Ready)
				});
			});
		}
	}

	#[gpui::test]
	fn detail_read_survives_refresh_and_rejects_foreign_thread(cx: &mut gpui::TestAppContext) {
		for foreign in [false, true] {
			let (_root, profile, server) = wire_test_support::fixture(move |listener| async move {
				let mut socket = wire_test_support::accept(&listener).await;
				let request: ClientMessage =
					serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
						.unwrap();
				let ClientMessage::Query(query) = request else { panic!("read only") };

				assert!(
					matches!(query.payload, QueryPayload::GetNativeAgents { ref work_id, thread_id: Some(ref thread), cursor: None } if work_id.as_str() == "agent" && thread.as_str() == "child")
				);

				let response = ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(super::super::wire_test_support::SERVER).unwrap(),
					query_id: query.query_id,
					payload: QueryResultPayload::NativeAgents(conversation(if foreign {
						"foreign"
					} else {
						"child"
					})),
				});

				socket
					.send(Message::Text(serde_json::to_string(&response).unwrap().into()))
					.await
					.unwrap();
			});
			let surface = cx.new(AgentSurface::new);

			surface.update(cx, |s, cx| {
				bind_fixture(s, profile, cx);

				s.native_agents.selected = Some(("agent".into(), "child".into()));

				s.read_native_agent(cx);

				s.generation += 1;

				s.apply_result(Ok(AgentSnapshotResult::Available(s.snapshot.clone().unwrap())));
			});

			cx.run_until_parked();
			server.join().unwrap();
			surface.read_with(cx, |s, _| {
				assert!(s.native_agents.detail_task.is_none());
				assert_eq!(
					s.native_agents.detail,
					Some(if foreign {
						NativeAgentsResult::Unavailable
					} else {
						conversation("child")
					})
				);
			});
		}
	}

	#[gpui::test]
	fn cancelled_send_preserves_scoped_draft_and_ignores_late_acceptance(
		cx: &mut gpui::TestAppContext,
	) {
		let (_root, profile, server) = wire_test_support::fixture(|_| async {});

		server.join().unwrap();

		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			bind_fixture(s, profile.clone(), cx);

			let target = s.native_agent_target("agent", "child").unwrap();

			s.native_agents.selected = Some(("agent".into(), "child".into()));
			s.native_agents.editor = Some(target.clone());

			let input = cx.new(|cx| ComposerInput::message(35, "Message", "Agent message", cx));

			input.update(cx, |i, cx| i.set_content("Keep my draft", cx));

			s.native_agents.input = Some(input.clone());
			s.native_agents.detail = Some(conversation("child"));
			s.native_agents.pending = Some(target.clone());
			s.native_agents.send_task = Some(cx.spawn(async |_, _| future::pending::<()>().await));

			s.bind_profile(None, cx);

			s.poll_task = None;

			assert!(s.native_agents.selected.is_none());
			assert!(s.native_agents.pending.is_none());
			assert!(s.native_agents.send_task.is_none());
			assert!(s.native_agents.uncertain.contains(&target));
			assert_eq!(s.native_agents.drafts[&target], "Keep my draft");

			s.finish_native_agent_send(
				target.clone(),
				"Keep my draft".into(),
				Some(AgentCommandResponse::Accepted { work_id: EntityId::new("agent").unwrap() }),
				cx,
			);

			assert_eq!(input.read(cx).content(), "Keep my draft");
			assert!(s.native_agents.uncertain.contains(&target));

			s.profile = Some(profile.clone());

			s.visual_workspace_fixture(cx);

			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap()
				.codex_thread_id = Some("parent".into());

			assert_eq!(s.native_agent_target("agent", "child").unwrap(), target);

			s.profile = Some(profile.with_expected_server_id(
				decodex_protocol::ServerId::new("foreign-server").unwrap(),
			));

			let other = s.native_agent_target("agent", "child").unwrap();

			assert_ne!(other, target);
			assert!(!s.native_agents.drafts.contains_key(&other));
			assert!(!s.native_agents.uncertain.contains(&other));
		});
	}

	#[gpui::test]
	fn native_send_lost_reply_is_not_repeated_after_refresh(cx: &mut gpui::TestAppContext) {
		let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
			let mut socket = wire_test_support::accept(&listener).await;
			let request: ClientMessage =
				serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
					.unwrap();
			let ClientMessage::Command(command) = request else { panic!("one message") };

			assert!(
				matches!(command.payload, CommandPayload::Agent { action } if matches!(*action, AgentActionDto::NativeAgentInput { ref work_id, ref thread_id, ref text, expected_turn: None } if work_id.as_str() == "agent" && thread_id.as_str() == "child" && text.as_str() == "Follow up"))
			);

			drop(socket);
		});
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			bind_fixture(s, profile, cx);

			s.native_agents.selected = Some(("agent".into(), "child".into()));
			s.native_agents.editor = s.native_agent_target("agent", "child");

			let input = cx.new(|cx| ComposerInput::message(35, "Message", "Agent message", cx));

			input.update(cx, |i, cx| i.set_content("Follow up", cx));

			s.native_agents.input = Some(input);
			s.native_agents.detail = Some(conversation("child"));

			s.send_native_agent(cx);

			assert!(s.native_agents.pending.is_some());

			s.generation += 1;

			s.apply_result(Ok(AgentSnapshotResult::Available(s.snapshot.clone().unwrap())));
		});

		cx.run_until_parked();
		server.join().unwrap();

		surface.update(cx, |s, cx| {
			let target = s.native_agent_target("agent", "child").unwrap();

			assert!(s.native_agents.pending.is_none());
			assert!(s.native_agents.send_task.is_none());
			assert!(s.native_agents.uncertain.contains(&target));
			assert_eq!(s.native_agents.input.as_ref().unwrap().read(cx).content(), "Follow up");

			s.send_native_agent(cx);

			assert!(
				s.native_agents.pending.is_none(),
				"uncertain delivery cannot authorize a retry"
			);
			assert!(s.native_agents.send_task.is_none());
		});
	}

	#[gpui::test]
	fn accepted_send_clears_only_its_unchanged_draft(cx: &mut gpui::TestAppContext) {
		let (_root, profile, server) = wire_test_support::fixture(|_| async {});

		server.join().unwrap();

		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			bind_fixture(s, profile, cx);

			let target = s.native_agent_target("agent", "child").unwrap();
			let input = cx.new(|cx| ComposerInput::message(35, "Message", "Agent message", cx));

			s.native_agents.input = Some(input.clone());
			s.native_agents.editor = Some(target.clone());

			for newer in [false, true] {
				let content = if newer { "Newer text" } else { "Sent text" };

				input.update(cx, |i, cx| i.set_content(content, cx));

				s.native_agents.selected = Some(("agent".into(), "child".into()));

				s.close_native_agent(cx);

				s.native_agents.pending = Some(target.clone());

				s.finish_native_agent_send(
					target.clone(),
					"Sent text".into(),
					Some(AgentCommandResponse::Accepted {
						work_id: EntityId::new("agent").unwrap(),
					}),
					cx,
				);

				assert_eq!(input.read(cx).content(), if newer { "Newer text" } else { "" });
				assert_eq!(
					s.native_agents.drafts.get(&target).map(String::as_str),
					newer.then_some("Newer text")
				);
			}
		});
	}
	#[gpui::test]
	fn refusal_before_dispatch_keeps_the_draft_available(cx: &mut gpui::TestAppContext) {
		let (_root, profile, server) = wire_test_support::fixture(|_| async {});

		server.join().unwrap(); // The local endpoint is gone before any command can be sent.

		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			bind_fixture(s, profile, cx);

			let target = s.native_agent_target("agent", "child").unwrap();

			s.native_agents.selected = Some(("agent".into(), "child".into()));
			s.native_agents.editor = Some(target);
			s.native_agents.detail = Some(conversation("child"));

			let input = cx.new(|cx| ComposerInput::message(35, "Message", "Agent message", cx));

			input.update(cx, |i, cx| i.set_content("Not dispatched", cx));

			s.native_agents.input = Some(input);

			s.send_native_agent(cx);
		});

		cx.run_until_parked();
		surface.read_with(cx, |s, cx| {
			assert!(s.native_agents.pending.is_none());
			assert!(s.native_agents.uncertain.is_empty());
			assert!(s.native_agents.feedback.contains("was not sent"));
			assert_eq!(
				s.native_agents.input.as_ref().unwrap().read(cx).content(),
				"Not dispatched"
			);
		});
	}
}
