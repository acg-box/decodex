//! Source-backed collaboration across chats, with the existing conversation owners.
#[path = "agent_collaboration_cases.rs"] mod cases;

use std::{
	collections::{BTreeMap, BTreeSet},
	rc::Rc,
	time::Instant,
};

use gpui::{
	AnyElement, Div, FontWeight, KeyDownEvent, Role, SharedString, Stateful, Task,
	prelude::{
		FluentBuilder, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
		Styled,
	},
};
use tokio::runtime::Builder;

use crate::{
	shell::{
		agent_surface::{
			AgentClient, AgentSurface, Context, markdown,
			work_board::{
				ReferencePage, Row, activity,
				relations::{Edge, Kind},
			},
			workspace,
		},
		workspace_symbols::{self, Symbol},
	},
	ui_motion::{self, SmoothControl},
	ui_scroll::SmoothScrollArea,
	ui_theme::{
		self, BLUE, BODY_LINE_HEIGHT, CONTROL_SELECTED_FILL, HOVER_FILL, INFO, PRESSED_FILL,
		foreground::{BODY, CAPTION_SIZE, METRIC_SIZE, PRIMARY, SECONDARY},
	},
};
use decodex_protocol::{
	AgentExchangeDto, AgentExchangeQuestionDto, AgentExchangesResult, AgentTimelineContent,
	EntityId,
};

#[derive(Default)]
pub(in super::super) struct State {
	entries: BTreeMap<i64, AgentExchangeDto>,
	questions: Vec<AgentExchangeQuestionDto>,
	source: Option<EntityId>,
	next_before: Option<i64>,
	page: Option<i64>,
	questions_truncated: bool,
	task: Option<Task<()>>,
	read_at: Option<Instant>,
	earlier: bool,
	source_open: Option<String>,
	original_text: Option<String>,
	original_read: Option<Task<()>>,
	task_labels: BTreeMap<String, String>,
	failed: bool,
}

struct Exchange {
	id: String,
	from: Row,
	to: Option<Row>,
	title: String,
	summary: String,
	state: &'static str,
	color: u32,
	request: Option<String>,
	steps: Vec<(&'static str, String)>,
	source: String,
	source_text: String,
	source_row: Row,
	source_event: Option<i64>,
	source_item: Option<String>,
	source_turn: Option<String>,
	source_thread: Option<String>,
	edge: Option<Edge>,
	earlier: bool,
	question: Option<AgentExchangeQuestionDto>,
	order: i64,
}

impl AgentSurface {
	pub(in super::super) fn refresh_collaboration(&mut self, cx: &mut Context<Self>) {
		self.read_collaboration(self.work_board.collaboration.page, false, cx);
	}

	fn read_collaboration(&mut self, before: Option<i64>, explicit: bool, cx: &mut Context<Self>) {
		if !self.command_connection_ready() {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Some(source) =
			self.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.clone())
		else {
			return;
		};

		if self.work_board.collaboration.source.as_ref() != Some(&source) {
			if self.work_board.collaboration.source.is_some() {
				self.reset_reference();
			}
			self.work_board.collaboration =
				State { source: Some(source.clone()), ..Default::default() };
		}

		let state = &self.work_board.collaboration;

		if state.task.is_some()
			|| (!explicit && state.read_at.is_some_and(|at| at.elapsed().as_secs() < 2))
		{
			return;
		}

		let (unresolved, work_ids) = self.missing_task_labels();
		let state = &mut self.work_board.collaboration;
		state.read_at = Some(Instant::now());

		if explicit {
			state.page = before;
		}

		let epoch = self.command_epoch;
		let read = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().map_err(|_| ())?;

			Ok(runtime.block_on(async move {
				let client = AgentClient::new(profile);
				let exchanges = client.exchanges(before).await;
				let labels = read_task_labels(&client, unresolved, work_ids).await;
				(exchanges, labels)
			}))
		});

