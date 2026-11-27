//! The right sidebar reads a selected source without navigating the main conversation.
#[path = "agent_turn_collaboration.rs"] mod turn_collaboration;
use super::*;
use crate::shell::agent_surface::work_board::board_topics::Post;
use turn_collaboration::{Participant, ParticipantState};
#[path = "agent_report_reader.rs"] mod report_reader;
use decodex_protocol::{
	AgentCollaborationDto, AgentTimelineContent, ItemContextRequest, ItemContextResult, WireText,
};

impl AgentSurface {
	fn clear_reference_source(&mut self) {
		self.work_board.collaboration.source_open = None;
		self.work_board.collaboration.original_text = None;
		self.work_board.collaboration.original_read = None;
	}

	pub(in crate::shell::agent_surface) fn reset_reference(&mut self) {
		self.work_board.reference = ReferencePage::Overview;
		self.work_board.reference_scroll = Default::default();
		self.work_board.reference_history.clear();
		self.work_board.collaboration.expanded_items.clear();
		self.clear_reference_source();
	}

	pub(in crate::shell::agent_surface::work_board) fn show_reference(
		&mut self,
		page: ReferencePage,
		cx: &mut Context<Self>,
	) {
		// Opening a second source explicitly restores the split from conversation-only mode.
		if self.workspace.chat_expanded {
			self.toggle_chat_fullscreen(cx);
		}
		if self.work_board.reference != page {
			if page == ReferencePage::Overview {
				self.reset_reference();
			} else {
				let board = &mut self.work_board;
				board.reference_history.push((
					std::mem::replace(&mut board.reference, page),
					std::mem::take(&mut board.reference_scroll),
				));
				self.clear_reference_source();
			}
		}
		self.workspace.agent_tree_visible = true;
		ui_motion::begin_interaction();
		cx.notify();
	}

	fn previous_reference(&mut self, cx: &mut Context<Self>) {
		let board = &mut self.work_board;
		(board.reference, board.reference_scroll) =
			board.reference_history.pop().unwrap_or_default();
		self.clear_reference_source();
		ui_motion::begin_interaction();
		cx.notify();
	}

	pub(in crate::shell::agent_surface) fn peek_conversation(
		&mut self,
		key: &str,
		cx: &mut Context<Self>,
	) {
		if let Some(thread) =
			self.factory_rows().into_iter().find(|row| row.key == key).and_then(|row| row.thread)
		{
			self.show_reference(
				ReferencePage::Conversation { key: key.into(), thread, message: None },
				cx,
			);
		}
	}

