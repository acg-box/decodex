//! Graph layout and presentation share the same click-and-drag segments.
use gpui::{AnyElement, IntoElement as _};

use crate::{
	shell::agent_surface::{
		work_board::relations::{
			AgentSurface, Context,
			layout::{self, Presentation},
		},
		workspace,
	},
	ui_motion::{Choice, Segmented},
};

impl AgentSurface {
	pub(super) fn relation_presentation_picker(&self, cx: &mut Context<Self>) -> AnyElement {
		Segmented::new(
			"relations-presentation-picker",
			"Agent view: links or together",
			[
				Choice::new("relations-mode-links", "Links", "Links view"),
				Choice::new("relations-mode-together", "Together", "Together view"),
			],
			usize::from(self.work_board.view.presentation == Presentation::Together),
			cx.listener(|s, index: &usize, window, cx| {
				s.switch_relation_presentation(
					if *index == 0 { Presentation::Links } else { Presentation::Together },
					cx,
				);
				workspace::restore_shell_focus(window, cx);
			}),
		)
		.graph("relations-presentation-thumb")
		.into_any_element()
	}

	pub(super) fn relation_layout_picker(&self, cx: &mut Context<Self>) -> AnyElement {
		Segmented::new(
			"relations-layout-picker",
			"Graph layout. Click a layout or drag between layouts.",
			layout::Mode::ALL.into_iter().map(|mode| {
				Choice::new(
					format!("relations-layout-{}", mode.label().to_lowercase()),
					mode.label(),
					format!("{} layout", mode.label()),
				)
			}),
			layout::Mode::ALL
				.iter()
				.position(|m| *m == self.work_board.view.layout_mode)
				.unwrap_or(0),
			cx.listener(|s, index: &usize, window, cx| {
				s.switch_relation_layout(layout::Mode::ALL[*index], cx);
				workspace::restore_shell_focus(window, cx);
			}),
		)
		.graph("relations-layout-thumb")
		.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use std::thread;

	use gpui::MouseButton;

	use crate::shell::agent_surface::work_board::{
		relations::picker::{AgentSurface, Context, layout},
		tests,
	};

	fn fixture(cx: &mut Context<AgentSurface>) -> AgentSurface {
		let mut s = tests::fixture(cx);

		s.visual_workspace_page("workbench", cx);

		s.workspace.graph_expanded = true;
		s.work_board.view.camera_fixed = true;
		s.work_board.view.zoom = Some(0.7);
		s.work_board.view.picked_agent = Some("release".into());

		s.composer.update(cx, |input, cx| input.set_content("Preserve the Main draft", cx));

		s
	}

	#[gpui::test]
	fn presentation_switch_preserves_chat_draft_facts_camera_anchor_and_manual_positions(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_800.), gpui::px(1_000.)));

		let (main, facts, anchor) = surface.update(visual, |s, _| {
			s.work_board.view.picked_agent = Some("mira".into());

			s.work_board.view.positions.insert("mira".into(), (176., 96.));

			let graph = s.relation_graph();
			let node = graph.nodes.iter().find(|n| n.key == "mira").unwrap();
			let point = node.connection_center();

			(
				(s.selected.clone(), s.native_agents.selected.clone()),
				graph.edges.iter().map(|e| (e.id.clone(), e.kind)).collect::<Vec<_>>(),
				(
					point.0 * s.relation_scale() + s.work_board.view.pan.0,
					point.1 * s.relation_scale() + s.work_board.view.pan.1,
				),
			)
		});

		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();

		let button = visual.debug_bounds("relations-mode-together").unwrap();

		visual.simulate_click(button.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		surface.read_with(visual, |s, cx| {
			assert_eq!(s.work_board.view.presentation, layout::Presentation::Together);
			assert_eq!(s.work_board.view.layout_mode, layout::Mode::Tree);
			assert_eq!(s.work_board.view.zoom, Some(0.7));
			assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), main);
			assert_eq!(s.composer.read(cx).content(), "Preserve the Main draft");

			let graph = s.relation_graph();

			assert_eq!(
				graph.edges.iter().map(|e| (e.id.clone(), e.kind)).collect::<Vec<_>>(),
				facts
			);

			let point = graph.nodes.iter().find(|n| n.key == "mira").unwrap().connection_center();

			assert!((point.0 * s.relation_scale() + s.work_board.view.pan.0 - anchor.0).abs() < 1.);
			assert!((point.1 * s.relation_scale() + s.work_board.view.pan.1 - anchor.1).abs() < 1.);
		});

		thread::sleep(std::time::Duration::from_millis(600));

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("relation-owner-hint-mira").is_some());
		assert!(visual.debug_bounds("relation-metrics-mira").is_some());

		surface.read_with(visual, |s, _| {
			assert_eq!(s.relation_wire_presence(std::time::Instant::now()), 0.);
			assert!(
				!s.work_board
					.view
					.motion
					.as_ref()
					.unwrap()
					.progress
					.moving(std::time::Instant::now())
			);
		});

		let button = visual.debug_bounds("relations-mode-links").unwrap();

		visual.simulate_click(button.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		surface.read_with(visual, |s, _| {
			assert_eq!(s.work_board.view.presentation, layout::Presentation::Links);
			assert_eq!(s.work_board.view.positions["mira"], (176., 96.));
			assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), main);
		});

		assert!(visual.debug_bounds("relation-owner-hint-mira").is_some());
	}

	#[gpui::test]
	fn layout_picker_clicks_slide_to_each_mode_without_changing_the_chat(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);

		let main = surface
			.read_with(visual, |s, _| (s.selected.clone(), s.native_agents.selected.clone()));

		for (mode, selector) in [
			(layout::Mode::Radial, "relations-layout-radial"),
			(layout::Mode::Grouped, "relations-layout-grouped"),
			(layout::Mode::Tree, "relations-layout-tree"),
		] {
			let button = visual.debug_bounds(selector).unwrap();

			visual.simulate_click(button.center(), Default::default());
			visual.update(crate::ui_frame::draw_frame);
			surface.read_with(visual, |s, cx| {
				assert_eq!(s.work_board.view.layout_mode, mode);
				assert_eq!(s.work_board.view.zoom, Some(0.7));
				assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("release"));
				assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), main);
				assert_eq!(s.composer.read(cx).content(), "Preserve the Main draft");
			});

			thread::sleep(std::time::Duration::from_millis(230));

			visual.update(crate::ui_frame::draw_frame);

			assert_eq!(visual.debug_bounds("relations-layout-thumb").unwrap(), button);
		}
	}

	#[gpui::test]
	fn layout_picker_drag_tracks_the_grab_point_and_releases_outside_the_track(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);

		let tree = visual.debug_bounds("relations-layout-tree").unwrap();
		let start = tree.center() + gpui::point(gpui::px(12.), gpui::px(0.));

		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());

		for (delta, mode) in [
			(24., layout::Mode::Tree),
			(72., layout::Mode::Radial),
			(160., layout::Mode::Grouped),
			(-80., layout::Mode::Tree),
			(60., layout::Mode::Radial),
		] {
			visual.simulate_mouse_move(
				start + gpui::point(gpui::px(delta), gpui::px(32.)),
				Some(MouseButton::Left),
				Default::default(),
			);
			visual.update(crate::ui_frame::draw_frame);
			surface.read_with(visual, |s, _| {
				assert_eq!(s.work_board.view.layout_mode, mode);
				assert!(s.work_board.view.drag.is_none(), "picker drags cannot pan the canvas");
				assert!(s.work_board.view.map_drag.is_none());
			});

			let thumb = visual.debug_bounds("relations-layout-thumb").unwrap();

			assert!((f32::from(thumb.left() - tree.left()) - delta.clamp(0., 120.)).abs() < 0.1);
		}

		let canvas = visual.debug_bounds("relations-canvas").unwrap();
		let outside = gpui::point(start.x + gpui::px(60.), canvas.bottom() - gpui::px(10.));

		visual.simulate_mouse_up(outside, MouseButton::Left, Default::default());
		visual.update(crate::ui_frame::draw_frame);

		let pan = surface.read_with(visual, |s, _| {
			assert_eq!(s.work_board.view.layout_mode, layout::Mode::Radial);
			assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("release"));
			assert_eq!(s.work_board.view.zoom, Some(0.7));

			s.work_board.view.pan
		});

		visual.simulate_mouse_move(tree.center(), None, Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.work_board.view.layout_mode, layout::Mode::Radial);
			assert_eq!(s.work_board.view.pan, pan);
		});
	}

	#[gpui::test]
	fn layout_picker_supports_keyboard_selection(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));

		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);

		let tree = visual.debug_bounds("relations-layout-tree").unwrap();

		visual.simulate_mouse_down(tree.center(), MouseButton::Left, Default::default());

		let focus = visual.update(|window, cx| window.focused(cx).unwrap());

		visual.simulate_mouse_up(tree.center(), MouseButton::Left, Default::default());
		visual.update(|window, cx| window.focus(&focus, cx));

		for (key, mode) in [
			("right", layout::Mode::Radial),
			("end", layout::Mode::Grouped),
			("left", layout::Mode::Radial),
			("home", layout::Mode::Tree),
		] {
			visual.simulate_keystrokes(key);
			visual.update(crate::ui_frame::draw_frame);
			surface.read_with(visual, |s, _| assert_eq!(s.work_board.view.layout_mode, mode));
		}
	}

	#[gpui::test]
	fn presentation_picker_drags_without_panning_or_changing_the_conversation(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		let links = visual.debug_bounds("relations-mode-links").unwrap();
		let pan = surface.read_with(visual, |s, _| s.work_board.view.pan);
		visual.simulate_mouse_down(links.center(), MouseButton::Left, Default::default());
		visual.simulate_mouse_move(
			links.center() + gpui::point(gpui::px(60.), gpui::px(32.)),
			Some(MouseButton::Left),
			Default::default(),
		);
		visual.update(crate::ui_frame::draw_frame);
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.work_board.view.presentation, layout::Presentation::Together);
			assert_eq!(s.work_board.view.pan, pan);
			assert!(s.work_board.view.drag.is_none());
			assert_eq!(s.composer.read(cx).content(), "Preserve the Main draft");
		});
		visual.simulate_mouse_up(
			links.center() + gpui::point(gpui::px(60.), gpui::px(32.)),
			MouseButton::Left,
			Default::default(),
		);
		visual.simulate_mouse_move(links.center(), None, Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.work_board.view.presentation, layout::Presentation::Together)
		});
	}
}