		self.work_board.collaboration.task = Some(cx.spawn(async move |surface, cx| {
			let result = read.await;
			let _ = surface.update(cx, |s, cx| {
				if s.command_epoch != epoch
					|| s.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.as_ref())
						!= Some(&source)
				{
					return;
				}

				let state = &mut s.work_board.collaboration;

				state.task = None;

				let (result, labels) = match result {
					Ok(pair) => (pair.0.ok(), pair.1),
					Err(()) => (None, BTreeMap::new()),
				};
				let labels_changed = !labels.is_empty();
				state.task_labels.extend(labels);
				let changed = match result {
					Some(AgentExchangesResult::Available {
						source: observed,
						entries,
						questions,
						questions_truncated,
						next_before,
					}) if observed == source => {
						let entries =
							entries.into_iter().map(|entry| (entry.event_id, entry)).collect();
						let changed = state.failed
							|| state.entries != entries
							|| state.questions != questions
							|| state.questions_truncated != questions_truncated
							|| state.next_before != next_before
							|| explicit;

						state.failed = false;
						// Refresh the selected page; an old assessment must not stay cached as
						// current.
						state.entries = entries;
						state.questions = questions;
						state.questions_truncated = questions_truncated;
						state.next_before = next_before;

						changed
					},
					_ => {
						let changed = !state.failed;

						state.failed = true;

						changed
					},
				};

				if changed || labels_changed {
					cx.notify();
				}
			});
		}));
	}

	fn missing_task_labels(&self) -> (BTreeSet<String>, Vec<EntityId>) {
		let mut unresolved = BTreeSet::new();
		if !self.workspace.agent_tree_visible {
			return (unresolved, Vec::new());
		}
		let Some(binding) = &self.timeline.native.binding else {
			return (unresolved, Vec::new());
		};
		for entry in &self.timeline.native.entries {
			if let AgentTimelineContent::Item {
				activity: Some(activity),
				collaboration: Some(call),
				..
			} = &entry.content
				&& call.sender_thread_id == binding.thread
				&& activity.kind == "subAgentActivity"
				&& let Some(name) = activity.detail.rsplit('/').next()
				&& managed_task_name(name)
				&& !self.work_board.collaboration.task_labels.contains_key(name)
			{
				unresolved.insert(name.to_owned());
			}
		}
		let work_ids = self
			.snapshot
			.iter()
			.flat_map(|snapshot| &snapshot.portfolio.works)
			.filter(|work| !unresolved.is_empty() && work.owner_agent_id == binding.work)
			.filter_map(|work| EntityId::new(&work.id).ok())
			.collect();
		(unresolved, work_ids)
	}

	fn collaboration_cases(&self, edges: &[Edge], rows: &[Row]) -> Vec<Exchange> {
		let state = &self.work_board.collaboration;
		let rebound = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| snapshot.runtime_source.as_ref())
			.is_some_and(|source| state.source.as_ref() != Some(source));
		let records: Vec<_> =
			if rebound { vec![] } else { state.entries.values().cloned().collect() };
		let questions = if rebound { &[][..] } else { state.questions.as_slice() };
		let mut cases = cases::exchanges(edges, rows, &records, questions);
		self.append_review_cases(&mut cases, rows);

		for row in rows
			.iter()
			.filter(|row| row.native && row.group != 1 && self.reviewer_role(row).is_none())
		{
			let Some(brief) = self.work_board.briefs.get(&row.key).filter(|brief| {
				brief.source_thread == row.thread && brief.stamp == self.brief_stamp(row)
			}) else {
				continue;
			};
			let (Some(output), Some((turn, status))) = (&brief.output, &brief.outcome) else {
				continue;
			};

			if output.turn != *turn
				|| status != "completed"
				|| brief.metrics.stats.is_some_and(|stats| stats.active_ms.is_some())
			{
				continue;
			}

			let Some(thread) = row.thread.as_ref() else {
				continue;
			};
			let Some(parent) = self
				.native_agents
				.lists
				.get(&row.work)
				.and_then(|agents| agents.iter().find(|agent| &agent.thread_id == thread))
				.and_then(|agent| {
					rows.iter()
						.find(|parent| parent.thread.as_ref() == Some(&agent.parent_thread_id))
				})
			else {
				continue;
			};

			// This is the child's reported output, not a reconstructed reply to a parent call.
			// Compact parent notifications remain separate source records in Earlier.
			for case in &mut cases {
				if case
					.edge
					.as_ref()
					.is_some_and(|edge| edge.kind == Kind::Returned && edge.from == row.key)
				{
					case.earlier = true;
				}
			}

			let mut source_text: String = output.text.chars().take(640).collect();

			if output.truncated || output.text.chars().count() > 640 {
				source_text.push('…');
			}

			cases.push(Exchange {
				id: format!("native-report:{thread}:{turn}"),
				from: row.clone(),
				to: Some(parent.clone()),
				title: brief
					.preview
					.as_ref()
					.filter(|preview| preview.turn == *turn && !preview.task.is_empty())
					.map(|preview| preview.task.clone())
					.or_else(|| self.native_assignment(row))
					.unwrap_or_else(|| "Reported output".into()),
				summary: activity::text(&output.text).chars().take(180).collect(),
				state: "Reported",
				color: INFO,
				request: None,
				steps: vec![("Review", "Acceptance and reply linkage are unknown.".into())],
				source: "Native report".into(),
				source_text,
				source_row: row.clone(),
				source_event: None,
				source_item: None,
				source_turn: Some(turn.clone()),
				source_thread: Some(thread.clone()),
				edge: None,
				earlier: false,
				question: None,
				order: 0,
			});
		}

		cases
	}

	fn append_review_cases(&self, cases: &mut Vec<Exchange>, rows: &[Row]) {
		for row in rows {
			let role = self.reviewer_role(row);
			let Some(evidence) = role
				.as_ref()
				.and_then(|_| self.review_evidence(row))
				.or_else(|| self.lead_review(row))
			else {
				continue;
			};
			let Some(phase) = self.review_phase(row) else { continue };
			// The reviewer entry already shows this handoff. Keep Lead entries for
			// preparation, findings and decisions rather than repeating the same wait.
			if role.is_none() && phase == "Waiting for review" {
				continue;
			}
			// Keep the native question's reply authority and exact pending state.
			if let Some(question) = cases.iter_mut().find(|case| {
				case.question.as_ref().is_some_and(|q| {
					q.pending && q.work_id == row.work && Some(&q.thread_id) == row.thread.as_ref()
				})
			}) {
				question.title = "Review decision".into();
				continue;
			}
			let parent = role.as_ref().and_then(|_| self.reviewer_parent(row, rows));
			let output = self.work_output(row);
			let completed =
				output.as_ref().is_some_and(|output| self.work_output_complete(row, output.turn));
			// The same native lifecycle is already represented by this review entry.
			// Leave peer messages and managed Work receipts intact.
			cases.retain(|case| {
				!case.edge.as_ref().is_some_and(|edge| {
					matches!(edge.kind, Kind::Returned | Kind::Wait)
						&& (edge.from == row.key || edge.to == row.key)
				})
			});
			let summary = output
				.as_ref()
				.map(|output| {
					if role.is_some() {
						review_excerpt(output.text)
					} else {
						activity::wording::message(output.text)
					}
				})
				.filter(|text| !text.is_empty())
				.unwrap_or_else(|| {
					role.as_ref().map_or("Using the original EGM workflow.".into(), |role| {
						format!("Independent {role} review in progress.")
					})
				});
			let summary = if role.is_none() && row.group != 1 {
				match evidence.step {
					decodex_protocol::AgentReviewActivity::Assess =>
						"Open Lead to inspect the findings and choose the next step.",
					_ => "Open Lead to inspect the review setup and continue.",
				}
				.into()
			} else {
				summary
			};
			let title = self.reviewer_title(row).unwrap_or_else(|| {
				match evidence.step {
					decodex_protocol::AgentReviewActivity::Assess => "Review findings",
					_ => "Review setup",
				}
				.into()
			});
			let source_text = output.as_ref().map_or_else(
				|| "Original EGM instructions were read successfully.".into(),
				|output| {
					let mut text: String = output.text.chars().take(1_200).collect();
					if output.truncated || output.text.chars().count() > 1_200 {
						text.push('…');
					}
					text
				},
			);
			cases.push(Exchange {
				id: format!(
					"review:{}:{}",
					row.thread.as_deref().unwrap_or(&row.key),
					evidence.turn
				),
				from: row.clone(),
				to: parent.cloned(),
				title,
				summary,
				state: phase,
				color: if row.group == 1 || role.is_some() { INFO } else { ui_theme::WARNING },
				request: None,
				steps: vec![
					(
						"Scope",
						if role.is_some() {
							"Independent reviewer using original EGM instructions"
						} else {
							"Lead using the original EGM workflow"
						}
						.into(),
					),
					(
						"Outcome",
						if completed && role.is_some() {
							"Report returned. Open Lead to review the next step."
						} else {
							"Continue in the original conversation."
						}
						.into(),
					),
				],
				source: "EGM activity".into(),
				source_text,
				source_row: row.clone(),
				source_event: None,
				source_item: (!completed).then_some(evidence.item),
				source_turn: Some(output.map_or(evidence.turn, |output| output.turn.into())),
				source_thread: row.thread.clone(),
				edge: None,
				earlier: false,
				question: None,
				order: i64::MAX - 1,
			});
		}
	}

	fn reviewer_parent<'a>(&self, row: &Row, rows: &'a [Row]) -> Option<&'a Row> {
		let agent = self
			.native_agents
			.lists
			.get(&row.work)?
			.iter()
			.find(|agent| Some(&agent.thread_id) == row.thread.as_ref())?;
		rows.iter().find(|parent| parent.thread.as_ref() == Some(&agent.parent_thread_id))
	}

	fn reference_control(
		&self,
		id: String,
		label: String,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let action = Rc::new(action);
		let keyboard = action.clone();

		gpui::div()
			.id(SharedString::from(id.clone()))
			.debug_selector(move || id.clone())
			.role(Role::Button)
			.aria_label(label.clone())
			.tab_index(0)
			.min_w_0()
			.min_h(gpui::px(26.))
			.flex_none()
			.px(gpui::px(8.))
			.flex()
			.items_center()
			.justify_center()
			.gap(gpui::px(6.))
			.rounded(gpui::px(6.))
			.bg(ui_theme::rgba(HOVER_FILL))
			.text_size(gpui::px(METRIC_SIZE))
			.line_height(gpui::px(18.))
			.font_weight(FontWeight::MEDIUM)
			.text_color(ui_theme::rgb(PRIMARY))
			.cursor_pointer()
			.hover(|style| style.bg(ui_theme::rgba(PRESSED_FILL)))
			.active(|style| style.bg(ui_theme::rgba(CONTROL_SELECTED_FILL)))
			.focus_visible(|style| {
				style.bg(ui_theme::rgba(CONTROL_SELECTED_FILL)).text_color(ui_theme::rgb(BLUE))
			})
			.on_click(cx.listener(move |s, _, window, cx| {
				ui_motion::begin_interaction();
				action(s, cx);

				workspace::restore_shell_focus(window, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, window, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					ui_motion::begin_interaction();
					keyboard(s, cx);

					workspace::restore_shell_focus(window, cx);

					cx.stop_propagation();
				}
			}))
	}

	fn collaboration_source_field(
		&self,
		case: &Exchange,
		label: &'static str,
		value: &str,
	) -> AnyElement {
		let key = format!("collaboration-reference-{}-{label}", case.id);

		gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(8.))
			.child(gpui::div().w(gpui::px(40.)).flex_none().child(label))
			.child(
				gpui::div()
					.debug_selector({
						let key = key.clone();

						move || key.clone()
					})
					.flex_1()
					.min_w_0()
					.whitespace_nowrap()
					.text_ellipsis()
					.overflow_hidden()
					.child(value.to_owned()),
			)
			.child(markdown::copy_button(
				&format!("copy-{key}"),
				match label {
					"Thread" => "Copy thread ID",
					"Turn" => "Copy turn ID",
					"Item" => "Copy item ID",
					"Event" => "Copy event ID",
					_ => "Copy source ID",
				},
				value.to_owned(),
			))
			.into_any_element()
	}

	fn collaboration_entry_visible(&self, entry: &super::Entry, cases: &[Exchange]) -> bool {
		!entry.result
			&& (entry.request.is_some() || self.review_phase(&entry.row).is_none())
			&& (entry.request.is_some()
				|| !cases.iter().any(|case| {
					case.question.as_ref().is_some_and(|question| {
						question.pending && question.work_id == entry.row.work
					})
				}))
	}
}