	fn discussion_calls<'a>(
		&'a self,
		thread: &'a str,
		turn: &'a str,
	) -> impl Iterator<Item = (&'a String, &'a AgentCollaborationDto)> {
		let bound =
			self.timeline.native.binding.as_ref().is_some_and(|binding| binding.thread == thread);
		self.timeline.native.entries.iter().filter_map(move |entry| match &entry.content {
			AgentTimelineContent::Item { turn_id, item_id, collaboration: Some(call), .. }
				if bound && turn_id == turn && call.sender_thread_id == thread =>
				Some((item_id, call)),
			_ => None,
		})
	}

	pub(in crate::shell::agent_surface::work_board) fn current_discussion_includes(
		&self,
		child: &str,
	) -> bool {
		let Some(binding) = self.timeline.native.binding.as_ref() else {
			return false;
		};
		let Some(turn) = self.latest_discussion_turn(&binding.thread) else {
			return false;
		};
		self.discussion_calls(&binding.thread, turn)
			.any(|(_, call)| call.receiver_thread_ids.iter().any(|id| id == child))
	}

	fn reference_link(
		&self,
		id: String,
		label: &str,
		icon: Symbol,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		self.reference_control(id, label.into(), action, cx)
			.justify_start()
			.px(gpui::px(0.))
			.bg(gpui::transparent_black())
			.text_color(ui_theme::rgb(BLUE))
			.child(workspace_symbols::icon_sized(icon, 13.))
			.child(label.to_owned())
			.smooth()
			.into_any_element()
	}

	pub(in crate::shell::agent_surface::work_board) fn reference_row(
		&self,
		id: String,
		title: String,
		detail: String,
		icon: Option<Symbol>,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let arrow = format!("reference-arrow-{id}");
		let label = if detail.is_empty() { title.clone() } else { format!("{title}. {detail}") };
		self.reference_control(id, label, action, cx)
			.w_full()
			.rounded(gpui::px(ui_theme::NAVIGATION_ROW_RADIUS))
			.justify_start()
			.items_center()
			.bg(gpui::transparent_black())
			.min_h(gpui::px(ui_theme::CONTROL_SIZE))
			.px(gpui::px(0.))
			.py(gpui::px(3.))
			.gap(gpui::px(8.))
			.when_some(icon, |row, icon| {
				row.child(
					gpui::div().flex_none().text_color(ui_theme::rgb(SECONDARY)).child(
						workspace_symbols::icon_sized(icon, ui_theme::NAVIGATION_ACTION_SIZE),
					),
				)
			})
			.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.flex()
					.flex_col()
					.child(
						gpui::div()
							.line_clamp(2)
							.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
							.line_height(gpui::px(19.))
							.text_color(ui_theme::rgb(PRIMARY))
							.child(title),
					)
					.when(!detail.is_empty(), |col| {
						col.child(
							gpui::div()
								.line_clamp(1)
								.text_size(gpui::px(ui_theme::NAVIGATION_DETAIL_SIZE))
								.font_weight(FontWeight::NORMAL)
								.text_color(ui_theme::rgb(SECONDARY))
								.line_height(gpui::px(17.))
								.child(detail),
						)
					}),
			)
			.child(
				gpui::div()
					.flex_none()
					.text_color(ui_theme::rgb(SECONDARY))
					.child(workspace_symbols::process_chevron(arrow, false)),
			)
			.smooth()
			.into_any_element()
	}

	pub(in crate::shell::agent_surface) fn render_agent_workbench(
		&self,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let page = &self.work_board.reference;
		let (title, (body, state)) = match page {
			ReferencePage::Overview => ("Collaboration", self.reference_index(None, cx)),
			ReferencePage::Exchange(id) => {
				let cases =
					self.collaboration_cases(&self.relation_graph().edges, &self.factory_rows());
				let detail = cases
					.iter()
					.find(|case| &case.id == id)
					.map(|case| self.exchange_reader(case, cx))
					.unwrap_or_else(|| {
						reading_body().child(note("This exchange is no longer available."))
					});
				("Discussion", (detail, "detail"))
			},
			ReferencePage::Conversation { key, thread, message } => {
				let rows = self.factory_rows();
				let row = rows.iter().find(|r| &r.key == key && r.thread.as_ref() == Some(thread));
				(
					if message.is_some() { "Board message" } else { "Agent activity" },
					(
						row.map(|r| self.conversation_reader(r, &rows, message.as_deref()))
							.unwrap_or_else(|| {
								reading_body()
									.child(note("This conversation is no longer available."))
							}),
						"detail",
					),
				)
			},
			ReferencePage::Discussion { turn, thread } =>
				("Earlier discussion", self.reference_index(Some((thread, turn)), cx)),
		};
		let overview = matches!(page, ReferencePage::Overview);
		let back_label = match self.work_board.reference_history.last().map(|(page, _)| page) {
			Some(ReferencePage::Discussion { .. }) => "Back to earlier discussion",
			Some(ReferencePage::Exchange(_)) => "Back to exchange",
			Some(ReferencePage::Conversation { .. }) => "Back to agent activity",
			_ => "Back to collaboration",
		};
		let scroll_key = match page {
			ReferencePage::Overview => format!(
				"reference-index-{}",
				self.native_agents
					.selected
					.as_ref()
					.filter(|(owner, _)| Some(owner) == self.selected.as_ref())
					.map(|(_, thread)| thread.as_str())
					.or(self.selected.as_deref())
					.unwrap_or("none")
			),
			ReferencePage::Exchange(id) => format!("reference-exchange-{id}"),
			ReferencePage::Conversation { thread, message, .. } => format!(
				"reference-conversation-{thread}-{}",
				message.as_deref().unwrap_or("activity")
			),
			ReferencePage::Discussion { turn, .. } => format!("reference-discussion-{turn}"),
		};
		let header = gpui::div()
			.w_full()
			.h(gpui::px(ui_theme::CONVERSATION_HEADER_HEIGHT))
			.flex_none()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(4.))
			.px(gpui::px(ui_theme::CONVERSATION_INSET))
			.py(gpui::px(5.))
			.when(!overview, |header| {
				header.child(
					self.reference_control(
						"reference-back".into(),
						back_label.into(),
						|s, cx| s.previous_reference(cx),
						cx,
					)
					.bg(gpui::transparent_black())
					.px(gpui::px(4.))
					.child(workspace_symbols::icon_sized(Symbol::ChevronLeft, 14.))
					.child("Back")
					.smooth(),
				)
			})
			.child(
				gpui::div()
					.id("workbench-heading")
					.role(Role::Heading)
					.aria_level(2)
					.aria_label(title)
					.flex_1()
					.min_w_0()
					.font_weight(FontWeight::MEDIUM)
					.text_size(gpui::px(13.))
					.text_color(ui_theme::rgb(PRIMARY))
					.child(title),
			);
		gpui::div()
			.id("reference-sidebar")
			.size_full()
			.flex()
			.flex_col()
			.min_w_0()
			.min_h_0()
			.role(Role::Group)
			.aria_label("Collaboration sidebar")
			.child(header)
			.child(
				gpui::div().relative().flex_1().min_h_0().overflow_hidden().child(
					ui_motion::arrival(
						format!("{scroll_key}-{state}"),
						gpui::div()
							.id(SharedString::from(scroll_key.clone()))
							.debug_selector(|| "workbench-body".into())
							.size_full()
							.min_w_0()
							.min_h_0()
							.overflow_y_scroll()
							.px(gpui::px(ui_theme::CONVERSATION_INSET))
							.pb(gpui::px(10.))
							.child(
								body.flex_none()
									.debug_selector(|| "reference-reading-content".into()),
							)
							.smooth_scroll(SharedString::from(scroll_key))
							.with_handle(&self.work_board.reference_scroll)
							.contain_scroll()
							.with_scrollbar("reference-scrollbar", "Scroll reference"),
					),
				),
			)
			.into_any_element()
	}

	fn reference_index(
		&self,
		scope: Option<(&str, &str)>,
		cx: &mut Context<Self>,
	) -> (Div, &'static str) {
		let mut body = reading_body();
		let Some(work) = self.selected.as_deref().and_then(|id| self.conversation_record(id))
		else {
			return (
				empty_collaboration(
					"Select a conversation",
					Some("Its collaborators and replies will appear here."),
				),
				"unselected",
			);
		};
		let thread = self
			.native_agents
			.selected
			.as_ref()
			.filter(|(id, _)| id == &work.id)
			.map(|(_, thread)| thread.as_str())
			.or(work.codex_thread_id.as_deref());
		let latest = thread.and_then(|thread| self.latest_discussion_turn(thread));
		let turn = scope.map(|(_, turn)| turn).or(latest);
		let thread = scope.map(|(thread, _)| thread).or(thread);
		let rows = self.factory_rows();
		let roster = thread
			.zip(turn)
			.map(|(thread, turn)| self.turn_participants(thread, turn, &rows))
			.unwrap_or_default();
		let bound = self
			.timeline
			.native
			.binding
			.as_ref()
			.is_some_and(|b| Some(b.thread.as_str()) == thread && b.work == work.id);
		let belongs = |row: &Row| row.work == work.id && row.thread.as_deref() == thread;
		// Graph observations lack a turn ID. Consult them only for sources without a native turn.
		let edges = if bound && turn.is_some() { Vec::new() } else { self.relation_graph().edges };
		let cases: Vec<_> = self
			.collaboration_cases(&edges, &rows)
			.into_iter()
			.filter(|case| belongs(&case.from) || case.to.as_ref().is_some_and(belongs))
			.filter(|case| {
				if bound
					&& turn.is_some()
					&& case.from.native
					&& case.state == "Reported"
					&& case.source_event.is_none()
					&& case.source_thread == case.from.thread
				{
					// Child reports belong to their own turn. The parent roster binds them when the
					// spawn is known.
					return false;
				}
				if case.source_event.is_none()
					&& case.edge.is_none()
					&& case.to.as_ref().is_some_and(|to| {
						[&case.from, to]
							.iter()
							.any(|r| r.thread.as_ref().is_some_and(|id| roster.contains_key(id)))
					}) {
					return false;
				}
				scope.is_none()
					|| (case.source_thread.as_deref() == thread
						&& case.source_turn.as_deref() == turn)
			})
			.collect();
		let pending_here = |case: &Exchange| {
			scope.is_none()
				&& case.question.as_ref().is_some_and(|question| {
					question.pending
						&& question.work_id == work.id
						&& Some(question.thread_id.as_str()) == thread
				})
		};
		let mut populated = false;
		if scope.is_none() {
			let entries = self.workbench_entries();
			for entry in entries.iter().filter(|e| {
				e.needs_user && belongs(&e.row) && self.collaboration_entry_visible(e, &cases)
			}) {
				if !populated {
					body = body.child(section("Needs your input"));
				}
				body = body.child(self.workbench_entry(entry, cx));
				populated = true;
			}
			for case in cases.iter().filter(|case| pending_here(case)) {
				if !populated {
					body = body.child(section("Needs your input"));
				}
				body = body.child(self.exchange_row(case, belongs(&case.from), cx));
				populated = true;
			}
		}
		if let Some(turn) = turn {
			for heading in ["Needs attention", "In progress"] {
				let mut tasks = roster
					.iter()
					.filter(|(_, m)| m.state.task_section() == Some(heading))
					.peekable();
				if tasks.peek().is_none() {
					continue;
				}
				populated = true;
				body = body.child(section(heading));
				for (id, member) in tasks {
					let key = format!("collaborator-{turn}-{id}");
					let (title, detail) = member.row_labels();
					let source = rows.iter().find(|row| row.thread.as_ref() == Some(id));
					let target = ReferencePage::Conversation {
						key: source
							.map(|row| row.key.clone())
							.unwrap_or_else(|| format!("native:{}:{id}", work.id)),
						thread: id.clone(),
						message: None,
					};
					body = body.child(self.reference_row(
						key,
						title,
						detail,
						None,
						move |s, cx| s.show_reference(target.clone(), cx),
						cx,
					));
				}
			}
		}
		if let Some(outcome) = thread.zip(turn).and_then(|(thread, turn)| {
			self.outcome_row(&work.id, thread, turn, scope.is_none(), cx)
		}) {
			body = body.child(outcome);
			populated = true;
		}
		let topics = thread
			.map(|t| self.board_topics(&work.id, t, scope.map(|(_, turn)| turn), &rows))
			.unwrap_or_default();
		if !topics.is_empty() {
			populated = true;
			body = body.child(section("Discussion"));
			for (index, topic) in topics.iter().enumerate() {
				body = body.child(self.topic_group(topic, &rows, index > 0, cx));
			}
		}
		let mut reports = roster
			.iter()
			.filter(|(id, member)| {
				member.state == ParticipantState::Replied
					&& !member.reply.is_empty()
					&& !topics.iter().flatten().any(|post| {
						post.posted_by_thread.as_ref() == Some(*id)
							&& !post.truncated
							&& same_excerpt(&post.text, &member.reply)
					})
			})
			.peekable();
		if reports.peek().is_some() {
			populated = true;
			body = body.child(section("Agent reports"));
			for (id, member) in reports {
				body = body.child(self.report_row(
					format!("report-{}-{id}", turn.unwrap_or_default()),
					member,
					cx,
				));
			}
		}

		let is_earlier = |case: &Exchange| {
			scope.is_none()
				&& (case.earlier
					|| (bound
						&& case.source_thread.as_deref() == thread
						&& case.source_turn.is_some()
						&& case.source_turn.as_deref() != turn))
		};
		let previous = if scope.is_none() {
			thread
				.zip(turn)
				.map(|(thread, turn)| self.earlier_discussions(thread, turn))
				.unwrap_or_default()
		} else {
			Vec::new()
		};
		let earlier = self.work_board.collaboration.earlier;
		for historical in [false, true] {
			let mut exchanges = cases
				.iter()
				.filter(|case| !pending_here(case) && is_earlier(case) == historical)
				.peekable();
			if exchanges.peek().is_none() && (!historical || previous.is_empty()) {
				continue;
			}
			populated = true;
			let mut list = reading_body();
			if historical {
				body = body.child(
					self.reference_control(
						"collaboration-earlier".into(),
						"Earlier discussions".into(),
						|s, cx| {
							s.work_board.collaboration.earlier =
								!s.work_board.collaboration.earlier;
							cx.notify();
						},
						cx,
					)
					.w_full()
					.mt(gpui::px(6.))
					.aria_expanded(earlier)
					.px(gpui::px(0.))
					.bg(gpui::transparent_black())
					.justify_start()
					.child(
						ui_theme::navigation_category_label(gpui::div().flex_1())
							.child("Earlier discussions"),
					)
					.child(workspace_symbols::process_chevron(
						"earlier-discussions-chevron",
						earlier,
					))
					.smooth(),
				);
				if let Some(thread) = thread {
					for (turn, title) in &previous {
						let page =
							ReferencePage::Discussion { thread: thread.into(), turn: turn.clone() };
						list = list.child(self.reference_row(
							format!("earlier-turn-{turn}"),
							title.clone(),
							String::new(),
							None,
							move |s, cx| s.show_reference(page.clone(), cx),
							cx,
						));
					}
				}
			} else {
				body = body.child(section("Exchanges"));
			}
			for case in exchanges {
				list = list.child(self.exchange_row(case, belongs(&case.from), cx));
			}
			body = if historical {
				body.child(ui_motion::disclosure("earlier-discussions-content", earlier, list))
			} else {
				body.child(list)
			};
		}
		if self.work_board.collaboration.failed {
			body = body.child(note("Could not refresh project exchanges."));
		} else if !populated {
			if !bound
				&& thread.is_some()
				&& (self.native_history_loading(work) || self.timeline.native.task.is_some())
			{
				return (empty_collaboration("Loading collaborators", None), "loading");
			}
			return if roster.is_empty() {
				(
					empty_collaboration(
						"No collaborators yet",
						Some("Discussions and open tasks will appear here."),
					),
					"empty",
				)
			} else if roster.values().all(|member| member.state == ParticipantState::Finished) {
				(
					empty_collaboration(
						"Agents finished",
						Some("No report text is available for this turn."),
					),
					"empty",
				)
			} else {
				(empty_collaboration("Awaiting a report", None), "empty")
			};
		}
		(body, "content")
	}

	fn topic_group(
		&self,
		posts: &[Post],
		rows: &[Row],
		divider: bool,
		cx: &mut Context<Self>,
	) -> Div {
		let mut group = reading_body().pb(gpui::px(5.)).when(divider, |group| {
			group
				.pt(gpui::px(5.))
				.border_t_1()
				.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER))
		});
		if let [opener, earlier @ .., latest] = posts
			&& opener.message_id == opener.thread_id
			&& earlier.len() >= 2
		{
			let id = opener.thread_id.clone();
			let expanded = self.work_board.collaboration.expanded_items.contains(&id);
			let label = format!(
				"{} earlier replies ({})",
				if expanded { "Hide" } else { "Show" },
				earlier.len()
			);
			let toggle = id.clone();
			let control = self
				.reference_control(
					format!("topic-replies-{id}"),
					label.clone(),
					move |s, cx| {
						s.work_board.collaboration.toggle_item(&toggle);
						cx.notify();
					},
					cx,
				)
				.aria_expanded(expanded)
				.px(gpui::px(0.))
				.justify_start()
				.bg(gpui::transparent_black())
				.child(workspace_symbols::process_chevron(
					format!("topic-replies-arrow-{id}"),
					expanded,
				))
				.child(label)
				.smooth();
			group = group
				.child(self.board_post(opener, rows, cx))
				.child(control)
				.child(ui_motion::disclosure(
					format!("topic-replies-body-{id}"),
					expanded,
					reading_body()
						.children(earlier.iter().map(|post| self.board_post(post, rows, cx))),
				))
				.child(self.board_post(latest, rows, cx));
		} else {
			group = group.children(posts.iter().map(|post| self.board_post(post, rows, cx)));
		}
		group
	}

	fn board_post(&self, post: &Post, rows: &[Row], cx: &mut Context<Self>) -> AnyElement {
		let reply = post.message_id != post.thread_id;
		let source = post
			.posted_by_thread
			.as_deref()
			.and_then(|thread| rows.iter().find(|row| row.thread.as_deref() == Some(thread)));
		let author = if post.author == "/root" {
			"Lead"
		} else {
			source
				.map(participant_name)
				.filter(|name| !name.is_empty())
				.unwrap_or_else(|| post.author.strip_prefix("/root/").unwrap_or(&post.author))
		};
		let heading = if reply {
			format!("{author} replied")
		} else if post.text.trim_end().ends_with(['?', '？']) {
			format!("{author} asked")
		} else {
			author.to_owned()
		};
		let byline = if let Some((row, thread)) = source.zip(post.posted_by_thread.as_ref()) {
			let page = ReferencePage::Conversation {
				key: row.key.clone(),
				thread: thread.clone(),
				message: Some(post.message_id.clone()),
			};
			self.reference_control(
				format!("inspect-post-{}", post.message_id),
				format!(
					"{heading}. View board message: {}",
					crate::shell::agent_surface::execution_dock::preview(&post.text, 72)
				),
				move |s, cx| s.show_reference(page.clone(), cx),
				cx,
			)
			.bg(gpui::transparent_black())
			.justify_start()
			.px(gpui::px(0.))
			.min_h(gpui::px(24.))
			.child(named_heading(&post.message_id, &heading))
			.child(
				gpui::div()
					.text_size(gpui::px(CAPTION_SIZE))
					.text_color(ui_theme::rgb(SECONDARY))
					.child("Source"),
			)
			.smooth()
			.into_any_element()
		} else {
			named_heading(&post.message_id, &heading).into_any_element()
		};

		// Preserve native Markdown and its links when the entire message fits.
		let excerpt = source
			.filter(|_| post.text.chars().nth(240).is_some() || post.text.contains("\n\n"))
			.map(|_| markdown::report_excerpt(&post.text))
			.filter(|preview| !same_excerpt(preview, &markdown::plain_text(&post.text)));
		discussion_message(&post.message_id, byline, &post.text, post.truncated, reply, excerpt)
			.into_any_element()
	}

	fn exchange_row(&self, case: &Exchange, outgoing: bool, cx: &mut Context<Self>) -> AnyElement {
		let key = case.id.clone();
		let peer = case.to.as_ref().filter(|_| outgoing).unwrap_or(&case.from);
		self.reference_row(
			format!("collaboration-{key}"),
			case.question
				.as_ref()
				.filter(|_| needs_user(case))
				.map_or_else(|| case.title.clone(), |q| q.question.title.clone()),
			format!("{} ({})", participant_name(peer), case.state),
			Some(Symbol::TeamMembers),
			move |s, cx| s.show_reference(ReferencePage::Exchange(key.clone()), cx),
			cx,
		)
	}

	fn outcome_row(
		&self,
		work: &str,
		thread: &str,
		turn: &str,
		overview: bool,
		cx: &mut Context<Self>,
	) -> Option<Div> {
		let (item, answer, ended_at) = self.collaboration_outcome(thread, turn)?;
		let request = ItemContextRequest {
			work_id: EntityId::new(work).ok()?,
			thread_id: EntityId::new(thread).ok()?,
			turn_id: WireText::new(turn).ok()?,
			item_id: WireText::new(item).ok()?,
			cursor: None,
			before: false,
		};
		let title = if overview && self.latest_thread_turn(thread) != Some(turn) {
			"Last outcome"
		} else {
			"Outcome"
		};
		let heading = ended_at.map_or_else(
			|| title.to_owned(),
			|micros| format!("{title} ({})", crate::ui_time::timestamp(micros)),
		);
		// A short final often puts the action the user must take in its last paragraph.
		let content = if markdown::plain_text(answer).chars().nth(240).is_none() {
			markdown::render(answer, &format!("outcome-{turn}-{item}"))
		} else {
			let excerpt = markdown::report_excerpt(answer);
			crate::shell::agent_surface::selectable_text::SelectableText::plain(
				format!("outcome-excerpt-{turn}-{item}"),
				format!("{}…", excerpt.trim_end_matches('…')),
			)
			.into_any_element()
		};
		Some(reading_body().child(section(&heading)).child(content).child(self.reference_link(
			"locate-collaboration-outcome".into(),
			"Show in chat",
			Symbol::Forward,
			move |s, cx| s.locate_find_match(request.clone(), cx),
			cx,
		)))
	}

	fn report_row(&self, key: String, member: &Participant, cx: &mut Context<Self>) -> Div {
		let summary = activity::wording::message(&member.reply);
		let name = &member.name;
		let row = reading_body().gap(gpui::px(0.));
		// Show brief, self-contained reports directly, including their native Markdown links.
		if summary.chars().nth(80).is_none()
			&& same_excerpt(&summary, &markdown::plain_text(&member.reply))
		{
			return row
				.child(named_heading(&key, name))
				.child(markdown::render(&member.reply, &key));
		}
		let expanded = self.work_board.collaboration.expanded_items.contains(&key);
		let toggle = key.clone();
		let control = self
			.reference_control(
				key.clone(),
				format!("{name}. {} report", if expanded { "Collapse" } else { "Read full" }),
				move |s, cx| {
					s.work_board.collaboration.toggle_item(&toggle);
					cx.notify();
				},
				cx,
			)
			.w_full()
			.px(gpui::px(0.))
			.py(gpui::px(4.))
			.bg(gpui::transparent_black())
			.justify_start()
			.aria_expanded(expanded)
			.child(
				gpui::div().flex_1().min_w_0().flex().flex_col().child(name.clone()).child(
					ui_motion::disclosure(
						format!("{key}-summary"),
						!expanded,
						gpui::div()
							.w_full()
							.truncate()
							.font_weight(FontWeight::NORMAL)
							.text_color(ui_theme::rgb(SECONDARY))
							.child(summary),
					),
				),
			)
			.child(workspace_symbols::process_chevron(format!("{key}-disclosure"), expanded))
			.smooth();
		let reply = member.reply.clone();
		row.child(control).child(ui_motion::disclosure_lazy(
			format!("{key}-details"),
			expanded,
			move |_| markdown::render(&reply, &key),
		))
	}

	fn conversation_reader(&self, row: &Row, rows: &[Row], message: Option<&str>) -> Div {
		let name = participant_name(row);
		let mut body = reading_body().child(reader_title(name));
		if let Some(agent) = self.native_agents.lists.get(&row.work).and_then(|agents| {
			agents.iter().find(|agent| Some(&agent.thread_id) == row.thread.as_ref())
		}) {
			let parent = if self.conversation_matches(&row.work, &agent.parent_thread_id) {
				Some("this conversation")
			} else {
				rows.iter()
					.find(|r| r.thread.as_ref() == Some(&agent.parent_thread_id))
					.map(participant_name)
			};
			if let Some(parent) = parent {
				let relation = format!("Assigned by {parent}");
				body = body.child(
					note(&relation)
						.id(SharedString::from(format!("agent-parent-{}", row.key)))
						.role(Role::Label)
						.aria_label(relation),
				);
			}
		}
		if let Some(message) = message {
			let Some((work, thread)) =
				self.conversation_work().and_then(|w| Some((w.id, w.codex_thread_id?)))
			else {
				return body.child(note("This message is unavailable."));
			};
			let topics = self.board_topics(&work, &thread, None, rows);
			for topic in &topics {
				let Some(post) = topic
					.iter()
					.find(|p| p.message_id == message && p.posted_by_thread == row.thread)
				else {
					continue;
				};
				let mut heading = format!("Topic in #{}", post.channel_name);
				if post.thread_id != post.message_id {
					let author = topic
						.iter()
						.find(|p| p.message_id == post.thread_id)
						.and_then(|p| p.posted_by_thread.as_ref())
						.and_then(|id| rows.iter().find(|r| r.thread.as_ref() == Some(id)));
					heading = match author {
						Some(author) => format!(
							"Reply to {} in #{}",
							participant_name(author),
							post.channel_name
						),
						None => format!("Reply in #{}", post.channel_name),
					};
				}
				return body.child(discussion_message(
					&post.message_id,
					named_heading(&post.message_id, &heading).into_any_element(),
					&post.text,
					post.truncated,
					false,
					None,
				));
			}
			return body.child(note("This message is unavailable."));
		}
		let (state, summary) = self.work_activity_summary(row);
		body = body.child(section(&state));
		if !summary.is_empty() && summary != state {
			body = body.child(markdown::render(&summary, &format!("agent-preview-{}", row.key)));
		}
		body
	}

	fn read_sidebar_source(&mut self, req: ItemContextRequest, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else { return };
		let page = self.work_board.reference.clone();
		let read = cx.background_executor().spawn(async move {
			let rt = Builder::new_current_thread().enable_all().build().ok()?;
			let ItemContextResult::Available { request, items, .. } =
				rt.block_on(AgentClient::new(profile).item_context(req.clone())).ok()?
			else {
				return None;
			};
			if request != req {
				return None;
			}
			items.into_iter().find_map(|i| match i {
				AgentTimelineContent::Item { turn_id, item_id, text, .. }
					if turn_id == req.turn_id.as_str()
						&& item_id == req.item_id.as_str()
						&& !text.is_empty() =>
					Some(text),
				_ => None,
			})
		});
		self.work_board.collaboration.original_read = Some(cx.spawn(async move |surface, cx| {
			let text = read.await;
			let _ = surface.update(cx, |s, cx| {
				if s.work_board.reference == page
					&& s.work_board.collaboration.source_open.is_some()
				{
					s.work_board.collaboration.original_text = Some(
						text.unwrap_or_else(|| "The original message text is unavailable.".into()),
					);
					s.work_board.collaboration.original_read = None;
					cx.notify();
				}
			});
		}));
	}

	fn exchange_reader(&self, case: &Exchange, cx: &mut Context<Self>) -> Div {
		let mut body = reading_body().child(reader_title(&case.title));
		let route = case.to.as_ref().map_or_else(
			|| case.from.title.clone(),
			|to| format!("{} → {}", case.from.title, to.title),
		);
		body = body.child(note(&route));
		if !case.summary.is_empty()
			&& !same_excerpt(&case.summary, &case.title)
			&& !same_excerpt(&case.summary, &case.source_text)
		{
			body = body.child(note(&case.summary));
		}
		for (label, text) in
			[("Request", case.request.as_deref()), ("Source", Some(case.source_text.as_str()))]
		{
			if let Some(text) =
				text.filter(|text| !text.trim().is_empty() && !same_excerpt(&case.title, text))
			{
				body = body
					.child(section(label))
					.child(markdown::render(text, &format!("reference-{}-{label}", case.id)));
			}
		}
		for (label, text) in &case.steps {
			if !text.is_empty()
				&& !same_excerpt(text, &case.source_text)
				&& !same_excerpt(text, &case.title)
			{
				body = body.child(section(label)).child(note(text));
			}
		}
		if let Some(question) = case.question.as_ref().filter(|q| q.pending) {
			body =
				body.child(section("Awaiting your answer")).child(note(&question.question.title));
		}
		let row = case.source_row.clone();
		let anchor =
			case.source_turn.as_ref().zip(case.source_item.as_ref()).and_then(|(turn, item)| {
				if case.source_thread.as_ref() != row.thread.as_ref() {
					return None;
				}
				Some(ItemContextRequest {
					work_id: EntityId::new(&row.work).ok()?,
					thread_id: EntityId::new(row.thread.as_ref()?).ok()?,
					turn_id: WireText::new(turn).ok()?,
					item_id: WireText::new(item).ok()?,
					cursor: None,
					before: false,
				})
			});
		if let Some(request) = anchor.as_ref().filter(|request| {
			case.question.as_ref().is_some_and(|q| q.pending)
				&& self.conversation_matches(request.work_id.as_str(), request.thread_id.as_str())
		}) {
			let request = request.clone();
			body = body.child(self.reference_link(
				"locate-collaboration-question".into(),
				"Show question in chat",
				Symbol::Forward,
				move |s, cx| s.locate_find_match(request.clone(), cx),
				cx,
			));
		}
		let key = case.id.clone();
		let open = self.work_board.collaboration.source_open.as_deref() == Some(&key);
		body = body.child(
			self.reference_control(
				format!("collaboration-source-{key}"),
				"Source details".into(),
				move |s, cx| {
					s.work_board.collaboration.source_open = (!open).then(|| key.clone());
					s.work_board.collaboration.original_text = None;
					s.work_board.collaboration.original_read = None;
					if !open && let Some(request) = &anchor {
						s.read_sidebar_source(request.clone(), cx);
					}
					cx.notify();
				},
				cx,
			)
			.aria_expanded(open)
			.justify_start()
			.bg(gpui::transparent_black())
			.child(workspace_symbols::process_chevron(format!("source-arrow-{}", case.id), open))
			.child("Source details")
			.smooth(),
		);
		let mut fields = reading_body();
		if let Some(original) = &self.work_board.collaboration.original_text {
			if !same_excerpt(original, &case.source_text) && !same_excerpt(original, &case.title) {
				fields = fields
					.child(section("Original message"))
					.child(markdown::render(original, &format!("source-message-{}", case.id)));
			}
		} else if self.work_board.collaboration.original_read.is_some() {
			fields = fields.child(note("Reading the original message…"));
		}
		for (label, v) in [
			("Source", Some(case.source.clone())),
			("Event", case.source_event.map(|v| v.to_string())),
			("Thread", case.source_thread.clone()),
			("Turn", case.source_turn.clone()),
			("Item", case.source_item.clone()),
		] {
			if let Some(v) = v {
				fields = fields.child(self.collaboration_source_field(case, label, &v));
			}
		}
		body =
			body.child(ui_motion::disclosure(format!("exchange-fields-{}", case.id), open, fields));
		body
	}
}

