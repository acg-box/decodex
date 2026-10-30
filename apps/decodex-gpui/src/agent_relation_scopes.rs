//! Workspace membership is visible on the canvas, without hiding cross-workspace edges.
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

pub(super) const HEADER_HEIGHT: f32 = 24.;
const TITLE_SIZE: f32 = 12.;
const COUNT_SIZE: f32 = 11.;
const LABEL_PADDING: f32 = 6.;
const LABEL_GAP: f32 = 8.;

fn corner_radius(scale: f32) -> f32 {
	(16. * scale).clamp(4., 16.)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super::super) enum Selection {
	Project(String),
	Unassigned,
}

pub(super) struct Scope {
	pub id: Option<String>,
	pub title: String,
	pub members: BTreeSet<String>,
	pub bounds: [f32; 4],
}

impl Scope {
	pub(super) fn selection(&self) -> Selection {
		self.id.as_ref().map_or(Selection::Unassigned, |id| Selection::Project(id.clone()))
	}

	fn heading(&self) -> String {
		if self.id.is_some() { self.title.clone() } else { "No project".into() }
	}

	fn count_label(&self) -> String {
		let count = self.members.len();

		format!("{count} {}", if count == 1 { "agent" } else { "agents" })
	}

	pub(super) fn label(&self) -> String {
		format!(
			"{}{} · {}",
			if self.id.is_some() { "Project: " } else { "" },
			self.heading(),
			self.count_label()
		)
	}

	pub(super) fn header_width(&self, text: &WindowTextSystem) -> f32 {
		let measure = |value: String, size: f32, numeric: bool| {
			let len = value.len();
			let mut font = gpui::font(ui_theme::FONT_FAMILY);

			if numeric {
				font.features = card::number_features();
			}

			f32::from(
				text.shape_line(
					value.into(),
					gpui::px(size),
					&[TextRun { len, font, ..Default::default() }],
					None,
				)
				.width,
			)
		};

		2. * LABEL_PADDING
			+ LABEL_GAP
			+ 4.
			+ measure(self.heading(), TITLE_SIZE, false).min(240.)
			+ measure(self.count_label(), COUNT_SIZE, true)
	}

	/// Pin the title to the visible portion without turning the whole group into a hit target.
	pub(super) fn header_rect(
		&self,
		scale: f32,
		pan: (f32, f32),
		viewport: (f32, f32),
		width: f32,
	) -> Option<card::Rect> {
		let [left, top, right, bottom] = self.bounds;
		let (left, top, right, bottom) = (
			left * scale + pan.0,
			top * scale + pan.1,
			right * scale + pan.0,
			bottom * scale + pan.1,
		);

		if right < 0. || left > viewport.0 || bottom < HEADER_HEIGHT || top > viewport.1 {
			return None;
		}

		let width = width.min((viewport.0 - 8.).max(0.));
		let inset = corner_radius(scale) + 12.;
		let embedded = self.id.is_some()
			&& 2. * GRID * scale >= HEADER_HEIGHT / 2. + 4.
			&& right - left >= width + 2. * inset;

		Some(card::Rect {
			x: (left + if embedded { inset } else { 4. })
				.clamp(4., (viewport.0 - width - 4.).max(4.)),
			y: (top - if embedded { HEADER_HEIGHT / 2. } else { HEADER_HEIGHT })
				.max(4.)
				.min(bottom - HEADER_HEIGHT),
			width,
			height: HEADER_HEIGHT,
		})
	}

