//! Project captions share a stable bottom rail with their actors, not transient speech.
use gpui::{AnyElement, ClickEvent, PathBuilder, TextRun, WindowTextSystem};

use crate::{
	shell::agent_surface::work_board::relations::{
		AgentSurface, BTreeSet, Context, Div, GRID, Graph, InteractiveElement as _,
		IntoElement as _, ParentElement as _, Role, SharedString, SmoothControl as _, Stateful,
		StatefulInteractiveElement as _, Styled as _, card,
	},
	ui_motion, ui_theme,
};
use decodex_protocol::ProjectDto;

pub(super) const CAPTION_HEIGHT: f32 = 24.;
const TITLE_SIZE: f32 = 12.;
const LABEL_PADDING: f32 = 6.;
const TITLE_CLEARANCE: f32 = 4.;

pub(super) struct Scope {
	pub id: String,
	pub title: String,
	pub members: BTreeSet<String>,
	pub bounds: [f32; 4],
}

impl Scope {
	pub(super) fn label(&self) -> String {
		let count = self.members.len();

		format!("Project: {} ({count} {})", self.title, if count == 1 { "agent" } else { "agents" })
	}

	pub(super) fn caption_width(&self, text: &WindowTextSystem) -> f32 {
		2. * LABEL_PADDING
			+ f32::from(
				text.shape_line(
					self.title.clone().into(),
					gpui::px(TITLE_SIZE),
					&[TextRun {
						len: self.title.len(),
						font: gpui::font(ui_theme::FONT_FAMILY),
						..Default::default()
					}],
					None,
				)
				.width,
			)
			.ceil()
	}

	/// The caption has one anchor. Cropping and selection never move it along the rail.
	pub(super) fn caption_rect(
		&self,
		scale: f32,
		pan: (f32, f32),
		width: f32,
	) -> Option<card::Rect> {
		let [left, _, right, bottom] = self.bounds;

		((right - left) * scale >= width + 2. * LABEL_PADDING).then_some(card::Rect {
			x: left * scale + pan.0 + LABEL_PADDING,
			y: bottom * scale + pan.1 - CAPTION_HEIGHT / 2.,
			width,
			height: CAPTION_HEIGHT,
		})
	}

	/// Routing uses the same caption in world coordinates, independent of viewport cropping.
	pub(super) fn caption_bounds(
		&self,
		layout: card::Layout,
		text: &WindowTextSystem,
	) -> Option<[f32; 4]> {
		if layout.points() {
			return None;
		}
		let scale = layout.zoom;
		let rect = self.caption_rect(scale, (0., 0.), self.caption_width(text))?;

		Some([
			rect.x / scale,
			rect.y / scale,
			(rect.x + rect.width) / scale,
			(rect.y + rect.height) / scale,
		])
	}

	pub(super) fn title_gap(&self, rect: card::Rect, scale: f32, pan: (f32, f32)) -> (f32, f32) {
		let left = self.bounds[0] * scale + pan.0;

		(
			rect.x + LABEL_PADDING - TITLE_CLEARANCE - left,
			rect.x + rect.width - LABEL_PADDING + TITLE_CLEARANCE - left,
		)
	}

