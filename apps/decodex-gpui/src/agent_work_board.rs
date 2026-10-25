//! Source-backed agent relationships and execution previews.
#[path = "agent_relation_activity.rs"] mod activity;
#[path = "agent_relation_canvas.rs"] mod relations;
#[path = "agent_workbench.rs"] mod workbench;

use std::{
	collections::{BTreeMap, BTreeSet},
	sync::Arc,
};

use gpui::{AnyElement, Task, TextSystem, prelude::FluentBuilder};
use tokio::runtime::Builder;

use crate::{
	shell::agent_surface::{
		AgentClient, AgentHistoryResult, AgentSurface, ClientProfile, Context, EntityId,
		InteractiveElement, IntoElement, ParentElement, Styled, dock,
		ui_theme::{SUCCESS, TEXT_MUTED, WARNING},
	},
	ui_theme::{ERROR, INFO},
};
use activity::{OperationKind, OutputRef, Preview, wording};
use decodex_protocol::{
	AgentCollaborationDto, AgentResourceDto, AgentResourcesResult, AgentTimelineContent,
	AgentTimelineEntry, AgentTimelineResult,
};
use relations::View;
use workbench::Collaboration;

pub(super) struct Board {
	text_system: Arc<TextSystem>,
	pub(super) graph: bool,
	collaboration_expanded: Option<String>,
	collaboration: Collaboration,
	focus: Option<String>,
	view: View,
	pub(super) briefs: BTreeMap<String, Brief>,
	usage: Option<BTreeMap<String, (Option<String>, crate::ui_work_metrics::Sample)>>,
	token_gaps: usize,
	pub(super) brief_task: Option<Task<()>>,
	reload_pending: BTreeSet<String>,
}
impl Board {
	pub(super) fn focus_work(&mut self, id: &str) {
		self.focus = Some(id.into());
	}

	pub(super) fn clear_focus(&mut self) {
		self.focus = None;
	}

	pub(super) fn new(cx: &mut Context<AgentSurface>) -> Self {
		Self {
			text_system: cx.text_system().clone(),
			graph: false,
			collaboration_expanded: None,
			collaboration: Default::default(),
			focus: None,
			view: Default::default(),
			briefs: Default::default(),
			usage: None,
			token_gaps: 0,
			brief_task: None,
			reload_pending: Default::default(),
		}
	}

	pub(super) fn clear(&mut self, cx: &mut Context<AgentSurface>) {
		*self = Self::new(cx);
	}
}

#[derive(Clone)]
pub(super) struct Row {
	key: String,
	work: String,
	thread: Option<String>,
	native: bool,
	title: String,
	owner: String,
	workspace: Option<String>,
	status: String,
	group: u8,
	color: u32,
	review: bool,
}

pub(super) struct Brief {
	source_thread: Option<String>,
	output: Option<activity::Output>,
	assignments: BTreeMap<String, String>,
	outcome: Option<(String, String)>,
	preview: Option<Preview>,
	question: Option<String>,
	resources: Option<Vec<AgentResourceDto>>,
	relations: Vec<(String, AgentCollaborationDto)>,
	metrics: crate::ui_work_metrics::Sample,
	stamp: String,
	read_at: std::time::Instant,
}
impl Brief {
	fn retain_relations(&mut self, previous: &Self) {
		if self.source_thread != previous.source_thread {
			return;
		}

		let mut records = previous.relations.clone();

		for (item, call) in &self.relations {
			if let Some((_, observed)) = records.iter_mut().find(|(id, _)| id == item) {
				*observed = call.clone();
			} else {
				records.push((item.clone(), call.clone()));
			}
		}

		if records.len() > 64 {
			records.drain(..records.len() - 64);
		}

		self.relations = records;
	}
}

impl AgentSurface {
	pub(crate) fn work_statistics(&self) -> Option<crate::ui_work_metrics::Summary> {
		if self.workspace_connecting() {
			return None;
		}

		let rows = self.factory_rows();

		if rows.is_empty() {
			return None;
		}

		let samples = rows
			.iter()
			.map(|row| {
				self.work_board
					.usage
					.as_ref()
					.and_then(|usage| usage.get(&row.key))
					.filter(|(thread, _)| thread == &row.thread)
					.map(|(_, sample)| *sample)
					.unwrap_or_default()
			})
			.collect();
		let mut live: Vec<_> = rows.iter().filter_map(|row| self.live_work_activity(row)).collect();

		live.sort_by_key(|row| {
			self.snapshot.as_ref().and_then(|s| s.agent_slots.get(&row.key)).copied()
		});

		Some(crate::ui_work_metrics::Summary {
			working: rows.iter().filter(|row| row.group == 1).count(),
			samples,
			live,
		})
	}