#[path = "agent_reference_reader.rs"] mod reader;

fn same_excerpt(a: &str, b: &str) -> bool {
	a.split_whitespace().eq(b.split_whitespace())
}

fn review_excerpt(text: &str) -> String {
	// Select original report text, not a computed verdict or an acceptance signal.
	let paragraph = text
		.split("\n\n")
		.find(|paragraph| {
			let lower = paragraph.to_ascii_lowercase();
			["blocker", "major", "nit"].iter().any(|severity| {
				lower.split(|c: char| !c.is_ascii_alphabetic()).any(|word| word == *severity)
			})
		})
		.unwrap_or(text);
	activity::wording::message(paragraph)
}

#[cfg(test)]
#[path = "agent_collaboration_tests.rs"]
mod tests;

fn needs_user(case: &Exchange) -> bool {
	!case.earlier
		&& (case.question.as_ref().is_some_and(|question| question.pending)
			|| case.state == "Decision")
}

// Decodex uses this deterministic name when preparing a managed native contribution.
fn managed_task_name(name: &str) -> bool {
	name.strip_prefix("work_").is_some_and(|hash| {
		hash.len() == 32 && hash.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
	})
}

async fn read_task_labels(
	client: &AgentClient,
	mut missing: BTreeSet<String>,
	works: Vec<EntityId>,
) -> BTreeMap<String, String> {
	let mut labels = BTreeMap::new();
	let mut complete = !works.is_empty();
	for id in works {
		let Ok(detail) = client.work_detail(id).await else {
			complete = false;
			continue;
		};
		complete &= !detail.has_more;
		for contribution in detail.native_contributions {
			let name = contribution.native_task_name();
			missing.remove(&name);
			labels.insert(
				name,
				activity::wording::purpose(&contribution.id.replace(['_', '-'], " ")),
			);
		}
		if missing.is_empty() {
			break;
		}
	}
	if complete {
		// An unregistered path can look like a managed task. Keep its number off the screen.
		for name in missing {
			labels.insert(name, String::new());
		}
	}
	labels
}
