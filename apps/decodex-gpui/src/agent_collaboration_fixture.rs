//! A disposable native view of the approved graph, without any service connection.
use super::{
	super::{Brief, activity},
	AgentSurface, Context,
};
use decodex_protocol::{
	AgentCollaborationDto, AgentDispatchStateDto, AgentWorkKindDto, AgentWorkStatusDto,
	NativeAgentDto,
};
use gpui::prelude::*;

impl AgentSurface {
	#[allow(dead_code)] // Only the isolated capture driver advances these events.
	pub(crate) fn visual_collaboration_actions(&mut self, step: usize, cx: &mut Context<Self>) {
		if let Some(preview) = self
			.work_board
			.briefs
			.get_mut("native:mira:ref-lin")
			.and_then(|brief| brief.preview.as_mut())
		{
			let actions = [
				"Read the context fixture",
				"Prepare 256k input",
				"Run the native probe",
				"Collect the complete reply",
			];
			preview.recent = actions
				.iter()
				.enumerate()
				.take(step)
				.skip(step.saturating_sub(3))
				.map(|(index, text)| activity::Operation {
					id: format!("motion:{index}"),
					text: (*text).into(),
				})
				.collect();
		}
		cx.notify();
	}

	#[allow(dead_code)] // Shared source is also compiled into the application binary.
	pub(crate) fn render_collaboration_component(
		&self,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		// The offscreen fixture supplies a deterministic compositor backdrop.
		// Production keeps its existing native window and sidebar materials.
		gpui::div()
			.size_full()
			.flex()
			.font_family(crate::ui_theme::FONT_FAMILY)
			.font_weight(gpui::FontWeight::NORMAL)
			.text_color(crate::ui_theme::rgb(crate::ui_theme::TEXT))
			.bg(crate::ui_theme::rgb(0x303030))
			.child(gpui::div().flex_1().min_w_0().h_full().child(self.render_work_board(cx)))
			.child(
				gpui::div()
					.w(gpui::px(394.))
					.flex_none()
					.h_full()
					.bg(crate::ui_theme::rgb(0x373737))
					.border_l_1()
					.border_color(crate::ui_theme::rgba(0xffffff16))
					.child(self.render_agent_workbench(cx)),
			)
			.into_any_element()
	}

	pub(super) fn visual_collaboration_fixture(&mut self, cx: &mut Context<Self>) {
		let Some(snapshot) = &mut self.snapshot else { return };
		let template = snapshot.work_items[0].clone();
		snapshot.work_items = [
			("agent", None, "Main"),
			("mira", Some("agent"), "Mira"),
			("sage", Some("agent"), "Sage"),
			("ravi", Some("agent"), "Ravi"),
		]
		.into_iter()
		.map(|(id, parent, title)| {
			let mut work = template.clone();
			work.id = id.into();
			work.parent_goal_id = parent.map(str::to_owned);
			work.title = title.into();
			work.kind = AgentWorkKindDto::Manager;
			work.codex_thread_id = Some(format!("ref-{id}"));
			work.active_turn_id = Some("reference".into());
			work.dispatch_state = AgentDispatchStateDto::Running;
			work.status = AgentWorkStatusDto::Open;
			work
		})
		.collect();
		snapshot.pending_events.clear();
		snapshot.dependencies.clear();
		snapshot.context_references.clear();
		snapshot.workspaces.clear();
		snapshot.archived_work_ids.clear();
		self.native_agents.lists.clear();
		for (owner, id, parent, name, task, state) in [
			("mira", "nora", "mira", "Nora", "Profile recovery", "waitingOnUserInput"),
			("mira", "lin", "mira", "Lin", "Long-context probe", "running"),
			("mira", "sol", "nora", "Sol", "Recovery research", "running"),
			("sage", "iris", "sage", "Iris", "Node layout", "running"),
			("sage", "theo", "sage", "Theo", "Inspector", "idle"),
			("ravi", "tess", "ravi", "Tess", "UI checks", "idle"),
			("ravi", "eden", "ravi", "Eden", "Runtime", "idle"),
		] {
			self.native_agents.lists.entry(owner.into()).or_default().push(NativeAgentDto {
				thread_id: format!("ref-{id}"),
				parent_thread_id: format!("ref-{parent}"),
				title: name.into(),
				task: task.into(),
				status: state.into(),
			});
		}
		self.work_board.briefs.clear();
		let rows = self.factory_rows();
		for (index, row) in rows.into_iter().enumerate() {
			let id =
				row.thread.as_deref().unwrap_or_default().strip_prefix("ref-").unwrap_or_default();
			let (task, action) = reference_text(id);
			self.work_board.briefs.insert(
				row.key.clone(),
				Brief {
					source_thread: row.thread.clone(),
					output: None,
					assignments: Default::default(),
					outcome: None,
					preview: Some(activity::Preview {
						turn: "reference".into(),
						task: task.into(),
						text: action.into(),
						running: None,
						recent: vec![activity::Operation {
							id: format!("reference:{id}"),
							text: action.into(),
						}],
					}),
					question: None,
					resources: None,
					relations: vec![],
					metrics: crate::ui_work_metrics::Sample {
						tokens: Some(37_000 + index as u64 * 7_000),
						stats: Some(decodex_protocol::AgentThreadMetrics {
							completed_ms: Some(480_000 + index as u64 * 17_000),
							active_ms: None,
							generating: false,
							rate: Some(decodex_protocol::AgentOutputRate {
								estimated: true,
								active: false,
								output_tokens: 520,
								duration_ms: 10_000,
								started_at_ms: 0,
								completed_at_ms: Some(10_000),
							}),
						}),
						read_at: None,
					},
					stamp: self.brief_stamp(&row),
					read_at: std::time::Instant::now(),
				},
			);
		}
		self.visual_reference_usage();
		self.reference_collaboration();
		self.selected = Some("agent".into());
		self.native_agents.selected = None;
		self.workspace.sidebar_visible = false;
		self.workspace.agent_panel_width = 320.;
		self.workspace.graph_visible = true;
		self.workspace.graph_expanded = true;
		self.work_board.view.picked_agent = Some("native:mira:ref-nora".into());
		self.work_board.collaboration_expanded =
			Some("call:ref-mira:request-mira-nora:ref-nora".into());
		self.publish_usage();
		cx.notify();
	}