	/// A quiet dashed boundary keeps membership visible without enclosing a card.
	pub(super) fn outline(
		&self,
		scale: f32,
		selected: bool,
		title_gap: Option<(f32, f32)>,
	) -> AnyElement {
		let id = format!("project-scope-{}", self.id.as_deref().unwrap_or("unassigned"));

		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, cx| {
				let active = ui_motion::value(id.clone(), u8::from(selected) as f32, window, cx);
				let density = window.scale_factor();
				let snap = |value: f32| (value * density).round() / density;
				let origin = gpui::point(
					gpui::px(snap(f32::from(bounds.left()))),
					gpui::px(snap(f32::from(bounds.top()))),
				);
				let radius = snap(corner_radius(scale));
				let stroke = snap(1.);
				let left = stroke / 2.;
				let top = stroke / 2.;
				let right = snap(f32::from(bounds.right())) - f32::from(origin.x) - stroke / 2.;
				let bottom = snap(f32::from(bounds.bottom())) - f32::from(origin.y) - stroke / 2.;
				let point = |x, y| origin + gpui::point(gpui::px(x), gpui::px(y));
				let dashed = || {
					PathBuilder::stroke(gpui::px(stroke)).dash_array(&[gpui::px(4.), gpui::px(6.)])
				};
				let gap = title_gap.map(|(start, end)| {
					(
						snap(start).clamp(left + radius, right - radius),
						snap(end).clamp(left + radius, right - radius),
					)
				});
				let mut path = dashed();

				path.move_to(point(left + radius, top));
				if let Some((start, end)) = gap {
					path.line_to(point(start, top));
					path.move_to(point(end, top));
				}
				path.line_to(point(right - radius, top));
				path.curve_to(point(right, top + radius), point(right, top));
				path.line_to(point(right, bottom - radius));
				path.curve_to(point(right - radius, bottom), point(right, bottom));
				path.line_to(point(left + radius, bottom));
				path.curve_to(point(left, bottom - radius), point(left, bottom));
				path.line_to(point(left, top + radius));
				path.curve_to(point(left + radius, top), point(left, top));

				if let Ok(path) = path.build() {
					window.paint_path(path, ui_theme::rgb(ui_theme::TEXT_MUTED).opacity(0.24));
				}

				// Selection belongs to the legend, not a second bright box around
				// the whole team. The short continuation dissolves into the border.
				if active > 0.001
					&& let Some((_, end)) = gap
				{
					let mut accent = dashed();

					accent.move_to(point(end, top));
					accent.line_to(point((end + 48.).min(right - radius), top));
					if let Ok(path) = accent.build() {
						window.paint_path(
							path,
							gpui::linear_gradient(
								90.,
								gpui::linear_color_stop(
									ui_theme::rgb(ui_theme::BLUE).opacity(active * 0.6),
									0.,
								),
								gpui::linear_color_stop(
									ui_theme::rgb(ui_theme::BLUE).opacity(0.),
									1.,
								),
							),
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
		self.work_board.view.picked_scope.as_ref() == Some(&scope.selection())
			|| self
				.work_board
				.view
				.picked_agent
				.as_ref()
				.is_some_and(|key| scope.members.contains(key))
	}

	/// Share final label positions with the border, including overlap adjustment.
	pub(super) fn relation_scope_labels<'a>(
		&self,
		graph: &'a Graph,
	) -> Vec<(&'a Scope, card::Rect)> {
		let text = WindowTextSystem::new(self.work_board.text_system.clone());
		let mut placed = Vec::new();
		let mut labels = Vec::new();

		for scope in &graph.scopes {
			let width = scope.header_width(&text);
			let Some(rect) = scope.header_rect(
				self.relation_scale(),
				self.work_board.view.pan,
				self.work_board.view.viewport,
				width,
			) else {
				continue;
			};
			let Some(rect) = place_header(rect, &placed, self.work_board.view.viewport.1) else {
				continue;
			};
			let rect = clear_header_routes(
				rect,
				scope,
				graph,
				self.relation_scale(),
				self.work_board.view.pan,
				self.work_board.view.viewport.0,
				&placed,
			);

			placed.push(rect);
			labels.push((scope, rect));
		}

		labels
	}

	pub(super) fn relation_scope_headers(
		&self,
		mut canvas: Stateful<Div>,
		graph: &Graph,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		for (scope, rect) in self.relation_scope_labels(graph) {
			let heading = scope.heading();
			let count = scope.count_label();
			let selection = scope.selection();
			let selected = self.relation_scope_selected(scope);
			let id = format!("project-title-{}", scope.id.as_deref().unwrap_or("unassigned"));
			let action: card::PointerAction<ClickEvent> =
				std::rc::Rc::new(cx.listener(move |s, _, _, cx| {
					s.frame_relation_project(selection.clone(), cx);
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
				.gap(gpui::px(LABEL_GAP))
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
				.child(gpui::div().flex_1().min_w_0().truncate().child(heading))
				.child(
					gpui::div()
						.flex_none()
						.text_size(gpui::px(COUNT_SIZE))
						.font_features(card::number_features())
						.text_color(ui_theme::rgb(ui_theme::foreground::SECONDARY))
						.child(count),
				);

			canvas = canvas.child(
				gpui::div()
					.absolute()
					.left(gpui::px(rect.x))
					.top(gpui::px(rect.y))
					.w(gpui::px(rect.width))
					.h(gpui::px(rect.height))
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

/// Slide a caption along its own group's top edge when a wire would cross the text.
fn clear_header_routes(
	rect: card::Rect,
	scope: &Scope,
	graph: &Graph,
	zoom: f32,
	pan: (f32, f32),
	viewport_width: f32,
	placed: &[card::Rect],
) -> card::Rect {
	let clear = |candidate: card::Rect| {
		let bounds = [
			(candidate.x - pan.0 - 4.) / zoom,
			(candidate.y - pan.1 - 2.) / zoom,
			(candidate.x + candidate.width - pan.0 + 4.) / zoom,
			(candidate.y + candidate.height - pan.1 + 2.) / zoom,
		];

		placed.iter().all(|other| {
			candidate.x + candidate.width + 4. <= other.x
				|| other.x + other.width + 4. <= candidate.x
				|| candidate.y + candidate.height + 4. <= other.y
				|| other.y + other.height + 4. <= candidate.y
		}) && graph
			.paths
			.values()
			.all(|path| path.ink.iter().all(|curve| !curve.intersects_bounds(bounds)))
	};

	if clear(rect) {
		return rect;
	}

	let right = (scope.bounds[2] * zoom + pan.0 - corner_radius(zoom) - 12.)
		.min(viewport_width - 4.)
		- rect.width;
	let steps = ((right - rect.x) / 16.).floor().max(0.) as usize;

	(1..=steps)
		.map(|step| card::Rect { x: rect.x + step as f32 * 16., ..rect })
		.find(|candidate| clear(*candidate))
		.unwrap_or(rect)
}

/// Small-zoom titles can overlap even though the project geometry does not.
/// Move only the label, keeping node positions and camera anchors unchanged.
fn place_header(rect: card::Rect, placed: &[card::Rect], height: f32) -> Option<card::Rect> {
	std::iter::once(rect.y)
		.chain((1..=placed.len()).flat_map(|step| {
			let offset = step as f32 * (HEADER_HEIGHT + 4.);

			[rect.y - offset, rect.y + offset]
		}))
		.find(|y| {
			*y >= 4.
				&& *y + rect.height <= height - 4.
				&& placed.iter().all(|other| {
					rect.x + rect.width + 4. <= other.x
						|| other.x + other.width + 4. <= rect.x
						|| *y + rect.height + 4. <= other.y
						|| other.y + other.height + 4. <= *y
				})
		})
		.map(|y| card::Rect { y, ..rect })
}

impl Graph {
	pub(super) fn scope_bounds(&mut self, projects: &[ProjectDto]) {
		if !self.nodes.iter().any(|n| n.row.as_ref().is_some_and(|r| r.workspace.is_some())) {
			return;
		}

		// A root that coordinates across projects belongs above those groups.
		// Including it in "No Project" would stretch that frame across all branches.
		let coordinator = self
			.nodes
			.iter()
			.find(|node| {
				node.row.as_ref().is_some_and(|row| row.workspace.is_none())
					&& !self.edges.iter().any(|edge| edge.kind.hierarchy() && edge.to == node.key)
					&& self.edges.iter().any(|edge| {
						edge.kind.hierarchy()
							&& edge.from == node.key
							&& self.nodes.iter().any(|child| {
								child.key == edge.to
									&& child.row.as_ref().is_some_and(|row| row.workspace.is_some())
							})
					})
			})
			.map(|node| node.key.as_str());

		for node in &self.nodes {
			if coordinator == Some(node.key.as_str()) {
				continue;
			}

			let Some(row) = &node.row else { continue };
			let bounds = [
				((node.x - GRID) / GRID).floor() * GRID,
				((node.y - 2. * GRID) / GRID).floor() * GRID,
				((node.x + node.width() + GRID) / GRID).ceil() * GRID,
				((node.y + node.height() + GRID) / GRID).ceil() * GRID,
			];

			if let Some(scope) = self.scopes.iter_mut().find(|g| g.id == row.workspace) {
				scope.bounds[0] = scope.bounds[0].min(bounds[0]);
				scope.bounds[1] = scope.bounds[1].min(bounds[1]);
				scope.bounds[2] = scope.bounds[2].max(bounds[2]);
				scope.bounds[3] = scope.bounds[3].max(bounds[3]);
				scope.members.insert(node.key.clone());
			} else {
				let title = row
					.workspace
					.as_ref()
					.and_then(|id| projects.iter().find(|w| &w.id == id))
					.map(|w| w.name.clone())
					.unwrap_or_else(|| "No Project".into());

				self.scopes.push(Scope {
					id: row.workspace.clone(),
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
	fn small_zoom_titles_remain_separate_without_moving_their_horizontal_anchor() {
		let mut placed = Vec::new();

		for x in [200., 250., 300.] {
			let rect = place_header(
				card::Rect { x, y: 80., width: 180., height: HEADER_HEIGHT },
				&placed,
				200.,
			)
			.unwrap();

			assert_eq!(rect.x, x);
			assert!(
				placed
					.iter()
					.all(|other: &card::Rect| (other.y - rect.y).abs() >= HEADER_HEIGHT + 4.)
			);
			placed.push(rect);
		}
	}

	#[test]
	fn pinned_caption_moves_past_a_crossing_wire_without_changing_its_row() {
		let curve =
			super::super::routing::Route::curve((80., -40.), (80., -10.), (80., 10.), (80., 40.));
		let graph = Graph {
			layout: card::Layout::new(0.75),
			scopes: vec![],
			nodes: vec![],
			edges: vec![],
			paths: std::collections::BTreeMap::from([("wire".into(), std::rc::Rc::new(curve))]),
		};
		let scope = Scope {
			id: Some("project".into()),
			title: "Project".into(),
			members: BTreeSet::new(),
			bounds: [0., 0., 600., 300.],
		};
		let rect = card::Rect { x: 41., y: 11., width: 100., height: HEADER_HEIGHT };
		let placed = clear_header_routes(rect, &scope, &graph, 0.75, (17., 23.), 500., &[]);

		assert!(placed.x > 81. && placed.x + placed.width < 500.);
		assert_eq!(placed.y, rect.y);
		assert_eq!(placed.width, rect.width);
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
	fn project_framing_keeps_chats_and_positions_in_all_layouts(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(fixture);

		surface.update(cx, |s, cx| {
			let current = (s.selected.clone(), s.native_agents.selected.clone());
			let selection = Selection::Project("project".into());

			for mode in layout::Mode::ALL {
				for presentation in [layout::Presentation::Links, layout::Presentation::Together] {
					s.work_board.view.layout_mode = mode;
					s.work_board.view.presentation = presentation;
					s.work_board.view.zoom = Some(0.1);
					s.work_board.view.pan = (-2_000., -2_000.);
					s.frame_relation_project(selection.clone(), cx);

					let graph = s.relation_graph();
					let scope =
						graph.scopes.iter().find(|scope| scope.selection() == selection).unwrap();
					let zoom = s.relation_scale();
					let (x, y) = s.work_board.view.pan;
					let [left, top, right, bottom] = scope.bounds;

					assert!(left * zoom + x >= 15.9);
					assert!(top * zoom + y >= HEADER_HEIGHT + 15.9);
					assert!(right * zoom + x <= 1_184.1);
					assert!(bottom * zoom + y <= 684.1);
					assert_eq!(s.work_board.view.picked_scope.as_ref(), Some(&selection));
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
			assert!(s.work_board.view.picked_scope.is_none());
		});
	}

	#[gpui::test]
	fn project_legend_stays_on_its_border_after_other_group_headers(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_600.), gpui::px(900.)));
		surface.update(visual, |s, cx| {
			s.work_board.view.zoom = Some(0.75);
			let graph = s.relation_graph();
			let scope = graph.scopes.iter().find(|scope| scope.id.is_some()).unwrap();

			s.work_board.view.pan = (80. - scope.bounds[0] * 0.75, 80. - scope.bounds[1] * 0.75);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		let canvas = visual.debug_bounds("relations-canvas").unwrap();
		let title = visual.debug_bounds("project-title-project").unwrap();

		surface.read_with(visual, |s, _| {
			let graph = s.work_board.view.layers.graph.borrow();
			let scope =
				graph.as_ref().unwrap().scopes.iter().find(|scope| scope.id.is_some()).unwrap();
			let border_y = f32::from(canvas.origin.y)
				+ scope.bounds[1] * s.relation_scale()
				+ s.work_board.view.pan.1;

			assert!(
				(f32::from(title.center().y) - border_y).abs() < 1.,
				"the legend must straddle the border, not inherit a previous header's flow offset"
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
		let current = surface
			.read_with(visual, |s, _| (s.selected.clone(), s.native_agents.selected.clone()));
		let title = visual.debug_bounds("project-title-project").unwrap();

		visual.simulate_click(title.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		surface.read_with(visual, |s, cx| {
			assert_eq!(s.work_board.view.picked_scope, Some(Selection::Project("project".into())));
			assert!(s.work_board.view.picked_agent.is_none());
			assert!(s.work_board.view.drag.is_none());
			assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), current);
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		});

		// A title remains usable when individual agent labels disappear.
		surface.update(visual, |s, cx| {
			s.work_board.view.zoom = Some(0.1);
			s.work_board.view.card_transition = None;
			let graph = s.relation_graph();
			let scope = graph.scopes.iter().find(|scope| scope.id.is_some()).unwrap();

			s.work_board.view.pan = (60. - scope.bounds[0] * 0.1, 80. - scope.bounds[1] * 0.1);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("project-title-project").is_some());

		let title = visual.debug_bounds("project-title-unassigned").unwrap();
		visual.simulate_click(title.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		assert!(
			visual.debug_bounds("project-unassigned").is_none(),
			"unassigned agents are not a framed team"
		);
		surface.read_with(visual, |s, _| {
			assert_eq!(s.work_board.view.picked_scope, Some(Selection::Unassigned))
		});

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
				assert_eq!(s.work_board.view.picked_scope, Some(Selection::Unassigned))
			});
		}
	}
}
