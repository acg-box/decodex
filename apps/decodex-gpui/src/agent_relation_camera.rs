//! Camera targets use painted content; stable layout envelopes keep their own purpose.
use super::{AgentSurface, Context, FRAME_INSET, Graph, Node, card, groups, header};

impl AgentSurface {
	pub(super) fn frame_relation_team(&mut self, key: &str, cx: &mut Context<Self>) {
		self.select_workbench_agent(key, cx);
		self.work_board.view.camera_fixed = true;
		self.work_board.view.card_transition = None;
		self.work_board.view.motion = None;
		let graph = self.relation_graph();
		if let Some(rect) = self.relation_team_rect(&graph, key) {
			if fits(rect, self.work_board.view.viewport) {
				self.center_relation_rect(rect);
			} else {
				self.frame_relation_scope(Some(key), self.relation_scale());
			}
		}
		cx.notify();
	}

	pub(super) fn frame_relations(&mut self) {
		self.work_board.view.card_transition = None;
		self.work_board.view.motion = None;
		// Global framing fits the current pane. A team frame keeps its placement.
		self.work_board.view.layout_viewport = self.work_board.view.viewport;
		self.frame_relation_scope(None, card::MAX_ZOOM);
	}

	fn frame_relation_scope(&mut self, team: Option<&str>, max_zoom: f32) {
		let (w, h) = self.work_board.view.viewport;
		if w <= 48. || h <= 48. {
			return;
		}
		let usable_height = (h - 2. * FRAME_INSET).max(1.);
		let now = std::time::Instant::now();
		for zoom in [1., 0.6, 0.3, card::MIN_ZOOM] {
			let layout = card::Layout::new(zoom.min(max_zoom));
			let graph = self.relation_graph_with_layout(now, layout.with_presence(1., false));
			let Some([min_x, min_y, max_x, max_y]) = frame_bounds(&graph, team) else { return };
			let width = (max_x - min_x).max(1.);
			let height = (max_y - min_y).max(1.);
			let fitted = ((w - 2. * FRAME_INSET) / width).min(usable_height / height);
			let (min, max) = layout.zoom_range();
			if fitted < min && !layout.points() {
				continue;
			}
			let scale = fitted.clamp(min, max.min(max_zoom));
			self.work_board.view.zoom = Some(scale);
			self.work_board.view.pan = (
				(w - width * scale) / 2. - min_x * scale,
				FRAME_INSET + (usable_height - height * scale) / 2. - min_y * scale,
			);
			break;
		}
	}

	fn center_relation_rect(&mut self, rect: card::Rect) {
		let view = &mut self.work_board.view;
		view.pan.0 += view.viewport.0 / 2. - rect.x - rect.width / 2.;
		view.pan.1 += view.viewport.1 / 2. - rect.y - rect.height / 2.;
	}

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
			self.center_relation_rect(rect);
		}
		cx.notify();
	}
}

