//! Camera targets use painted content; stable layout envelopes keep their own purpose.
use super::{AgentSurface, Context, FRAME_INSET, Graph, Node, card, groups, header};

impl AgentSurface {
	pub(super) fn relation_painted_rect(&self, node: &Node, graph: &Graph) -> card::Rect {
		let layout = graph.layout.on_display(self.work_board.view.display_scale);
		let mut rect = node.screen_rect(layout.zoom, self.work_board.view.pan);
		if node.row.is_some() && !layout.overview() {
			let (_, rows) = header::content_rows(
				&node.assignment,
				&node.activity,
				layout,
				&self.work_board.text_system,
			);
			rect.height = layout.header_height(rows) + 2. * layout.spacing(card::INSET_Y);
		}
		rect
	}

	fn relation_team_rect(&self, graph: &Graph, key: &str) -> Option<card::Rect> {
		let members = groups::descendants(graph, key);
		graph
			.nodes
			.iter()
			.filter(|node| members.contains_key(&node.key))
			.map(|node| self.relation_painted_rect(node, graph))
			.reduce(union)
	}

	pub(in super::super) fn locate_relation_agent(&mut self, key: &str, cx: &mut Context<Self>) {
		self.select_workbench_agent(key, cx);
		let graph = self.relation_graph();
		let Some(node) = graph.nodes.iter().find(|node| node.key == key) else { return };
		let viewport = self.work_board.view.viewport;
		let rect = self
			.relation_team_rect(&graph, key)
			.filter(|rect| fits(*rect, viewport))
			.unwrap_or_else(|| self.relation_painted_rect(node, &graph));
		if rect.x < FRAME_INSET
			|| rect.y < FRAME_INSET
			|| rect.x + rect.width > viewport.0 - FRAME_INSET
			|| rect.y + rect.height > viewport.1 - FRAME_INSET
		{
			let view = &mut self.work_board.view;
			view.pan.0 += viewport.0 / 2. - rect.x - rect.width / 2.;
			view.pan.1 += viewport.1 / 2. - rect.y - rect.height / 2.;
		}
		cx.notify();
	}
}

fn fits(rect: card::Rect, viewport: (f32, f32)) -> bool {
	rect.width <= viewport.0 - 2. * FRAME_INSET && rect.height <= viewport.1 - 2. * FRAME_INSET
}

fn union(a: card::Rect, b: card::Rect) -> card::Rect {
	let (x, y) = (a.x.min(b.x), a.y.min(b.y));
	card::Rect {
		x,
		y,
		width: (a.x + a.width).max(b.x + b.width) - x,
		height: (a.y + a.height).max(b.y + b.height) - y,
	}
}

#[cfg(test)]
mod tests {
	use super::{
		super::{Brief, layout},
		*,
	};

	fn team_fixture(cx: &mut gpui::Context<AgentSurface>) -> AgentSurface {
		let mut s = super::super::super::tests::fixture(cx);
		let snapshot = s.snapshot.as_mut().unwrap();
		snapshot
			.work_items
			.retain(|work| ["agent", "release", "flow", "verify"].contains(&work.id.as_str()));
		snapshot.archived_work_ids.insert("agent".into());
		snapshot.workspaces.clear();
		snapshot.dependencies.clear();
		snapshot.context_references.clear();
		for work in &mut snapshot.work_items {
			work.parent_goal_id = (work.id != "agent")
				.then(|| if work.id == "release" { "agent" } else { "release" }.into());
			work.title = format!("Check {} recovery", work.id);
		}
		s.native_agents.lists.clear();
		s.work_board.briefs.clear();
		for row in s.board_rows().into_iter().filter(|row| row.key != "agent") {
			let text = if row.key == "release" {
				"Compare the recovered answer with the original response and verify that the final paragraph and every code block are complete."
			} else {
				"The final paragraph is complete and the source reference still matches."
			};
			s.work_board.briefs.insert(
				row.key.clone(),
				Brief {
					source_thread: row.thread.clone(),
					output: Some(super::super::super::activity::Output {
						turn: "camera-check".into(),
						text: text.into(),
						truncated: false,
					}),
					outcome: Some(("camera-check".into(), "completed".into())),
					preview: None,
					question: None,
					assignments: Default::default(),
					metrics: Default::default(),
					resources: None,
					relations: vec![],
					stamp: s.brief_stamp(&row),
					read_at: std::time::Instant::now(),
				},
			);
		}
		s.workspace.graph_visible = true;
		s.workspace.graph_expanded = true;
		s.work_board.view.camera_fixed = true;
		s.work_board.view.presentation = layout::Presentation::Together;
		s.work_board.view.zoom = Some(0.75);
		s.work_board.view.pan = (-2000., -2000.);
		s.composer.update(cx, |input, cx| input.set_content("Keep the Main draft", cx));
		s
	}