	fn live_work_activity(&self, row: &Row) -> Option<crate::ui_work_metrics::Live> {
		if row.group != 1 || self.graph_question(row).is_some() {
			return None;
		}

		let brief = self
			.work_board
			.briefs
			.get(&row.key)
			.filter(|brief| brief.source_thread == row.thread)?;
		let sample = brief.metrics;

		sample.active_time()?;

		let turn = self
			.snapshot
			.as_ref()?
			.work_items
			.iter()
			.find(|work| work.id == row.work && work.codex_thread_id == row.thread)
			.and_then(|work| work.active_turn_id.as_deref());

		if turn
			.is_some_and(|turn| brief.preview.as_ref().is_none_or(|preview| preview.turn != turn))
		{
			return None;
		}

		let action = if sample.stats?.generating {
			"Generating".into()
		} else {
			let current = self
				.timeline
				.native
				.binding
				.as_ref()
				.filter(|binding| {
					binding.work == row.work && row.thread.as_ref() == Some(&binding.thread)
				})
				.and_then(|_| activity::latest(&self.timeline.native.entries, turn));

			current
				.as_ref()
				.or(brief.preview.as_ref())
				.filter(|preview| turn.is_none_or(|turn| preview.turn == turn))
				.and_then(|preview| preview.running.clone())
				.unwrap_or_else(|| row.status.clone())
		};
		let slot = self.snapshot.as_ref()?.agent_slots.get(&row.key)?;

		Some(crate::ui_work_metrics::Live {
			key: row.key.clone(),
			agent: format!("A{slot}"),
			action,
			sample,
		})
	}

	pub(super) fn token_statistics_gap(&self) -> usize {
		self.work_board.token_gaps
	}

	pub(super) fn debug_reload_graph(&mut self) {
		self.work_board.brief_task = None;
		// Keep visible values while forcing every brief to be read again.
		self.work_board.reload_pending = self.work_board.briefs.keys().cloned().collect();
	}

	fn board_rows(&self) -> Vec<Row> {
		let Some(snapshot) = &self.snapshot else { return Vec::new() };
		let workspace_for = |id: &str| {
			snapshot
				.projects
				.iter()
				.find(|w| w.agent_ids.iter().any(|work| work == id))
				.map(|w| w.id.clone())
		};
		let handoffs = self.handoff_items();
		let mut rows = Vec::new();
		let mut threads = BTreeSet::new();

		for work in &snapshot.work_items {
			if let Some(thread) = &work.codex_thread_id {
				threads.insert(thread.clone());
			}

			let state = dock::progress_state(snapshot, work);
			let owner = work
				.parent_goal_id
				.as_ref()
				.and_then(|id| snapshot.work_items.iter().find(|w| &w.id == id));
			let review = handoffs.iter().any(|h| h.work == work.id && h.result && h.attention);
			let (status, group, color) = if self.work_is_archived(&work.id) {
				("Archived", 2, TEXT_MUTED)
			} else {
				(state.label, state.group, state.color)
			};

			rows.push(Row {
				key: work.id.clone(),
				work: work.id.clone(),
				thread: work.codex_thread_id.clone(),
				native: false,
				title: self.work_label(work),
				owner: owner.map(|w| self.work_label(w)).unwrap_or_default(),

				workspace: workspace_for(&work.id),
				status: status.into(),
				review,
				group,
				color,
			});
		}
		for (owner, agents) in &self.native_agents.lists {
			if !snapshot.work_items.iter().any(|w| &w.id == owner) {
				continue;
			}

			for agent in agents {
				if !threads.insert(agent.thread_id.clone()) {
					continue;
				}

				let (status, group, color) = native_state(&agent.status);
				let task = agent.task.split_whitespace().collect::<Vec<_>>().join(" ");

				rows.push(Row {
					key: format!("native:{owner}:{}", agent.thread_id),
					work: owner.clone(),
					thread: Some(agent.thread_id.clone()),
					native: true,
					title: if task.is_empty() { agent.title.clone() } else { task },
					owner: agent.title.clone(),
					workspace: workspace_for(owner),
					status: status.into(),
					review: false,
					group,
					color,
				});
			}
		}
		for row in &mut rows {
			if let Some(brief) = self.work_board.briefs.get(&row.key)
				&& brief.stamp == self.brief_stamp(row)
				&& let Some((turn, outcome)) = &brief.outcome
				&& (row.native
					|| snapshot.work_items.iter().any(|w| {
						w.id == row.work
							&& w.active_turn_id.as_ref().is_none_or(|active| active == turn)
					})) {
				apply_outcome(row, outcome);
			}
		}

		rows.sort_by(|a, b| a.key.cmp(&b.key));

		rows
	}
}