fn frame_bounds(graph: &Graph, team: Option<&str>) -> Option<[f32; 4]> {
	if let Some(key) = team {
		let members = groups::descendants(graph, key);
		let rect = graph
			.nodes
			.iter()
			.filter(|node| members.contains_key(&node.key))
			.map(|node| node.screen_rect(1., (0., 0.)))
			.reduce(union)?;
		Some([rect.x, rect.y, rect.x + rect.width, rect.y + rect.height])
	} else {
		let keys = graph
			.nodes
			.iter()
			.filter(|node| node.row.is_some())
			.map(|node| node.key.clone())
			.collect();
		graph.layout_bounds(&keys)
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
	use gpui::AppContext as _;

	fn team_fixture(cx: &mut gpui::Context<AgentSurface>) -> AgentSurface {
		let mut s = super::super::super::tests::fixture(cx);
		let snapshot = s.snapshot.as_mut().unwrap();
		snapshot
			.work_items
			.retain(|work| ["agent", "release", "flow", "verify"].contains(&work.id.as_str()));
		snapshot.archived_work_ids.insert("agent".into());
		snapshot.projects.clear();
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

	fn assert_team_visible(visual: &mut gpui::VisualTestContext) {
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
			assert_team_visible(visual);
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

	#[gpui::test]
	fn team_count_frames_the_branch_with_pointer_and_keyboard_without_opening_its_chat(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| team_fixture(cx));
		visual.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40));
		visual.update(crate::ui_frame::draw_frame);
		let main = surface
			.read_with(visual, |s, _| (s.selected.clone(), s.native_agents.selected.clone()));
		for mode in layout::Mode::ALL {
			surface.update(visual, |s, cx| {
				s.switch_relation_layout(mode, cx);
				s.work_board.view.motion = None;
				let graph = s.relation_graph();
				let node = graph.nodes.iter().find(|node| node.key == "release").unwrap();
				s.work_board.view.pan =
					(50. - node.x * 0.75, s.work_board.view.viewport.1 - 70. - node.y * 0.75);
			});
			visual.update(crate::ui_frame::draw_frame);
			let team = visual.debug_bounds("relation-team-release").unwrap();
			visual.simulate_click(team.center(), Default::default());
			visual.update(crate::ui_frame::draw_frame);
			assert_team_visible(visual);
			let framed = surface.read_with(visual, |s, _| s.work_board.view.pan);
			let focus = visual.update(|window, cx| window.focused(cx).unwrap());
			for key in ["enter", "space"] {
				surface.update(visual, |s, _| {
					s.work_board.view.pan.0 += 25.;
					s.work_board.view.pan.1 += 45.;
				});
				visual.update(|window, cx| window.focus(&focus, cx));
				visual.simulate_keystrokes(key);
				visual.update(crate::ui_frame::draw_frame);
				assert_team_visible(visual);
				surface.read_with(visual, |s, _| {
					let pan = s.work_board.view.pan;
					assert!((pan.0 - framed.0).abs() < 0.01 && (pan.1 - framed.1).abs() < 0.01);
				});
			}
			surface.read_with(visual, |s, cx| {
				assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), main);
				assert_eq!(s.composer.read(cx).content(), "Keep the Main draft");
				assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("release"));
				assert_eq!(s.relation_scale(), 0.75);
				assert!(s.work_board.view.positions.is_empty());
				assert!(s.work_board.view.drag.is_none());
			});
		}
	}

	#[gpui::test]
	fn a_team_larger_than_the_pane_fits_at_a_lower_zoom_without_moving_its_members(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(team_fixture);
		surface.update(cx, |s, cx| {
			s.work_board.view.viewport = (560., 220.);
			s.work_board.view.layout_viewport = (1200., 700.);
			for mode in layout::Mode::ALL {
				s.work_board.view.layout_mode = mode;
				s.work_board.view.zoom = Some(0.75);
				s.frame_relation_team("release", cx);
				assert!(s.relation_scale() < 0.75);
				assert!(s.relation_scale() >= card::MIN_ZOOM);
				let graph = s.relation_graph();
				let target = s.relation_team_rect(&graph, "release").unwrap();
				assert!(fits(target, s.work_board.view.viewport), "{mode:?}: {target:?}");
				assert!(target.x >= FRAME_INSET - 0.01 && target.y >= FRAME_INSET - 0.01);
				assert!(target.x + target.width <= 560. - FRAME_INSET + 0.01);
				assert!(target.y + target.height <= 220. - FRAME_INSET + 0.01);
				assert!(s.work_board.view.positions.is_empty());
				assert_eq!(s.work_board.view.layout_viewport, (1200., 700.));
				assert_eq!(s.composer.read(cx).content(), "Keep the Main draft");
			}
		});
	}

	#[gpui::test]
	fn ownership_routes_do_not_cross_native_agent_identities(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| team_fixture(cx));
		visual.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(1000.)));
		visual.update(crate::ui_frame::draw_frame);
		for density in [1., 2.] {
			let handle = visual.update(|window, _| window.window_handle());
			visual.simulate_window_scale_factor_change(handle, density);
			for zoom in [1., 0.75, 0.5, 0.33] {
				surface.update(visual, |s, _| {
					s.work_board.view.zoom = Some(zoom);
					s.work_board.view.pan = (20., 20.);
					s.work_board.view.positions = [
						("release".into(), (450., 20.)),
						("flow".into(), (20., 340.)),
						("verify".into(), (880., 340.)),
					]
					.into();
				});
				visual.update(crate::ui_frame::draw_frame);
				let canvas = visual.debug_bounds("relations-canvas").unwrap();
				let identities = [
					"relation-identity-release",
					"relation-identity-flow",
					"relation-identity-verify",
				]
				.map(|selector| visual.debug_bounds(selector).unwrap());
				surface.read_with(visual, |s, _| {
					let graph = s.relation_graph();
					for path in graph.paths.values() {
						for sample in 0..=128 {
							let (x, y) = path.at(sample as f32 / 128.);
							let point = canvas.origin
								+ gpui::point(gpui::px(x * zoom + 20.), gpui::px(y * zoom + 20.));
							assert!(
								!identities.iter().any(|identity| identity.contains(&point)),
								"a wire crosses an identity at {zoom}x, {density}x: {point:?}, {identities:?}"
							);
						}
					}
				});
			}
		}
	}
}