	/// A short bottom rail stays quiet; selection adds only the two upper corners.
	pub(super) fn outline(
		&self,
		scale: f32,
		selected: bool,
		title_gap: Option<(f32, f32)>,
		presence: f32,
	) -> AnyElement {
		let id = format!("project-scope-{}", self.id);

		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, cx| {
				if presence <= 0.001 {
					return;
				}
				let active = ui_motion::value(id.clone(), u8::from(selected) as f32, window, cx);
				let density = window.scale_factor();
				let snap = |value: f32| (value * density).round() / density;
				let origin = gpui::point(
					gpui::px(snap(f32::from(bounds.left()))),
					gpui::px(snap(f32::from(bounds.top()))),
				);
				let stroke = snap(1.25);
				let left = stroke / 2.;
				let top = stroke / 2.;
				let right = snap(f32::from(bounds.right())) - f32::from(origin.x) - stroke / 2.;
				let bottom = snap(f32::from(bounds.bottom())) - f32::from(origin.y) - stroke / 2.;
				let cap = snap((12. * scale).clamp(6., 12.))
					.min((bottom - top) / 2.)
					.min((right - left) / 2.);
				let radius = snap(2.).min(cap).min((right - left) / 2.);
				let point = |x, y| origin + gpui::point(gpui::px(x), gpui::px(y));
				let radii = gpui::point(gpui::px(radius), gpui::px(radius));
				let mut rail = PathBuilder::stroke(gpui::px(stroke));

				rail.move_to(point(left, bottom - cap));
				rail.line_to(point(left, bottom - radius));
				rail.arc_to(radii, gpui::px(0.), false, false, point(left + radius, bottom));
				if let Some((start, end)) = title_gap {
					rail.line_to(point(snap(start), bottom));
					rail.move_to(point(snap(end), bottom));
				}
				rail.line_to(point(right - radius, bottom));
				rail.arc_to(radii, gpui::px(0.), false, false, point(right, bottom - radius));
				rail.line_to(point(right, bottom - cap));

				if let Ok(path) = rail.build() {
					window.paint_path(
						path.clone(),
						ui_theme::rgb(ui_theme::TEXT_MUTED).opacity(0.44 * presence),
					);
					if active > 0.001 {
						window.paint_path(
							path,
							ui_theme::rgb(ui_theme::BLUE).opacity(active * 0.45 * presence),
						);
					}
				}

				if active > 0.001 {
					let mut corners = PathBuilder::stroke(gpui::px(stroke));

					corners.move_to(point(left, top + cap));
					corners.line_to(point(left, top + radius));
					corners.arc_to(radii, gpui::px(0.), false, true, point(left + radius, top));
					corners.line_to(point(left + cap, top));
					corners.move_to(point(right - cap, top));
					corners.line_to(point(right - radius, top));
					corners.arc_to(radii, gpui::px(0.), false, true, point(right, top + radius));
					corners.line_to(point(right, top + cap));
					if let Ok(path) = corners.build() {
						window.paint_path(
							path,
							ui_theme::rgb(ui_theme::BLUE).opacity(active * 0.5 * presence),
						);
					}
				}
			},
		)
		.absolute()
		.inset_0()
		.size_full()
		.into_any_element()
	}
}

impl AgentSurface {
	pub(super) fn relation_scope_selected(&self, scope: &Scope) -> bool {
		self.work_board.view.picked_project.as_ref() == Some(&scope.id)
			|| self
				.work_board
				.view
				.picked_agent
				.as_ref()
				.is_some_and(|key| scope.members.contains(key))
	}

	pub(super) fn relation_scope_labels<'a>(
		&self,
		graph: &'a Graph,
	) -> Vec<(&'a Scope, card::Rect)> {
		if graph.layout.points() {
			return Vec::new();
		}
		let text = WindowTextSystem::new(self.work_board.text_system.clone());
		let (width, height) = self.work_board.view.viewport;

		graph
			.scopes
			.iter()
			.filter_map(|scope| {
				let rect = scope.caption_rect(
					self.relation_scale(),
					self.work_board.view.pan,
					scope.caption_width(&text),
				)?;

				(rect.x >= 4.
					&& rect.y >= 4.
					&& rect.x + rect.width <= width - 4.
					&& rect.y + rect.height <= height - 4.)
					.then_some((scope, rect))
			})
			.collect()
	}

	pub(super) fn relation_scope_captions(
		&self,
		mut canvas: Stateful<Div>,
		graph: &Graph,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		for (scope, rect) in self.relation_scope_labels(graph) {
			let heading = scope.title.clone();
			let project = scope.id.clone();
			let selected = self.relation_scope_selected(scope);
			let id = format!("project-title-{}", scope.id);
			let action: card::PointerAction<ClickEvent> =
				std::rc::Rc::new(cx.listener(move |s, _, _, cx| {
					s.frame_relation_project(project.clone(), cx);
					cx.stop_propagation();
				}));
			let button = gpui::div()
				.id(SharedString::from(id.clone()))
				.debug_selector(move || id.clone())
				.role(Role::Button)
				.tab_index(0)
				.aria_label(format!("Show all members of {}", scope.label()))
				.w(gpui::px(rect.width))
				.h(gpui::px(rect.height))
				.px(gpui::px(LABEL_PADDING))
				.flex()
				.items_center()
				.rounded(gpui::px(5.))
				.whitespace_nowrap()
				.font_family(ui_theme::FONT_FAMILY)
				.text_size(gpui::px(TITLE_SIZE))
				.text_color(ui_theme::rgb(if selected {
					ui_theme::BLUE
				} else {
					ui_theme::foreground::BODY
				}))
				.cursor_pointer()
				.occlude()
				.hover(|style| style.text_color(ui_theme::rgb(ui_theme::BLUE)))
				.focus_visible(|style| style.bg(ui_theme::rgb(ui_theme::BLUE).opacity(0.10)))
				.child(heading);

			canvas = canvas.child(
				gpui::div()
					.absolute()
					.left(gpui::px(rect.x))
					.top(gpui::px(rect.y))
					.w(gpui::px(rect.width))
					.h(gpui::px(rect.height))
					.opacity(graph.layout.card_presence().powi(2))
					.child(
						card::Interaction::keyboard_action(button, &action)
							.on_click(move |event, window, cx| action(event, window, cx))
							.smooth(),
					),
			);
		}

		canvas
	}
}

