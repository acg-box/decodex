//! Observe native descendants without creating duplicate local workers.
use super::*;
use decodex_protocol::{NativeAgentDto, NativeAgentsResult};
use std::{
	collections::BTreeMap,
	time::{Duration, Instant},
};
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
	drafts: BTreeMap<String, String>,
	feedback: String,
	sending: bool,
	uncertain: Option<String>,
	send_task: Option<Task<()>>,
}
impl ChiefSurface {
	pub(super) fn poll_native_agents(&mut self, cx: &mut Context<Self>) {
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
					let Ok(runtime) =
						tokio::runtime::Builder::new_current_thread().enable_all().build()
					else {
						return Vec::new();
					};
					runtime.block_on(async move {
						let client = ChiefClient::new(profile);
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
		if self.native_agents.sending {
			return;
		}
		if let (Some((_, previous)), Some(input)) =
			(&self.native_agents.selected, &self.native_agents.input)
		{
			self.native_agents.drafts.insert(previous.clone(), input.read(cx).content().into());
		}
		self.open_page(owner, cx);
		self.native_agents.selected = Some((owner.into(), thread.into()));
		self.native_agents.detail = None;
		self.native_agents.feedback = if self.native_agents.uncertain.as_deref() == Some(thread) {
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
		let draft = self.native_agents.drafts.get(thread).cloned().unwrap_or_default();
		input.update(cx, |i, cx| i.set_content(&draft, cx));
		self.read_native_agent(cx);
		cx.notify();
	}

	fn read_native_agent(&mut self, cx: &mut Context<Self>) {
		let (Some(profile), Some((owner, thread))) =
			(self.profile.clone(), self.native_agents.selected.clone())
		else {
			return;
		};
		let target = thread.clone();
		let read = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime
				.block_on(ChiefClient::new(profile).native_agents(
					EntityId::new(owner).ok()?,
					Some(WireText::new(thread).ok()?),
					None,
				))
				.ok()
		});
		self.native_agents.detail_task = Some(cx.spawn(async move |surface, cx| {
			let result = read.await.unwrap_or(NativeAgentsResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.native_agents.selected.as_ref().is_some_and(|(_, id)| id == &target)
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
	) -> (gpui::AnyElement, usize) {
		let mut rows = div().flex().flex_col();
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
			rows = rows.child(
				div()
					.h(px(ui_theme::TREE_ROW_HEIGHT))
					.pl(px(8. + depth as f32 * 14.))
					.flex()
					.items_center()
					.gap_1()
					.child(div().flex_1().min_w_0().child(self.workspace_action(
						format!("native-agent-{thread}"),
						label,
						move |s, cx| s.open_native_agent(&work, &thread, cx),
						cx,
					)))
					.child(
						div()
							.text_size(px(10.))
							.text_color(rgb(ui_theme::TEXT_MUTED))
							.child(format!("L{depth} · {}", agent.status)),
					),
			);
			count += 1;
			let (children, n) = self.native_branches(owner, &agent.thread_id, depth + 1, cx);
			rows = rows.child(children);
			count += n;
		}
		(rows.into_any_element(), count)
	}

	fn native_agent_transcript(&self, thread: &str) -> (gpui::AnyElement, bool) {
		let mut body = div()
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
				can_input = *enabled;
				if *truncated {
					body = body.child(muted("Recent conversation · earlier content omitted"));
				}
				for message in messages {
					let user = message.role == "user";
					body = body.child(
						div().w_full().flex().when(user, |d| d.justify_end()).child(
							div()
								.max_w(gpui::relative(if user { 0.8 } else { 1.0 }))
								.when(user, |d| d.p_3().rounded(px(15.)).bg(rgba(0xffffff0b)))
								.child(markdown::render(
									&message.text,
									&format!("native-{thread}-{}", message.id),
								)),
						),
					);
				}
			},
			None => body = body.child(muted("Loading conversation…")),
			_ => body = body.child(muted("This agent's conversation is unavailable. Retrying…")),
		}
		(body.into_any_element(), can_input)
	}

	pub(super) fn native_agent_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let Some((owner, thread)) = &self.native_agents.selected else {
			return div().into_any_element();
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
		let mut panel =
			div()
				.size_full()
				.flex()
				.flex_col()
				.rounded(px(14.))
				.bg(rgba(ui_theme::CHIEF_CHAT_OVERLAY))
				.child(
					div()
						.h(px(36.))
						.px_3()
						.flex()
						.items_center()
						.gap_3()
						.child(self.workspace_action(
							"native-agent-back".into(),
							"←".into(),
							move |s, cx| {
								if let Some(parent) = &parent
									&& s.native_agents.lists.get(&back).is_some_and(|list| {
										list.iter().any(|a| &a.thread_id == parent)
									}) {
									s.open_native_agent(&back, parent, cx);
									return;
								}
								s.open_page(&back, cx);
							},
							cx,
						))
						.child(div().flex_1().min_w_0().text_ellipsis().child(title.to_owned()))
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
					div()
						.id("native-agent-input")
						.m_4()
						.p_2()
						.rounded(px(16.))
						.bg(rgba(0x202024ee))
						.flex()
						.items_center()
						.on_action(cx.listener(|s, _: &SubmitComposer, _, cx| {
							s.send_native_agent(cx);
							cx.stop_propagation();
						}))
						.child(div().flex_1().min_w_0().child(input.clone()))
						.child(self.workspace_action(
							"native-agent-send".into(),
							if self.native_agents.sending { "…" } else { "↑" }.into(),
							|s, cx| s.send_native_agent(cx),
							cx,
						)),
				);
			}
		} else if matches!(self.native_agents.detail, Some(NativeAgentsResult::Conversation { .. }))
		{
			panel = panel.child(div().p_4().child(muted(
				"This agent is controlled by its parent. Open the parent conversation to request changes.",
			)));
		}
		if !self.native_agents.feedback.is_empty() {
			panel =
				panel.child(div().px_4().pb_3().child(muted(self.native_agents.feedback.clone())));
		}
		panel.into_any_element()
	}