fn participant_name(row: &Row) -> &str {
	if row.native && !row.owner.is_empty() { &row.owner } else { &row.title }
}

fn empty_collaboration(title: &'static str, detail: Option<&'static str>) -> Div {
	reading_body()
		.items_center()
		.h_full()
		.min_h(gpui::px(140.))
		.text_center()
		.px(gpui::px(12.))
		.py(gpui::px(ui_theme::WORKSPACE_GUTTER))
		.child(gpui::div().flex_grow(1.))
		.child(
			gpui::div()
				.mb(gpui::px(4.))
				.text_color(ui_theme::ColorScheme::current().navigation_heading())
				.child(workspace_symbols::icon_sized(Symbol::TeamMembers, 28.)),
		)
		.child(
			gpui::div()
				.id("collaboration-empty-heading")
				.role(Role::Heading)
				.aria_level(3)
				.aria_label(title)
				.text_color(ui_theme::rgb(PRIMARY))
				.font_weight(FontWeight::MEDIUM)
				.child(title),
		)
		.when_some(detail, |body, detail| {
			body.child(
				gpui::div()
					.id("collaboration-empty-description")
					.role(Role::Label)
					.aria_label(detail)
					.max_w(gpui::px(176.))
					.text_size(gpui::px(CAPTION_SIZE))
					.line_height(gpui::px(16.))
					.text_color(ui_theme::rgb(SECONDARY))
					.child(detail),
			)
		})
		.child(gpui::div().flex_grow(2.))
}

