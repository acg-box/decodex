//! Exercise the production popup host with synthetic content and no daemon or chat.
use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::{
	App, AppContext as _, Bounds, Context, IntoElement, Render, Window, WindowBounds,
	WindowOptions,
	prelude::{FluentBuilder as _, ParentElement as _, Styled as _},
};
use serde_json::{Value, json};

use crate::{
	ui_motion,
	ui_theme::{
		self,
		material::{GlassScope, GlassStyle, Material, Surface},
		native_material_panel,
	},
};

struct Fixture {
	open: [bool; 2],
	large: bool,
	attached: bool,
	compact: bool,
	opacity: f32,
	center_y: Option<f32>,
}
impl Render for Fixture {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		window.set_window_title(&format!(
			"Overlay qualification - {} - {:?} - {} - {}",
			Surface::Popover.material().label(),
			GlassScope::Popover.configured(),
			if self.attached { "islands" } else { "menus" },
			if self.large { "large" } else { "small" }
		));
		let large = self.large;
		let compact = self.compact;
		let opacity = self.opacity;
		let center_y = self.center_y;
		let mut root = gpui::div().size_full().bg(gpui::rgb(0x1d2432)).child(
			gpui::canvas(
				|_, _, _| (),
				|bounds, _, window, _| {
					for index in 0..(f32::from(bounds.size.width) / 24.).ceil() as u32 {
						window.paint_quad(gpui::fill(
							Bounds::new(
								bounds.origin
									+ gpui::point(gpui::px(index as f32 * 24.), gpui::px(0.)),
								gpui::size(gpui::px(24.), bounds.size.height),
							),
							gpui::rgb(if index.is_multiple_of(2) { 0x101a28 } else { 0xd4c29a }),
						));
					}
				},
			)
			.absolute()
			.inset_0(),
		);
		for index in 0..2 {
			let owner = cx.entity().downgrade();
			root = root.child(
				gpui::div()
					.absolute()
					.top(gpui::px(100.))
					.left(gpui::px(80.))
					.w(gpui::px(if large { 292. } else { 224. }))
					.child(
						ui_motion::popover_lazy(
							format!("qualified-popup-{index}"),
							self.open[index],
							move |_, _| {
								gpui::div()
									.p(gpui::px(ui_theme::POPOVER_CONTENT_PADDING))
									.flex()
									.flex_col()
									.children((0..if large { 7 } else { 3 }).map(|row| {
										ui_theme::menu_row(format!("fixture-{row}"), true)
											.child(format!("Synthetic option {}", row + 1))
									}))
									.into_any_element()
							},
						)
						.when_some(center_y, |popup, y| {
							popup.preview().left_centered(320.).trigger(Rc::new(Cell::new(Some(
								Bounds::new(
									gpui::point(gpui::px(600.), gpui::px(y - 22.)),
									gpui::size(gpui::px(12.), gpui::px(44.)),
								),
							))))
						})
						.on_dismiss(move |cx| {
							let _ = owner.update(cx, |s, cx| {
								s.open[index] = false;
								cx.notify();
							});
						}),
					),
			);
		}
		for (index, label) in ["2.6M tok", "60 FPS"].into_iter().enumerate() {
			let id = format!("fixture-island-{index}");
			if self.attached {
				let height = if large && !compact { 120. } else { 28. };
				root = root.child(
					gpui::div()
						.absolute()
						.top(gpui::px(20.))
						.left(gpui::px(180. + index as f32 * 90.))
						.w(gpui::px(if compact { 28. } else { 80. }))
						.h(gpui::px(height))
						.child(
							ui_motion::material_surface(
								id,
								if large { 20. } else { 14. },
								move |_, _| {
									gpui::div()
										.w_full()
										.h(gpui::px(height))
										.child(if compact { "↓" } else { label })
										.into_any_element()
								},
								|_| {},
							)
							.opacity(opacity),
						),
				);
			} else {
				ui_motion::hide_material_surface(id, window, cx);
			}
		}
		root
	}
}