	fn send_native_agent(&mut self, cx: &mut Context<Self>) {
		if self.native_agents.sending
			|| self
				.native_agents
				.selected
				.as_ref()
				.is_some_and(|(_, thread)| self.native_agents.uncertain.as_ref() == Some(thread))
		{
			return;
		}
		let (
			Some(profile),
			Some((owner, thread)),
			Some(input),
			Some(NativeAgentsResult::Conversation { can_input: true, active_turn, .. }),
		) = (
			self.profile.clone(),
			self.native_agents.selected.clone(),
			self.native_agents.input.clone(),
			self.native_agents.detail.clone(),
		)
		else {
			return;
		};
		let text = input.read(cx).content().to_owned();
		if text.trim().is_empty() {
			return;
		}
		let sent_thread = thread.clone();
		let (Ok(work_id), Ok(thread_id), Ok(message)) =
			(EntityId::new(owner), WireText::new(thread), HistoryText::new(text.clone()))
		else {
			return;
		};
		self.native_agents.sending = true;
		self.native_agents.feedback = "Sending…".into();
		let send = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime
				.block_on(ChiefClient::new(profile).execute(
					ChiefActionDto::NativeAgentInput {
						work_id,
						thread_id,
						text: message,
						expected_turn: active_turn.and_then(|t| WireText::new(t).ok()),
					},
					IdempotencyKey::new(unique_command()).ok()?,
				))
				.ok()
		});
		self.native_agents.send_task=Some(cx.spawn(async move |surface,cx| {
            let result=send.await;
            let _=surface.update(cx,|s,cx| {
                s.native_agents.sending=false;
                if matches!(result,Some(ChiefCommandResponse::Accepted{..})) {
                    if input.read(cx).content()==text {input.update(cx,|i,cx|i.set_content("",cx));}
                    s.native_agents.feedback="Sent".into();
                } else if matches!(result,Some(ChiefCommandResponse::Rejected{..})) {s.native_agents.feedback="Message was not sent. Refresh the conversation and check its availability.".into();} else {s.native_agents.uncertain=Some(sent_thread);s.native_agents.feedback="Delivery was not confirmed. Inspect the conversation before sending again.".into();}
                s.native_agents.next_detail=None;cx.notify();
            });
        }));
		cx.notify();
	}
}