impl Graph {
	pub(super) fn scope_bounds(&mut self, projects: &[ProjectDto]) {
		for node in &self.nodes {
			let Some(row) = &node.row else { continue };
			let Some(project) = &row.workspace else { continue };
			let [left, top, right, bottom] = node.actor_bounds();
			let bounds = [
				left - GRID,
				top - GRID,
				right + GRID,
				bottom + GRID + CAPTION_HEIGHT / (2. * self.layout.zoom),
			];

			if let Some(scope) = self.scopes.iter_mut().find(|g| &g.id == project) {
				scope.bounds[0] = scope.bounds[0].min(bounds[0]);
				scope.bounds[1] = scope.bounds[1].min(bounds[1]);
				scope.bounds[2] = scope.bounds[2].max(bounds[2]);
				scope.bounds[3] = scope.bounds[3].max(bounds[3]);
				scope.members.insert(node.key.clone());
			} else {
				let title = projects
					.iter()
					.find(|w| &w.id == project)
					.map(|w| w.name.clone())
					.unwrap_or_else(|| "Project".into());

				self.scopes.push(Scope {
					id: project.clone(),
					title,
					members: BTreeSet::from([node.key.clone()]),
					bounds,
				});
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use gpui::AppContext as _;

	use super::*;
	use crate::shell::agent_surface::work_board::{relations::layout, tests};

	#[test]
	fn caption_anchor_uses_the_bottom_rail_and_follows_pan_without_sliding() {
		let scope = Scope {
			id: "project".into(),
			title: "Project".into(),
			members: BTreeSet::new(),
			bounds: [0., 0., 600., 300.],
		};
		let rect = scope.caption_rect(1., (40., 80.), 100.).unwrap();

		assert_eq!((rect.x, rect.y + CAPTION_HEIGHT / 2.), (46., 380.));
		let panned = scope.caption_rect(1., (-40., 50.), 100.).unwrap();
		assert_eq!((panned.x - rect.x, panned.y - rect.y), (-80., -30.));
		assert_eq!(panned.width, rect.width);

		let narrow = scope.caption_rect(0.1, (40., 80.), 42.).unwrap();
		assert_eq!(narrow.x, 46.);
		let (start, end) = scope.title_gap(narrow, 0.1, (40., 80.));
		assert_eq!(end - start, 38., "only text and its clearance interrupt the rail");
		assert!(scope.caption_rect(0.1, (40., 80.), 48.).is_some());
		assert!(scope.caption_rect(0.1, (40., 80.), 49.).is_none());
	}

	#[gpui::test]
	fn caption_gap_uses_shaped_text_and_keeps_a_readable_hit_height(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(fixture);
		surface.read_with(cx, |s, _| {
			let text = WindowTextSystem::new(s.work_board.text_system.clone());
			for title in ["work", "Design tools", "项目工作区"] {
				let scope = Scope {
					id: "project".into(),
					title: title.into(),
					members: BTreeSet::from(["one".into(), "two".into()]),
					bounds: [0., 0., 2_000., 300.],
				};
				let width = scope.caption_width(&text);
				let rect = scope.caption_rect(0.23, (40., 80.), width).unwrap();
				let (start, end) = scope.title_gap(rect, 0.23, (40., 80.));
				assert_eq!(start + 40., rect.x + 2.);
				assert_eq!(end + 40., rect.x + rect.width - 2.);
				assert_eq!(rect.height, CAPTION_HEIGHT);
			}
		});
	}

	fn fixture(cx: &mut Context<AgentSurface>) -> AgentSurface {
		let mut s = tests::fixture(cx);

		s.snapshot.as_mut().unwrap().projects = vec![ProjectDto {
			id: "project".into(),
			name: "Design".into(),
			native_project_id: None,
			lead_agent_id: None,
			directory: "/tmp/design".into(),
			agent_ids: vec!["release".into(), "flow".into()],
		}];
		s.workspace.graph_visible = true;
		s.workspace.graph_expanded = true;
		s.work_board.view.camera_fixed = true;
		s.work_board.view.viewport = (1_200., 700.);
		s.work_board.view.layout_viewport = (1_200., 700.);
		s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));

		s
	}

	#[gpui::test]
	fn project_rail_ignores_empty_layout_space_and_growing_or_disappearing_speech(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(fixture);

		surface.update(cx, |s, _| {
			let mut graph = s.relation_graph();
			let mut node = graph
				.nodes
				.iter()
				.find(|node| node.row.as_ref().is_some_and(|row| row.workspace.is_some()))
				.unwrap()
				.clone();
			node.x = 0.;
			node.y = 0.;
			node.size = (600., 600.);
			node.attachment = Some([250., 10., 330., 90.]);
			node.identity = Some([265., 93., 295., 106.]);
			node.speech = None;
			graph.nodes = vec![node];
			graph.scopes.clear();
			graph.scope_bounds(&[]);

			let stable = [234., -6., 346., 122. + CAPTION_HEIGHT / (2. * graph.layout.zoom)];
			assert_eq!(graph.scopes[0].bounds, stable);
			assert_eq!(graph.scopes[0].members.len(), 1);

			graph.nodes[0].speech = Some([-30., -120., 550., 5.]);
			graph.scopes.clear();
			graph.scope_bounds(&[]);

			assert_eq!(graph.scopes[0].bounds, stable);
			graph.nodes[0].speech = None;
			graph.scopes.clear();
			graph.scope_bounds(&[]);
			assert_eq!(graph.scopes[0].bounds, stable);
			assert_eq!((graph.nodes[0].x, graph.nodes[0].y), (0., 0.));
			assert_eq!(graph.nodes[0].size, (600., 600.));
		});
	}

	#[gpui::test]
	fn point_overview_keeps_topology_without_captions_or_hidden_label_obstacles(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(fixture);
		surface.update(cx, |s, _| {
			let text = WindowTextSystem::new(s.work_board.text_system.clone());
			for mode in layout::Mode::ALL {
				for presentation in [layout::Presentation::Links, layout::Presentation::Together] {
					s.work_board.view.layout_mode = mode;
					s.work_board.view.presentation = presentation;
					for zoom in [0.05, 0.15, 0.25, 0.30] {
						s.work_board.view.zoom = Some(zoom);
						let graph = s.relation_graph();
						assert!(graph.layout.points());
						assert!(s.relation_scope_labels(&graph).is_empty());
						assert!(
							graph
								.scopes
								.iter()
								.all(|scope| scope.caption_bounds(graph.layout, &text).is_none())
						);
						assert_eq!(s.relation_wire_presence(std::time::Instant::now()), 1.);
					}
				}
			}
		});
	}

	#[gpui::test]
	fn project_framing_keeps_chats_and_positions_in_all_layouts(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(fixture);

		surface.update(cx, |s, cx| {
			let current = (s.selected.clone(), s.native_agents.selected.clone());
			let selection = String::from("project");

			for mode in layout::Mode::ALL {
				for presentation in [layout::Presentation::Links, layout::Presentation::Together] {
					s.work_board.view.layout_mode = mode;
					s.work_board.view.presentation = presentation;
					s.work_board.view.zoom = Some(0.1);
					s.work_board.view.pan = (-2_000., -2_000.);
					s.frame_relation_project(selection.clone(), cx);

					let graph = s.relation_graph();
					let scope = graph.scopes.iter().find(|scope| scope.id == selection).unwrap();
					let zoom = s.relation_scale();
					let (x, y) = s.work_board.view.pan;
					let [left, top, right, bottom] = scope.bounds;

					assert!(left * zoom + x >= 15.9);
					assert!(top * zoom + y >= 15.9);
					assert!(right * zoom + x <= 1_184.1);
					assert!(bottom * zoom + y + CAPTION_HEIGHT / 2. <= 684.1);
					assert_eq!(s.work_board.view.picked_project.as_ref(), Some(&selection));
					assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), current);
					assert_eq!(s.composer.read(cx).content(), "Keep this draft");
					assert!(s.work_board.view.positions.is_empty());
					assert!(s.work_board.view.drag.is_none());

					let neighborhood = s.relation_neighborhood(&graph);
					for node in graph.nodes.iter().filter(|node| node.row.is_some()) {
						let (selected, faded) =
							s.relation_node_emphasis(node, &graph, &neighborhood);

						assert_eq!(
							(selected, faded),
							if scope.members.contains(&node.key) {
								(true, false)
							} else {
								(false, true)
							}
						);
					}
				}
			}

			s.select_workbench_agent("release", cx);
			assert!(s.work_board.view.picked_project.is_none());
		});
	}

	#[gpui::test]
	fn project_border_keeps_padding_around_the_hud_at_readable_zoom(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_600.), gpui::px(900.)));
		for zoom in [0.46, 0.5, 0.75, 1.] {
			surface.update(visual, |s, cx| {
				s.snapshot.as_mut().unwrap().projects[0].agent_ids = vec!["release".into()];
				s.work_board.view.zoom = Some(zoom);
				let graph = s.relation_graph();
				let scope = graph.scopes.iter().find(|scope| scope.id == "project").unwrap();

				s.work_board.view.pan =
					(80. - scope.bounds[0] * zoom, 80. - scope.bounds[1] * zoom);
				cx.notify();
			});
			visual.update(crate::ui_frame::draw_frame);
			let canvas = visual.debug_bounds("relations-canvas").unwrap();
			let hud = visual.debug_bounds("relation-hud-release").unwrap();

			surface.read_with(visual, |s, _| {
				let graph = s.work_board.view.layers.graph.borrow();
				let scope = graph
					.as_ref()
					.unwrap()
					.scopes
					.iter()
					.find(|scope| scope.id == "project")
					.unwrap();
				let left =
					f32::from(canvas.left()) + scope.bounds[0] * zoom + s.work_board.view.pan.0;
				let right =
					f32::from(canvas.left()) + scope.bounds[2] * zoom + s.work_board.view.pan.0;

				assert!(
					f32::from(hud.left()) - left >= GRID * zoom - 1.,
					"left HUD padding at zoom {zoom}"
				);
				assert!(
					right - f32::from(hud.right()) >= GRID * zoom - 1.,
					"right HUD padding at zoom {zoom}"
				);
			});
		}
	}

	#[gpui::test]
	fn project_legend_stays_on_its_border_without_unassigned_headers(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_600.), gpui::px(900.)));
		surface.update(visual, |s, cx| {
			s.work_board.view.zoom = Some(0.75);
			let graph = s.relation_graph();
			let scope = graph.scopes.iter().find(|scope| scope.id == "project").unwrap();

			s.work_board.view.pan = (80. - scope.bounds[0] * 0.75, 80. - scope.bounds[1] * 0.75);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		let canvas = visual.debug_bounds("relations-canvas").unwrap();
		let title = visual.debug_bounds("project-title-project").unwrap();

		surface.read_with(visual, |s, _| {
			let graph = s.work_board.view.layers.graph.borrow();
			let scope =
				graph.as_ref().unwrap().scopes.iter().find(|scope| scope.id == "project").unwrap();
			let rail_y = f32::from(canvas.origin.y)
				+ scope.bounds[3] * s.relation_scale()
				+ s.work_board.view.pan.1;

			assert!(
				(f32::from(title.center().y) - rail_y).abs() < 1.,
				"the caption stays on the bottom rail"
			);
		});
		surface.update(visual, |s, _| {
			let graph = s.relation_graph();
			let (_, before) = s
				.relation_scope_labels(&graph)
				.into_iter()
				.find(|(scope, _)| scope.id == "project")
				.unwrap();
			s.work_board.view.pan.0 += 32.;
			let (_, moved) = s
				.relation_scope_labels(&graph)
				.into_iter()
				.find(|(scope, _)| scope.id == "project")
				.unwrap();
			assert_eq!((moved.x - before.x, moved.y), (32., before.y));
			s.work_board.view.viewport.0 = moved.x + moved.width - 2.;
			assert!(
				s.relation_scope_labels(&graph).iter().all(|(scope, _)| scope.id != "project"),
				"a cropped caption hides at its anchor rather than sliding along the rail"
			);
		});
	}

	#[gpui::test]
	fn overview_project_titles_are_clickable_without_dragging_or_opening_a_chat(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_600.), gpui::px(900.)));
		surface.update(visual, |s, cx| {
			s.fit_relations();
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40));
		visual.update(crate::ui_frame::draw_frame);
		surface.update(visual, |s, cx| {
			s.fit_relations();
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		let current = surface
			.read_with(visual, |s, _| (s.selected.clone(), s.native_agents.selected.clone()));
		let title = visual.debug_bounds("project-title-project").unwrap();
		assert!(visual.debug_bounds("project-title-unassigned").is_none());
		assert!(visual.debug_bounds("project-count-project").is_none());
		assert!(visual.debug_bounds("relation-team-release").is_none());

		visual.simulate_click(title.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("project-count-project").is_none());
		assert_eq!(
			visual.debug_bounds("project-title-project").unwrap().size.width,
			title.size.width
		);
		assert!(visual.debug_bounds("relation-team-release").is_none());

		surface.read_with(visual, |s, cx| {
			assert_eq!(s.work_board.view.picked_project, Some(String::from("project")));
			assert!(s.work_board.view.picked_agent.is_none());
			assert!(s.work_board.view.drag.is_none());
			assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), current);
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		});

		// The title hides when there is no longer room in its own border.
		surface.update(visual, |s, cx| {
			s.work_board.view.zoom = Some(0.1);
			s.work_board.view.card_transition = None;
			let graph = s.relation_graph();
			let scope = graph.scopes.iter().find(|scope| scope.id == "project").unwrap();

			s.work_board.view.pan = (60. - scope.bounds[0] * 0.1, 160. - scope.bounds[3] * 0.1);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("project-title-project").is_none());

		assert!(visual.debug_bounds("project-title-unassigned").is_none());
		assert!(visual.debug_bounds("project-unassigned").is_none());

		surface.update(visual, |s, cx| {
			s.work_board.view.zoom = Some(0.75);
			let graph = s.relation_graph();
			let scope = graph.scopes.iter().find(|scope| scope.id == "project").unwrap();
			s.work_board.view.pan = (60. - scope.bounds[0] * 0.75, 240. - scope.bounds[3] * 0.75);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		let title = visual.debug_bounds("project-title-project").unwrap();
		visual.simulate_click(title.center(), Default::default());
		let focus = visual.update(|window, cx| window.focused(cx).unwrap());
		for key in ["enter", "space"] {
			surface.update(visual, |s, cx| {
				s.clear_relation_selection();
				cx.notify();
			});
			visual.update(|window, cx| window.focus(&focus, cx));
			visual.simulate_keystrokes(key);
			visual.update(crate::ui_frame::draw_frame);
			surface.read_with(visual, |s, _| {
				assert_eq!(s.work_board.view.picked_project.as_deref(), Some("project"));
			});
		}

		surface.update(visual, |s, cx| s.select_workbench_agent("release", cx));
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("project-count-project").is_none());
		assert!(visual.debug_bounds("relation-team-release").is_some());
	}
}