fn sample(cx: &mut App) -> Value {
	let windows: Vec<_> = cx
		.windows()
		.into_iter()
		.filter_map(|handle| {
			handle
				.update(cx, |_, window, _| {
                    let native = crate::probe::native_window(window);
                    let frame = native.frame();
                    let content = native.convertRectToScreen(native.contentView().expect("content view").bounds());
					json!({
                        "frame": [frame.origin.x, frame.origin.y, frame.size.width, frame.size.height],
                        "content_frame": [content.origin.x, content.origin.y, content.size.width, content.size.height],
						"id":format!("{:?}",handle.window_id()), "visible":window.is_visible(),
						"material":native_material_panel::observed_material(window),
						"style":native_material_panel::observed_style(window),
						"tinted":native_material_panel::observed_tint(window),
						"width":f32::from(window.viewport_size().width),
						"height":f32::from(window.viewport_size().height),
						"scale":window.scale_factor(),
						"rendering":native_material_panel::observed_rendering(window),
					})
				})
				.ok()
		})
		.collect();
	json!({"window_count":windows.len(),"windows":windows,"application_active":crate::ui_frame::application_active()})
}

fn qualify_sample(row: &Value, material: Material, style: GlassStyle) {
	assert_eq!(row["window_count"], 2);
	let child = row["windows"]
		.as_array()
		.expect("native windows")
		.iter()
		.find(|window| window["width"].as_f64().is_some_and(|width| width < 300.))
		.expect("popup window");
	assert_eq!(child["visible"], true);
	assert_eq!(child["rendering"]["window_shadow"], true);
	assert_eq!(child["rendering"]["window_background_alpha"], 0.);
	assert_eq!(child["material"], if material == Material::Thin { "thin" } else { "liquid-glass" });
	if material == Material::LiquidGlass {
		assert_eq!(child["style"], if style == GlassStyle::Clear { 1 } else { 0 });
		assert_eq!(child["tinted"], style != GlassStyle::Clear);
	}
}

pub fn run() {
	let islands_only = std::env::args().any(|arg| arg == "--popup-material-islands");
	let centered_only = std::env::args().any(|arg| arg == "--popup-centered-preview");
	let library =
		std::env::var("DECODEX_PROBE_NATIVE_LIBRARY").expect("explicit native fixture library");
	let library = std::ffi::CString::new(library).expect("native fixture path");
	assert!(
		!unsafe { libc::dlopen(library.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) }.is_null()
	);
	let output = std::env::var_os("DECODEX_POPUP_PROBE_OUTPUT").expect("explicit probe output");
	gpui_platform::application().run(move |cx| {
		let parent = cx
			.open_window(
				WindowOptions {
					window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
						None,
						gpui::size(gpui::px(640.), gpui::px(480.)),
						cx,
					))),
					..Default::default()
				},
				|_, cx| {
					cx.new(|_| Fixture {
						open: [false; 2],
						large: false,
						attached: false,
						compact: false,
						opacity: 1.,
						center_y: None,
					})
				},
			)
			.expect("isolated fixture window");
		cx.activate(true);
		cx.spawn(async move |cx| {
			let mut samples = Vec::new();
			if centered_only {
				let report = qualify_centered(parent, cx).await;
				parent.update(cx, |_, window, _| window.remove_window()).expect("close parent");
				cx.background_executor().timer(Duration::from_millis(100)).await;
				assert_eq!(cx.update(sample)["window_count"], 0);
				std::fs::write(output, serde_json::to_vec_pretty(&report).expect("probe report"))
					.expect("save report");
				cx.update(|cx| cx.quit());
				return;
			}
			let menus = if islands_only {
				vec![]
			} else {
				vec![
					(Material::Thin, GlassStyle::Regular),
					(Material::LiquidGlass, GlassStyle::Regular),
					(Material::LiquidGlass, GlassStyle::Clear),
				]
			};
			for (material, style) in menus {
				cx.update(|cx| {
					Surface::Popover.select(material, cx);
					GlassScope::Popover.select(style, cx);
				});
				for (index, large) in [(0, false), (0, true), (1, false)] {
					cx.update(|cx| cx.activate(true));
					parent
						.update(cx, |s, window, cx| {
							window.activate_window();
							s.open[index] = true;
							s.large = large;
							cx.notify();
						})
						.expect("show fixture");
					cx.background_executor().timer(Duration::from_millis(50)).await;
					samples.push(json!({
						"phase":"arrival", "requested_material":material.label(),
						"requested_style":format!("{style:?}"), "state":cx.update(sample),
					}));
					cx.background_executor().timer(settle()).await;
					let row = cx.update(sample);
					let open = parent.update(cx, |s, _, _| s.open).expect("fixture flags");
					assert_eq!(open, if index == 0 { [true, false] } else { [false, true] });
					qualify_sample(&row, material, style);
					samples.push(json!({
						"requested_material":material.label(), "requested_style":format!("{style:?}"),
						"large":large, "owner_open":open, "state":row,
					}));
				}
				parent
					.update(cx, |s, _, cx| {
						s.open = [false; 2];
						cx.notify();
					})
					.expect("close fixture");
				cx.background_executor().timer(Duration::from_millis(150)).await;
				samples.push(json!({"closed":true,"state":cx.update(sample)}));
			}
			qualify_islands(parent, cx, &mut samples, if islands_only { 3 } else { 4 }).await;
			parent.update(cx, |_, window, _| window.remove_window()).expect("close parent");
			cx.background_executor().timer(Duration::from_millis(100)).await;
			let final_state = cx.update(sample);
			assert_eq!(final_state["window_count"], 0);
			let report = json!({
				"schema":1, "scope":"Synthetic popup content only; no account or conversation requests",
				"samples":samples, "after_parent_close":final_state, "menus_qualified":!islands_only,
			});
			std::fs::write(output, serde_json::to_vec_pretty(&report).expect("probe report"))
				.expect("save report");
			cx.update(|cx| cx.quit());
		})
		.detach();
	});
}