impl AgentSurface {
	fn graph_question(&self, row: &Row) -> Option<&str> {
		if self.work_is_archived(&row.key) {
			return None;
		}
		if !row.native
			&& self.conversation_matches(&row.work, row.thread.as_deref().unwrap_or_default())
			&& let Some((owner, AgentHistoryResult::Available { questions, .. })) = &self.history
			&& owner == &row.work
		{
			return questions.last().map(|question| question.title.as_str());
		}

		self.work_board
			.briefs
			.get(&row.key)
			.filter(|brief| {
				brief.question.is_some()
					&& brief.source_thread == row.thread
					&& brief.stamp == self.brief_stamp(row)
			})
			.and_then(|brief| brief.question.as_deref())
	}

	fn work_output(&self, row: &Row) -> Option<OutputRef<'_>> {
		let brief =
			self.work_board.briefs.get(&row.key).filter(|brief| brief.source_thread == row.thread);
		let turn = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| {
				snapshot
					.work_items
					.iter()
					.find(|work| work.id == row.work && work.codex_thread_id == row.thread)
			})
			.and_then(|work| work.active_turn_id.as_deref());
		let current = self
			.timeline
			.native
			.binding
			.as_ref()
			.filter(|binding| {
				binding.work == row.work && row.thread.as_ref() == Some(&binding.thread)
			})
			.and_then(|_| activity::latest_output(&self.timeline.native.entries));

		current
			.or_else(|| {
				brief.and_then(|brief| brief.output.as_ref()).map(activity::Output::borrowed)
			})
			.filter(|output| turn.is_none_or(|turn| output.turn == turn))
	}

	fn work_activity_summary(&self, row: &Row) -> (String, String) {
		if let Some(question) = self.graph_question(row) {
			return ("Needs your answer".into(), question.into());
		}

		if row.group == 1
			&& let Some(live) = self.live_work_activity(row)
		{
			return ("Now".into(), live.action);
		}

		if let Some(output) = self.work_output(row) {
			let complete = self.work_output_complete(row, output.turn);

			if row.group != 1 || !complete {
				return (
					if complete { "Latest result" } else { "Latest update" }.into(),
					activity::text(output.text),
				);
			}
		}

		let (_, operation) = self.graph_preview(row);

		if !operation.is_empty()
			&& operation != row.status
			&& operation != "Exit status unavailable"
		{
			return (if row.group == 1 { "Now" } else { "Last activity" }.into(), operation);
		}

		(row.status.clone(), String::new())
	}

	fn work_output_complete(&self, row: &Row, turn: &str) -> bool {
		self.work_board
			.briefs
			.get(&row.key)
			.filter(|brief| brief.source_thread == row.thread)
			.and_then(|brief| brief.outcome.as_ref())
			.is_some_and(|(observed, status)| observed == turn && status == "completed")
	}

	fn graph_preview(&self, row: &Row) -> (String, String) {
		if self.work_is_archived(&row.key) {
			return (row.title.clone(), "Archived conversation".into());
		}

		let work =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == row.work));
		let turn = work
			.filter(|w| w.codex_thread_id == row.thread)
			.and_then(|w| w.active_turn_id.as_deref());
		let current = if self
			.conversation_matches(&row.work, row.thread.as_deref().unwrap_or_default())
			&& let Some(binding) = &self.timeline.native.binding
			&& binding.work == row.work
			&& row.thread.as_ref() == Some(&binding.thread)
		{
			activity::latest(&self.timeline.native.entries, turn)
		} else {
			None
		};
		let preview = current.as_ref().or_else(|| {
			self.work_board
				.briefs
				.get(&row.key)
				.filter(|brief| brief.source_thread == row.thread)
				.and_then(|brief| brief.preview.as_ref())
				.filter(|preview| turn.is_none_or(|turn| preview.turn == turn))
		});
		// Native nicknames are not responsibilities. Missing assignment metadata
		// can use the exact native input; assistant replies never supply this label.
		let task = if row.native && row.title == row.owner {
			preview
				.filter(|preview| !preview.task.is_empty())
				.map(|preview| preview.task.clone())
				.or_else(|| self.native_assignment(row))
				.unwrap_or_default()
		} else if !row.native {
			preview
				.filter(|preview| !preview.task.is_empty())
				.map(|preview| preview.task.clone())
				.unwrap_or_else(|| row.title.clone())
		} else {
			row.title.clone()
		};
		let operation = self
			.graph_question(row)
			.map(str::to_owned)
			.or_else(|| preview.map(|preview| preview.text.clone()))
			.unwrap_or_default();

		(task, operation)
	}

	fn native_assignment(&self, row: &Row) -> Option<String> {
		let thread = row.thread.as_ref()?;
		let agent = self
			.native_agents
			.lists
			.get(&row.work)?
			.iter()
			.find(|agent| &agent.thread_id == thread)?;
		let brief = self
			.work_board
			.briefs
			.values()
			.find(|brief| brief.source_thread.as_ref() == Some(&agent.parent_thread_id))?;

		brief.assignments.get(thread).cloned()
	}

	fn graph_purpose(&self, row: &Row) -> String {
		let input = self.graph_preview(row).0;
		let source = if row.native {
			self.native_assignment(row).unwrap_or(input)
		} else if self.root_id().as_ref() == Some(&row.work)
			|| (row.title.chars().count() == 64
				&& !wording::message(&row.title).ends_with(['.', '?', '!', '。', '？', '！'])
				&& !input.is_empty())
		{
			// Older saved conversations used a 64-character prompt prefix.
			// Use a complete observed request when that prefix has no sentence.
			input
		} else {
			row.title.clone()
		};
		let purpose = wording::purpose(&source);

		if purpose == self.relation_agent_number(row) || purpose == self.relation_agent_name(row) {
			String::new()
		} else {
			purpose
		}
	}

	fn graph_operations(&self, row: &Row) -> (String, Vec<activity::Operation>) {
		let turn = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| {
				snapshot
					.work_items
					.iter()
					.find(|work| work.id == row.work && work.codex_thread_id == row.thread)
			})
			.and_then(|work| work.active_turn_id.as_deref());
		let current = self
			.timeline
			.native
			.binding
			.as_ref()
			.filter(|binding| {
				binding.work == row.work && row.thread.as_ref() == Some(&binding.thread)
			})
			.and_then(|_| activity::latest(&self.timeline.native.entries, turn));
		let preview = current
			.as_ref()
			.or_else(|| {
				self.work_board
					.briefs
					.get(&row.key)
					.filter(|brief| brief.source_thread == row.thread)
					.and_then(|brief| brief.preview.as_ref())
			})
			.filter(|preview| turn.is_none_or(|turn| preview.turn == turn));
		let scope = format!(
			"{}:{}",
			row.thread.as_deref().unwrap_or(&row.key),
			turn.or_else(|| preview.map(|preview| preview.turn.as_str())).unwrap_or_default()
		);
		let question = self.graph_question(row);

		if let Some(question) = question {
			return (
				scope,
				vec![activity::Operation {
					id: format!("question:{question}"),
					text: activity::plain(question),
					kind: activity::OperationKind::Question,
				}],
			);
		}

		let mut operations = preview.map(|preview| preview.recent.clone()).unwrap_or_default();

		if operations.is_empty()
			&& self.live_work_activity(row).is_none()
			&& let Some(output) = self.work_output(row)
		{
			operations.push(activity::Operation {
				id: format!("{}:output", output.turn),
				text: wording::message(output.text),
				kind: if self.work_output_complete(row, output.turn) {
					OperationKind::Result
				} else {
					OperationKind::Message
				},
			});
		}
		// Retain native messages, but do not invent an operation for a quiet thread.
		if operations.is_empty() && row.group != 2 {
			let text = self.work_activity_summary(row).1;

			if !text.is_empty() {
				operations.push(activity::Operation {
					id: "latest".into(),
					text,
					kind: OperationKind::Action,
				});
			}
		}

		(scope, operations)
	}

	fn factory_rows(&self) -> Vec<Row> {
		self.board_rows().into_iter().filter(|row| !self.work_is_archived(&row.key)).collect()
	}

	fn brief_stamp(&self, row: &Row) -> String {
		let work =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == row.work));

		if row.native {
			// Parent progress and sibling events must not invalidate a child's preview.
			return serde_json::json!([
				work.map(|w| &w.codex_thread_id),
				self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref()),
				row.thread,
				self.native_agents
					.lists
					.get(&row.work)
					.and_then(|agents| agents
						.iter()
						.find(|agent| Some(&agent.thread_id) == row.thread.as_ref()))
					.map(|agent| &agent.status)
			])
			.to_string();
		}

		serde_json::json!([
			work.map(|w| self.dock_evidence_key(w)),
			row.thread,
			self.native_agents
				.lists
				.get(&row.work)
				.and_then(|agents| agents
					.iter()
					.find(|a| Some(&a.thread_id) == row.thread.as_ref()))
				.map(|a| &a.status)
		])
		.to_string()
	}

	// Publish a complete read together so partial batches cannot resize the usage strip.
	fn publish_usage(&mut self) -> bool {
		if self.native_agent_discovery_pending() {
			return false;
		}

		let rows = self.factory_rows();

		if rows.iter().any(|row| self.work_board.reload_pending.contains(&row.key)) {
			return false;
		}

		let mut usage = BTreeMap::new();
		let mut token_gaps = 0;

		for row in rows.iter().filter(|row| row.thread.is_some()) {
			let Some(brief) = self
				.work_board
				.briefs
				.get(&row.key)
				.filter(|brief| brief.source_thread == row.thread)
			else {
				return false;
			};

			if brief.metrics.tokens.is_none()
				&& brief
					.metrics
					.stats
					.is_some_and(|stats| stats.active_ms.is_none() && !stats.generating)
				&& brief.outcome.as_ref().is_some_and(|(_, status)| status == "completed")
			{
				token_gaps += 1;
			}

			usage.insert(row.key.clone(), (row.thread.clone(), brief.metrics));
		}

		let changed = self.work_board.usage.as_ref() != Some(&usage)
			|| self.work_board.token_gaps != token_gaps;

		self.work_board.usage = Some(usage);
		self.work_board.token_gaps = token_gaps;

		changed
	}

	fn read_factory_briefs(
		profile: ClientProfile,
		requests: Vec<(Row, String)>,
		cx: &Context<Self>,
	) -> Task<Vec<(String, Brief)>> {
		cx.background_executor().spawn(async move {
			let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
				return Vec::new();
			};

			runtime.block_on(async move {
				let client = AgentClient::new(profile);
				let mut results = Vec::new();

				for (row, stamp) in requests {
					let (Ok(owner), Some(thread)) = (
						EntityId::new(&row.work),
						row.thread.as_deref().and_then(|t| EntityId::new(t).ok()),
					) else {
						continue;
					};
					let mut relations = Vec::new();
					let mut outcome = None;
					let mut preview = None;
					let mut output = None;
					let mut assignments = BTreeMap::new();
					let mut metrics = crate::ui_work_metrics::Sample::default();
					let mut available = false;

					if let Ok(result) = client.timeline(owner.clone(), thread.clone(), None).await
						&& let AgentTimelineResult::Available { work_id, page, .. } = &result
						&& work_id.as_str() == owner.as_str()
						&& page.thread_id == thread.as_str()
					{
						available = true;
						metrics = crate::ui_work_metrics::Sample::from_page(page);
						outcome = latest_outcome(&page.entries);
						preview = activity::latest(&page.entries, None);
						output = activity::output(&page.entries);
						assignments = activity::assignments(&page.entries, &page.thread_id);
						relations = page
							.entries
							.iter()
							.filter_map(|entry| match &entry.content {
								AgentTimelineContent::Item {
									item_id,
									collaboration: Some(call),
									..
								} if call.sender_thread_id == page.thread_id => Some((item_id.clone(), call.clone())),
								_ => None,
							})
							.collect();
					}

					if !available {
						// A transient read failure must not erase the last visible content or
						// counters.
						continue;
					}

					let question = if row.native {
						None
					} else {
						match client.history(owner.clone()).await {
							Ok(AgentHistoryResult::Available { questions, .. }) =>
								questions.last().map(|question| question.title.clone()),
							_ => None,
						}
					};
					let resources = if row.native {
						None
					} else {
						match client.resources(owner).await {
							Ok(AgentResourcesResult::Available { resources }) => Some(resources),
							_ => None,
						}
					};

					results.push((
						row.key,
						Brief {
							output,
							source_thread: row.thread.clone(),
							assignments,
							outcome,
							preview,
							question,
							stamp,
							relations,
							resources,
							metrics,
							read_at: std::time::Instant::now(),
						},
					));
				}

				results
			})
		})
	}

	pub(super) fn refresh_factory_briefs(&mut self, cx: &mut Context<Self>) {
		self.refresh_collaboration(cx);

		if self.work_board.brief_task.is_some() || !self.command_connection_ready() {
			return;
		}

		let Some(profile) = self.profile.clone() else { return };
		let mut rows = self.factory_rows();

		rows.retain(|r| r.thread.is_some());
		rows.sort_by_key(|r| (r.group != 1, self.work_board.briefs.get(&r.key).map(|b| b.read_at)));

		let requests: Vec<_> = rows
			.into_iter()
			.filter(|r| {
				self.work_board.reload_pending.contains(&r.key)
					|| self.work_board.briefs.get(&r.key).is_none_or(|b| {
						b.stamp != self.brief_stamp(r)
							|| ((r.group == 1 || b.question.is_some())
								&& b.read_at.elapsed().as_secs() >= 1)
							|| b.read_at.elapsed().as_secs() >= 30
					})
			})
			.take(4)
			.map(|row| {
				let stamp = self.brief_stamp(&row);

				(row, stamp)
			})
			.collect();

		if requests.is_empty() {
			if self.publish_usage() {
				cx.notify();
			}

			return;
		}

		let read = Self::read_factory_briefs(profile, requests, cx);

		self.work_board.brief_task = Some(cx.spawn(async move |surface, cx| {
			let results = read.await;
			let _ =
				surface.update(cx, |s, cx| {
					let rows = s.board_rows();

					s.work_board.briefs.retain(|key, _| rows.iter().any(|r| &r.key == key));

					for (key, mut brief) in results {
						if rows.iter().any(|r| r.key == key && s.brief_stamp(r) == brief.stamp) {
							if let Some(previous) =
								s.work_board.briefs.get(&key).filter(|previous| {
									previous.source_thread == brief.source_thread
								}) {
								brief.retain_relations(previous);

								// Keep an observed role when its source scrolls out of the native
								// page. Retain only threads still represented by visible agents.
								for (thread, task) in &previous.assignments {
									if rows.iter().any(|row| row.thread.as_ref() == Some(thread)) {
										brief
											.assignments
											.entry(thread.clone())
											.or_insert_with(|| task.clone());
									}
								}
							}

							if brief.preview.is_none() {
								brief.preview = s
									.work_board
									.briefs
									.get(&key)
									.filter(|old| {
										old.source_thread == brief.source_thread
											&& old.stamp == brief.stamp
									})
									.and_then(|old| old.preview.clone());
							}

							s.work_board.reload_pending.remove(&key);
							s.work_board.briefs.insert(key, brief);
						}
					}

					s.work_board.brief_task = None;

					s.publish_usage();
					cx.notify();
				});
		}));
	}
}