	fn visual_reference_usage(&mut self) {
		let rows = self.factory_rows();
		self.snapshot.as_mut().expect("visual snapshot").agent_slots =
			rows.iter().enumerate().map(|(index, row)| (row.key.clone(), index as u32)).collect();
		self.work_board.usage = Some(
			rows.into_iter()
				.map(|row| {
					let sample = self.work_board.briefs[&row.key].metrics;
					(row.key, (row.thread, sample))
				})
				.collect(),
		);
	}

	/// Replace only the disposable capture data with a larger ownership tree.
	#[allow(dead_code)] // Used by the isolated capture driver and density test.
	pub(crate) fn visual_relation_density(&mut self, count: usize, cx: &mut Context<Self>) {
		let snapshot = self.snapshot.as_mut().expect("visual snapshot");
		let template = snapshot.work_items[0].clone();
		let key = |index| if index == 0 { "agent".into() } else { format!("density-{index:03}") };
		snapshot.work_items = (0..count)
			.map(|index| {
				let mut work = template.clone();
				work.id = key(index);
				work.title = format!("Research assignment {index}");
				work.parent_goal_id = (index > 0).then(|| key((index - 1) / 5));
				work.codex_thread_id = Some(format!("density-thread-{index}"));
				work.active_turn_id = Some("fixture-operation".into());
				work.dispatch_state = if index % 7 == 0 {
					AgentDispatchStateDto::Idle
				} else {
					AgentDispatchStateDto::Running
				};
				work
			})
			.collect();
		snapshot.agent_slots.clear();
		snapshot.pending_events.clear();
		snapshot.dependencies.clear();
		snapshot.context_references.clear();
		snapshot.workspaces.clear();
		snapshot.archived_work_ids.clear();
		self.native_agents.lists.clear();
		self.native_agents.selected = None;
		self.work_board.briefs.clear();
		self.work_board.view = Default::default();
		self.work_board.collaboration_expanded = None;
		self.visual_workbench_metrics();
		self.publish_usage();
		cx.notify();
	}

	fn reference_collaboration(&mut self) {
		for (owner, sender, receiver, prompt, tool, status) in [
			(
				"agent",
				"agent",
				"mira",
				"Restore Pro continuity and report the remaining blockers.",
				"sendInput",
				"completed",
			),
			(
				"mira",
				"mira",
				"nora",
				"Check reconnect preserves the full reply.",
				"sendInput",
				"completed",
			),
			(
				"mira",
				"nora",
				"sol",
				"Can native readback recover the missing ending?",
				"sendInput",
				"inProgress",
			),
			(
				"mira",
				"mira",
				"lin",
				"Run the long-context probe and keep its evidence.",
				"sendInput",
				"inProgress",
			),
			("sage", "sage", "iris", "Check spacing with 24 agents.", "sendInput", "inProgress"),
			(
				"sage",
				"sage",
				"theo",
				"Connect graph selection to the existing chat.",
				"sendInput",
				"completed",
			),
			(
				"ravi",
				"ravi",
				"tess",
				"Verify the existing composer and background materials.",
				"sendInput",
				"inProgress",
			),
		] {
			let key = if owner == sender {
				owner.to_owned()
			} else {
				format!("native:{owner}:ref-{sender}")
			};
			if let Some(brief) = self.work_board.briefs.get_mut(&key) {
				brief.relations.push((
					format!("request-{sender}-{receiver}"),
					AgentCollaborationDto {
						sender_thread_id: format!("ref-{sender}"),
						receiver_thread_ids: vec![format!("ref-{receiver}")],
						tool: tool.into(),
						status: status.into(),
						prompt: prompt.into(),
						results: vec![],
					},
				));
			}
		}
	}
}

fn reference_text(id: &str) -> (&'static str, &'static str) {
	match id {
		"agent" => ("Follow up on active projects", "Checking the blocked handoff"),
		"mira" => ("Pro continuity", "Reviewing recovery proposal"),
		"nora" => ("Profile recovery", "Waiting for your decision"),
		"lin" => ("Long-context probe", "Running 256k input"),
		"sol" => ("Recovery research", "Comparing native result paths"),
		"sage" => ("Graph workspace", "Reviewing compact nodes"),
		"iris" => ("Node layout", "Checking 24-agent spacing"),
		"theo" => ("Inspector", "Connecting selected artifact"),
		"ravi" => ("Local release", "Collecting acceptance"),
		"tess" => ("UI checks", "Checking material continuity"),
		_ => ("Runtime", "Connection healthy"),
	}
}