async fn qualify_centered(parent: gpui::WindowHandle<Fixture>, cx: &mut gpui::AsyncApp) -> Value {
	let mut samples = Vec::new();
	parent
		.update(cx, |s, window, cx| {
			window.activate_window();
			s.center_y = Some(240.);
			s.open = [true, false];
			cx.notify();
		})
		.expect("show centered preview");
	cx.background_executor().timer(settle()).await;
	let initial = cx.update(sample);
	let popup = |row: &Value| {
		row["windows"]
			.as_array()
			.expect("windows")
			.iter()
			.find(|window| window["width"] == 320.)
			.expect("centered popup")
			.clone()
	};
	let center = |row: &Value| {
		let frame = &row["frame"];
		frame[1].as_f64().expect("y") + frame[3].as_f64().expect("height") / 2.
	};
	let initial = popup(&initial);
	let expected_center = center(&initial);
	let small = initial["height"].as_f64().expect("small height");
	let mut seen_intermediate = false;
	for (large, frames) in [(true, 10), (false, 3), (true, 10), (false, 10)] {
		parent
			.update(cx, |s, _, cx| {
				s.large = large;
				cx.notify();
			})
			.expect("change preview content");
		for frame in 0..frames {
			cx.background_executor().timer(Duration::from_millis(24)).await;
			let row = popup(&cx.update(sample));
			let height = row["height"].as_f64().expect("height");
			assert!((center(&row) - expected_center).abs() < 1., "native center moved: {row}");
			assert_eq!(row["id"], initial["id"], "reuse the native preview");
			assert_eq!(row["width"], 320.);
			seen_intermediate |= height > small + 1. && height < small + 110.;
			samples.push(json!({"large":large,"frame":frame,"popup":row}));
		}
	}
	assert!(seen_intermediate, "native resizing must have intermediate frames");
	for edge in [0., 480.] {
		parent
			.update(cx, |s, _, cx| {
				s.center_y = Some(edge);
				cx.notify();
			})
			.expect("move anchor");
		cx.background_executor().timer(settle()).await;
		let row = cx.update(sample);
		let child = popup(&row);
		let parent_row = row["windows"]
			.as_array()
			.expect("window list")
			.iter()
			.find(|window| window["width"] != 320.)
			.expect("parent window");
		let available = &parent_row["content_frame"];
		let frame = &child["frame"];
		let bottom = frame[1].as_f64().expect("frame origin");
		let top = bottom + frame[3].as_f64().expect("frame height");
		assert!(bottom >= available[1].as_f64().expect("content origin") + 7.5);
		assert!(
			top <= available[1].as_f64().expect("content origin")
				+ available[3].as_f64().expect("content height")
				- 7.5
		);
		samples.push(json!({"edge":edge,"popup":child}));
	}
	json!({"scope":"Production centered native popover; synthetic content; no service or conversation requests", "intermediate_frames":seen_intermediate,"samples":samples})
}