	#[gpui::test]
	fn narrow_team_location_keeps_all_painted_metrics_and_messages_inside_the_canvas(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| team_fixture(cx));
		visual.simulate_resize(gpui::size(gpui::px(1040.), gpui::px(360.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		for density in [1., 2.] {
			let handle = visual.update(|window, _| window.window_handle());
			visual.simulate_window_scale_factor_change(handle, density);
			visual.simulate_resize(gpui::size(gpui::px(1040.), gpui::px(360.)));
			visual.update(crate::ui_frame::draw_frame);
			std::thread::sleep(
				crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40),
			);
			visual.update(crate::ui_frame::draw_frame);
			let height = surface.read_with(visual, |s, _| {
				let graph = s.relation_graph();
				let painted = s.relation_team_rect(&graph, "release").unwrap();
				// Reproduce the narrow interval where painted content fits but its
				// stable zoom envelope sends the old camera back to the owner alone.
				360. + painted.height + 2. * FRAME_INSET + 1. - s.work_board.view.viewport.1
			});
			visual.simulate_resize(gpui::size(gpui::px(1040.), gpui::px(height)));
			visual.update(crate::ui_frame::draw_frame);
			std::thread::sleep(
				crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40),
			);
			visual.update(crate::ui_frame::draw_frame);
			let main = surface.update(visual, |s, cx| {
				s.work_board.view.pan = (-2000., -2000.);
				let graph = s.relation_graph();
				let target = s.relation_team_rect(&graph, "release").unwrap();
				assert!(
					fits(target, s.work_board.view.viewport),
					"{target:?} in {:?}",
					s.work_board.view.viewport
				);
				let members = groups::descendants(&graph, "release");
				let reserved = graph
					.nodes
					.iter()
					.filter(|node| members.contains_key(&node.key))
					.map(|node| node.screen_rect(0.75, s.work_board.view.pan))
					.reduce(union)
					.unwrap();
				assert!(
					!fits(reserved, s.work_board.view.viewport),
					"the old reserved target must fail here: {reserved:?}, painted {target:?} in {:?}",
					s.work_board.view.viewport
				);
				let main = (
					s.selected.clone(),
					s.native_agents.selected.clone(),
					s.work_board.view.positions.clone(),
				);
				s.locate_relation_agent("release", cx);
				main
			});
			visual.update(crate::ui_frame::draw_frame);
			let canvas = visual.debug_bounds("relations-canvas").unwrap();
			for selector in [
				"relation-identity-release",
				"relation-avatar-release",
				"relation-metrics-release",
				"relation-preview-release",
				"relation-identity-flow",
				"relation-avatar-flow",
				"relation-metrics-flow",
				"relation-preview-flow",
				"relation-identity-verify",
				"relation-avatar-verify",
				"relation-metrics-verify",
				"relation-preview-verify",
			] {
				let bounds = visual.debug_bounds(selector).unwrap();
				assert!(
					bounds.left() >= canvas.left() && bounds.right() <= canvas.right(),
					"{selector}: {bounds:?} in {canvas:?}"
				);
				assert!(
					bounds.top() >= canvas.top() && bounds.bottom() <= canvas.bottom(),
					"{selector}: {bounds:?} in {canvas:?}"
				);
			}
			surface.read_with(visual, |s, cx| {
				assert_eq!(
					(
						s.selected.clone(),
						s.native_agents.selected.clone(),
						s.work_board.view.positions.clone()
					),
					main
				);
				assert_eq!(s.work_board.view.zoom, Some(0.75));
				assert_eq!(s.composer.read(cx).content(), "Keep the Main draft");
			});
		}
	}
}