fn latest_outcome(entries: &[AgentTimelineEntry]) -> Option<(String, String)> {
	entries.iter().rev().find_map(|e| match &e.content {
		AgentTimelineContent::TurnBoundary { turn_id, completed, status, .. } => Some((
			turn_id.clone(),
			if *completed { status.clone().unwrap_or_default() } else { "inProgress".into() },
		)),
		_ => None,
	})
}

fn apply_outcome(row: &mut Row, outcome: &str) {
	// Current execution, requests, uncertainty and dependencies outrank a previous result.
	if ![
		"Not running",
		"Idle",
		"Marked complete",
		"Follow-up pending",
		"Update pending",
		"Scheduled check",
	]
	.contains(&row.status.as_str())
	{
		return;
	}

	let (label, color) = match outcome {
		"failed" => ("Failed", ERROR),
		"interrupted" => ("Interrupted", TEXT_MUTED),
		_ => return,
	};

	row.status = label.into();
	row.color = color;
	row.group = 2;
	row.review = false;
}

fn native_state(status: &str) -> (&'static str, u8, u32) {
	match status {
		"active" | "running" => ("Running", 1, INFO),
		"waitingOnApproval" => ("Approval", 0, WARNING),
		"waitingOnUserInput" => ("Input needed", 0, WARNING),
		"systemError" => ("Error", 0, ERROR),
		"idle" => ("Idle", 2, TEXT_MUTED),
		"notLoaded" => ("Not running", 2, TEXT_MUTED),
		_ => ("Unknown", 0, TEXT_MUTED),
	}
}

#[cfg(test)]
#[path = "agent_work_board_tests.rs"]
mod tests;
