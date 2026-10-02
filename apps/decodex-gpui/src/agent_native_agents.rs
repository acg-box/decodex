//! Observe native descendants without creating duplicate local workers.
use std::{
	collections::{BTreeMap, BTreeSet},
	time::{Duration, Instant},
};

use gpui::{AnyElement, AppContext as _};
use tokio::runtime::Builder;
use ui_theme::{AGENT_CHAT_OVERLAY, CAPTION_SIZE, TEXT_MUTED, TREE_ROW_HEIGHT};

#[cfg(test)] use crate::shell::agent_surface::{AgentSnapshotResult, ClientProfile};
use crate::{
	shell::{
		agent_surface,
		agent_surface::{
			AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
			ComposerInput, Context, Entity, EntityId, FluentBuilder, HistoryText, IdempotencyKey,
			InteractiveElement, IntoElement, ParentElement, SharedString,
			StatefulInteractiveElement, Styled, SubmitComposer, Task, WireText, agent_tree,
			agent_tree::DISCLOSURE, markdown, ui_theme,
		},
	},
	ui_loading, ui_motion,
};
use decodex_protocol::{NativeAgentDto, NativeAgentsResult};

#[derive(Default)]
pub(super) struct NativeAgents {
	pub lists: BTreeMap<String, Vec<NativeAgentDto>>,
	pub selected: Option<(String, String)>,
	pub detail: Option<NativeAgentsResult>,
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct NativeAgentTarget {
	profile: String,
	source: Option<String>,
	root: String,
	work: String,
	thread: String,
}

impl AgentSurface {
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