async fn qualify_islands(
	parent: gpui::WindowHandle<Fixture>,
	cx: &mut gpui::AsyncApp,
	samples: &mut Vec<Value>,
	window_count: usize,
) {
	for (material, style) in [
		(Material::Thin, GlassStyle::Regular),
		(Material::LiquidGlass, GlassStyle::Regular),
		(Material::LiquidGlass, GlassStyle::Clear),
	] {
		cx.update(|cx| {
			cx.activate(true);
			Surface::Popover.select(material, cx);
			GlassScope::Popover.select(style, cx);
		});
		for large in [false, true] {
			parent
				.update(cx, |s, window, cx| {
					window.activate_window();
					s.attached = true;
					s.large = large;
					cx.notify();
				})
				.expect("show islands");
			cx.background_executor().timer(settle()).await;
			let row = cx.update(sample);
			assert_eq!(row["window_count"], window_count);
			let islands: Vec<_> = row["windows"]
				.as_array()
				.expect("windows")
				.iter()
				.filter(|w| w["width"] == 80.)
				.collect();
			assert_eq!(islands.len(), 2);
			for island in islands {
				assert_eq!(island["visible"], true, "native island state: {row}");
				assert_eq!(island["height"], if large { 120. } else { 28. });
				assert_eq!(
					island["material"],
					if material == Material::Thin { "thin" } else { "liquid-glass" }
				);
				if material == Material::LiquidGlass {
					assert_eq!(island["style"], if style == GlassStyle::Clear { 1 } else { 0 });
					assert_eq!(island["tinted"], style != GlassStyle::Clear);
				}
			}
			samples.push(json!({"islands":true,"large":large,"requested_material":material.label(),"requested_style":format!("{style:?}"),"state":row}));
			if style == GlassStyle::Clear
				&& large
				&& let Some(path) = std::env::var_os("DECODEX_POPUP_PROBE_CAPTURE_READY")
			{
				std::fs::write(path, b"clear-islands-ready").expect("capture readiness");
				cx.background_executor().timer(Duration::from_secs(5)).await;
			}
		}
	}
	// Small persistent controls must keep the selected overlay material while the
	// complete native composite fades, not just its foreground glyph.
	for (material, style) in [
		(Material::Thin, GlassStyle::Regular),
		(Material::LiquidGlass, GlassStyle::Regular),
		(Material::LiquidGlass, GlassStyle::Clear),
	] {
		cx.update(|cx| {
			Surface::Popover.select(material, cx);
			GlassScope::Popover.select(style, cx);
		});
		for opacity in [1., 0.5, 0.] {
			parent
				.update(cx, |s, _, cx| {
					s.compact = true;
					s.opacity = opacity;
					cx.notify();
				})
				.expect("fade compact controls");
			cx.background_executor().timer(settle()).await;
			let row = cx.update(sample);
			let controls: Vec<_> = row["windows"]
				.as_array()
				.expect("windows")
				.iter()
				.filter(|w| w["width"] == 28.)
				.collect();
			assert_eq!(controls.len(), 2);
			for control in controls {
				assert_eq!(control["height"], 28.);
				assert_eq!(control["rendering"]["window_alpha"], opacity);
				assert_eq!(
					control["material"],
					if material == Material::Thin { "thin" } else { "liquid-glass" }
				);
				if material == Material::LiquidGlass {
					assert_eq!(control["style"], if style == GlassStyle::Clear { 1 } else { 0 });
				}
			}
			samples.push(json!({"compact_control":true,"opacity":opacity,"requested_material":material.label(),"requested_style":format!("{style:?}"),"state":row}));
		}
	}
	parent
		.update(cx, |s, _, cx| {
			s.attached = false;
			cx.notify();
		})
		.expect("hide islands");
	cx.background_executor().timer(Duration::from_millis(150)).await;
	let row = cx.update(sample);
	assert_eq!(
		row["windows"].as_array().expect("windows").iter().filter(|w| w["visible"] == true).count(),
		1
	);
	samples.push(json!({"islands_hidden":true,"state":row}));
}

fn settle() -> Duration {
	Duration::from_millis(
		std::env::var("DECODEX_POPUP_PROBE_STEP_MS")
			.ok()
			.and_then(|v| v.parse().ok())
			.unwrap_or(350),
	)
}
