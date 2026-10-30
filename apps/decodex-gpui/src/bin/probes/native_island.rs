//! Display-backed activity-island measurement without a Shell, service or draft store.
use std::{env, time::Duration};

use gpui::{
	AppContext as _, Bounds, Context, Entity, IntoElement, Render, TitlebarOptions, Window,
	WindowBounds, WindowOptions,
	prelude::{ParentElement as _, Styled as _},
};
use objc2::{
	rc::Retained,
	runtime::{AnyClass, AnyObject},
};
use objc2_foundation::NSString;

use crate::{
	frame_trace, ui_frame,
	ui_performance::PerformanceIsland,
	ui_theme::appearance::Appearance,
	ui_work_metrics::{Live, Sample, Summary},
};

struct Fixture(Entity<PerformanceIsland>);
impl Render for Fixture {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.size_full()
			.bg(gpui::rgb(0x18202b))
			.flex()
			.justify_center()
			.pt(gpui::px(12.))
			.child(self.0.clone())
	}
}

pub(super) fn run() {
	assert!(env::var_os("DECODEX_FRAME_TRACE").is_some(), "explicit trace output");

	self::preferences();

	gpui_platform::application().run(|cx| {
		Appearance::Dark.apply(cx);

		frame_trace::start_for(cx, Duration::from_secs(14));

		let bounds = Bounds::centered(None, gpui::size(gpui::px(960.), gpui::px(500.)), cx);
		let root = cx
			.open_window(
				WindowOptions {
					titlebar: Some(TitlebarOptions {
						title: Some("Activity island frame qualification".into()),
						..Default::default()
					}),
					window_bounds: Some(WindowBounds::Windowed(bounds)),
					..ui_frame::window_options()
				},
				|window, cx| {
					cx.new(|cx| {
						ui_frame::observe_activation(window, cx);

						let island = cx.new(PerformanceIsland::new);

						island.update(cx, |island, cx| {
							island.set_available_width(860., cx);
							island.set_work(Some(self::work(false)), cx);
						});

						Fixture(island)
					})
				},
			)
			.expect("open isolated island fixture");
		let island = root.update(cx, |root, _, _| root.0.clone()).expect("fixture activity");

		root.update(cx, |_, window, cx| {
			eprintln!(
				"island-probe: display interval {:?}, scale {}",
				window.refresh_interval(cx),
				window.scale_factor()
			);
		})
		.expect("read fixture display");
		cx.activate(true);

		cx.spawn(async move |cx| {
			cx.background_executor().timer(Duration::from_secs(1)).await;
			cx.update(verify_viewports);

			for step in 0..20 {
				let debug = (step / 2) % 2 == 0;
				let open = step % 2 == 0;

				island.update(cx, |island, cx| {
					if step == 8 {
						island.set_work(Some(self::work(true)), cx);
					}

					island.visual_expansion(debug, open, cx);

					frame_trace::surface_render(if open {
						"island-opening"
					} else {
						"island-closing"
					});
				});
				cx.background_executor().timer(Duration::from_millis(600)).await;
			}

			cx.background_executor().timer(Duration::from_millis(1_500)).await;
			cx.update(|cx| cx.quit());
		})
		.detach();
	});
}

fn verify_viewports(cx: &mut gpui::App) {
	for handle in cx.windows() {
		handle
			.update(cx, |_, window, _| {
				let rendering = crate::ui_theme::native_material_panel::observed_rendering(window)
					.expect("native display geometry");

				assert_eq!(
					rendering["gpui_viewport"], rendering["view_bounds"],
					"native foreground bounds must match the GPUI viewport"
				);
			})
			.expect("inspect isolated native window");
	}
}

// NSArgumentDomain is process-local. Qualification never writes appearance preferences.
fn preferences() {
	let clear = env::var_os("DECODEX_VISUAL_ISLAND_CLEAR").is_some();

	unsafe {
		let defaults: Retained<AnyObject> = objc2::msg_send![
			AnyClass::get(c"NSUserDefaults").expect("Foundation"),
			standardUserDefaults
		];
		let values: Retained<AnyObject> =
			objc2::msg_send![AnyClass::get(c"NSMutableDictionary").expect("Foundation"), new];

		for (key, value) in [
			("debugMode", "YES"),
			("fullBackgroundRendering", "YES"),
			("popoverMaterial", "liquid-glass"),
			("DecodexPopoverGlassClear", if clear { "YES" } else { "NO" }),
		] {
			let key = NSString::from_str(key);
			let value = NSString::from_str(value);
			let _: () = objc2::msg_send![&*values, setObject: &*value, forKey: &*key];
		}

		let domain = NSString::from_str("NSArgumentDomain");
		let _: () = objc2::msg_send![&*defaults, setVolatileDomain: &*values, forName: &*domain];
	}
}

fn work(working: bool) -> Summary {
	Summary {
		samples: vec![Sample { tokens: Some(24_434), ..Default::default() }; 96],
		working: if working { 4 } else { 0 },
		live: if working {
			(0..4)
				.map(|index| Live {
					key: format!("fixture-{index}"),
					agent: format!("A{}", index + 1),
					action: "Reviewing rendering costs".into(),
					sample: Sample::default(),
				})
				.collect()
		} else {
			Vec::new()
		},
	}
}