fn reading_body() -> Div {
	gpui::div()
		.w_full()
		.min_w_0()
		.flex()
		.flex_col()
		.gap(gpui::px(4.))
		.text_size(gpui::px(12.5))
		.line_height(gpui::px(BODY_LINE_HEIGHT))
		.text_color(ui_theme::rgb(BODY))
}
fn reader_title(title: &str) -> impl IntoElement {
	named_heading(title, title)
}

fn named_heading(id: &str, title: &str) -> impl IntoElement + use<> {
	gpui::div()
		.id(SharedString::from(format!("reference-heading-{id}")))
		.role(Role::Heading)
		.aria_level(3)
		.aria_label(title.to_owned())
		.py(gpui::px(5.))
		.font_weight(FontWeight::SEMIBOLD)
		.text_color(ui_theme::rgb(PRIMARY))
		.child(title.to_owned())
}
fn discussion_message(
	id: &str,
	byline: AnyElement,
	text: &str,
	truncated: bool,
	reply: bool,
	excerpt: Option<String>,
) -> impl IntoElement + use<> {
	let mut block = reading_body()
		.gap(gpui::px(2.))
		.when(reply, |block| {
			block
				.ml(gpui::px(3.))
				.pl(gpui::px(9.))
				.border_l_1()
				.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER))
		})
		.child(byline);
	if let Some(preview) = excerpt {
		let preview = if preview.ends_with('…') { preview } else { format!("{preview}…") };
		block = block.child(crate::shell::agent_surface::selectable_text::SelectableText::plain(
			format!("discussion-excerpt-{id}"),
			preview,
		));
	} else if text.is_empty() {
		block = block.child(note("The post text is unavailable."));
	} else {
		block = block.child(markdown::render(text, &format!("discussion-text-{id}")));
		if truncated {
			block = block.child(note("Showing an excerpt."));
		}
	}
	block
}

fn section(title: &str) -> impl IntoElement {
	gpui::div()
		.id(SharedString::from(format!("reference-heading-{title}")))
		.role(Role::Heading)
		.aria_level(3)
		.aria_label(title.to_owned())
		.pt(gpui::px(ui_theme::NAVIGATION_SECTION_GAP / 2.))
		.font_weight(FontWeight::MEDIUM)
		.text_size(gpui::px(CAPTION_SIZE))
		.text_color(ui_theme::rgb(SECONDARY))
		.child(title.to_owned())
}
fn note(text: &str) -> Div {
	gpui::div().min_w_0().text_color(ui_theme::rgb(SECONDARY)).child(text.to_owned())
}