			self.native_agents.selected = None;
		}
	}

	pub(super) fn close_native_agent(&mut self, cx: &mut Context<Self>) {
		if let (Some(target), Some(input)) = (&self.native_agents.editor, &self.native_agents.input)
		{
			self.native_agents.drafts.insert(target.clone(), input.read(cx).content().into());
		}

		self.native_agents.selected = None;
		self.native_agents.detail = None;
		self.native_agents.detail_task = None;
		self.native_agents.next_detail = None;
	}

	pub(super) fn poll_native_agents(&mut self, cx: &mut Context<Self>) {
		if !self.command_connection_ready() {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			return;
		};

		if self.agent_tree_visible
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
								result.push((owner, list));
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
			&& self.native_agents.detail_task.is_none()
			&& self.native_agents.next_detail.is_none_or(|t| t <= Instant::now())
		{
			self.read_native_agent(cx);
		}
	}

	pub(super) fn open_native_agent(&mut self, owner: &str, thread: &str, cx: &mut Context<Self>) {
		if self.native_agents.pending.is_some() || !self.command_connection_ready() {
			return;
		}

		let Some(target) = self.native_agent_target(owner, thread) else {
			return;
		};

		self.open_page(owner, cx);
		self.reset_recap();

		self.native_agents.selected = Some((owner.into(), thread.into()));
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
			.get_or_insert_with(|| cx.new(|cx| ComposerInput::new(0, cx)))
			.clone();
		let draft = self.native_agents.drafts.get(&target).cloned().unwrap_or_default();

		self.native_agents.editor = Some(target);

		input.update(cx, |i, cx| i.set_content(&draft, cx));
		self.read_native_agent(cx);
		cx.notify();
	}

	fn read_native_agent(&mut self, cx: &mut Context<Self>) {
		if !self.command_connection_ready() {
			return;
		}

		let (Some(profile), Some((owner, thread))) =
			(self.profile.clone(), self.native_agents.selected.clone())
		else {
			return;
		};
		let target = (owner.clone(), thread.clone());
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
			let result = match read.await {
                Some(result) if matches!(&result, NativeAgentsResult::Conversation { thread_id, .. } if thread_id == &target.1) => result,
                _ => NativeAgentsResult::Unavailable,
            };
			let _ = surface.update(cx, |s, cx| {
				if s.native_agents.selected.as_ref() == Some(&target)
					&& s.native_agents.detail.as_ref() != Some(&result)
				{
					s.native_agents.detail = Some(result);

					cx.notify();
				}

				s.native_agents.detail_task = None;
				s.native_agents.next_detail = Some(Instant::now() + Duration::from_secs(3));
			});
		}));
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
			let expanded = !self.agent_tree_collapsed.contains(&key);
			let has_children = self.native_agents.lists.get(owner).is_some_and(|list| {
				list.iter().any(|child| child.parent_thread_id == agent.thread_id)
			});
			let selected = self
				.native_agents
				.selected
				.as_ref()
				.is_some_and(|(o, t)| o == owner && t == &agent.thread_id);
			let row = agent_tree::tree_row(
				format!("native-agent-row-{}", agent.thread_id),
				depth,
				selected,
			)
			.child(if has_children {
				self.tree_toggle(key.clone(), &label, expanded, cx)
			} else {
				gpui::div().w(gpui::px(DISCLOSURE)).flex_none().into_any_element()
			})
			.child(gpui::div().flex_1().min_w_0().child(self.workspace_action(
				format!("native-agent-open-{thread}"),
				label,
				move |s, cx| s.open_native_agent(&work, &thread, cx),
				cx,
			)))
			.child(
				gpui::div()
					.text_size(gpui::px(CAPTION_SIZE))
					.flex_none()
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(format!("L{depth} · {}", agent.status)),
			);
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

	fn native_agent_transcript(&self, thread: &str) -> (AnyElement, bool) {
		let mut body = gpui::div()
			.id("native-agent-transcript")
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.p_5()
			.flex()
			.flex_col()
			.gap_5();
		let mut can_input = false;

		match &self.native_agents.detail {
			Some(NativeAgentsResult::Conversation {
				messages,
				truncated,
				can_input: enabled,
				..
			}) => {
				can_input = *enabled && self.command_connection_ready();

				if *truncated {
					body = body.child(agent_surface::muted(
						"Recent conversation · earlier content omitted",
					));
				}

				for message in messages {
					let user = message.role == "user";

					body = body.child(
						gpui::div().w_full().flex().when(user, |d| d.justify_end()).child(
							gpui::div()
								.max_w(gpui::relative(if user { 0.8 } else { 1.0 }))
								.when(user, |d| {
									d.p_3().rounded(gpui::px(15.)).bg(gpui::rgba(0xffffff0b))
								})
								.child(markdown::render(
									&message.text,
									&format!("native-{thread}-{}", message.id),
								)),
						),
					);
				}
			},
			None => body = body.child(ui_loading::conversation("Loading conversation")),
			_ =>
				body = body.child(agent_surface::muted(
					"This agent's conversation is unavailable. Retrying…",
				)),
		}

		(body.into_any_element(), can_input)
	}

	pub(super) fn native_agent_view(&self, cx: &mut Context<Self>) -> AnyElement {
		let Some((owner, thread)) = &self.native_agents.selected else {
			return gpui::div().into_any_element();
		};
		let title = self
			.native_agents
			.lists
			.get(owner)
			.into_iter()
			.flatten()
			.find(|a| &a.thread_id == thread)
			.map(|a| a.title.as_str())
			.unwrap_or("Agent");
		let back = owner.clone();
		let parent = self
			.native_agents
			.lists
			.get(owner)
			.into_iter()
			.flatten()
			.find(|a| &a.thread_id == thread)
			.map(|a| a.parent_thread_id.clone());
		let (body, can_input) = self.native_agent_transcript(thread);
		let mut panel = gpui::div()
			.size_full()
			.flex()
			.flex_col()
			.rounded(gpui::px(14.))
			.bg(gpui::rgba(AGENT_CHAT_OVERLAY))
			.child(
				gpui::div()
					.h(gpui::px(36.))
					.px_3()
					.flex()
					.items_center()
					.gap_3()
					.child(self.workspace_action(
						"native-agent-back".into(),
						"←".into(),
						move |s, cx| {
							if let Some(parent) = &parent
								&& s.native_agents
									.lists
									.get(&back)
									.is_some_and(|list| list.iter().any(|a| &a.thread_id == parent))
							{
								s.open_native_agent(&back, parent, cx);

								return;
							}

							s.open_page(&back, cx);
						},
						cx,
					))
					.when(!self.pages.is_empty(), |row| {
						row.child(
							gpui::div()
								.max_w(gpui::px(360.))
								.min_w_0()
								.child(self.workspace_tabs(cx)),
						)
					})
					.child(gpui::div().flex_1().min_w_0().text_ellipsis().child(title.to_owned()))
					.child(markdown::copy_button(
						&format!("native-reference-{thread}"),
						"Copy agent reference",
						format!("thread://{thread}"),
					)),
			)
			.child(body);

		if can_input {
			if let Some(input) = &self.native_agents.input {
				panel = panel.child(
					gpui::div()
						.id("native-agent-input")
						.m_4()
						.p_2()
						.rounded(gpui::px(16.))
						.bg(gpui::rgba(0x202024ee))
						.flex()
						.items_center()
						.on_action(cx.listener(|s, _: &SubmitComposer, _, cx| {
							s.send_native_agent(cx);
							cx.stop_propagation();
						}))
						.child(gpui::div().flex_1().min_w_0().child(input.clone()))
						.child(self.workspace_action(
							"native-agent-send".into(),
							if self.native_agents.pending.is_some() { "…" } else { "↑" }.into(),
							|s, cx| s.send_native_agent(cx),
							cx,
						)),
				);
			}
		} else if matches!(self.native_agents.detail, Some(NativeAgentsResult::Conversation { .. }))
		{
			panel = panel.child(gpui::div().p_4().child(agent_surface::muted(
				"This agent is controlled by its parent. Open the parent conversation to request changes.",
			)));
		}
		if !self.native_agents.feedback.is_empty() {
			panel = panel.child(
				gpui::div()
					.px_4()
					.pb_3()
					.child(agent_surface::muted(self.native_agents.feedback.clone())),
			);
		}

		panel.into_any_element()
	}

	fn send_native_agent(&mut self, cx: &mut Context<Self>) {
		if self.native_agents.pending.is_some() || !self.command_connection_ready() {
			return;
		}

		let (
			Some(profile),
			Some((owner, thread)),
			Some(input),
			Some(NativeAgentsResult::Conversation {
				thread_id: observed,
				can_input: true,
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
			can_input: true,
			active_turn: None,
			messages: vec![],
			truncated: false,
		}
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

			let input = cx.new(|cx| ComposerInput::new(0, cx));

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

			let input = cx.new(|cx| ComposerInput::new(0, cx));

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
			let input = cx.new(|cx| ComposerInput::new(0, cx));

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

			let input = cx.new(|cx| ComposerInput::new(0, cx));

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
