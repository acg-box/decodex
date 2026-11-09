//! Display-backed workspace motion measurements without a live service or user drafts.
use std::{env, time::Duration};

use gpui::{
	AppContext as _, Bounds, Context, Entity, IntoElement, Render, TitlebarOptions, Window,
	WindowBounds, WindowOptions,
};
use objc2::{
	rc::Retained,
	runtime::{AnyClass, AnyObject},
};
use objc2_foundation::NSString;

use crate::{frame_trace, shell::agent_surface::AgentSurface, ui_frame, ui_theme};

struct ComposerWorkspace(Entity<AgentSurface>);
impl Render for ComposerWorkspace {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |surface, cx| surface.prepare_native_composer(true, window, cx));
		crate::ui_layout::panel(self.0.clone())
	}
}

/// Exercise the real AppKit composer across docked and floating panel transitions.
pub(super) fn run_composer() {
	assert!(env::var_os("DECODEX_FRAME_TRACE").is_some(), "explicit trace output");
	assert!(env::var_os("DECODEX_VISUAL_DRAFT_ROOT").is_some(), "isolated draft root");
	preferences();
	gpui_platform::application().run(|cx| {
		ui_theme::appearance::Appearance::Dark.apply(cx);
		frame_trace::start_for(cx, Duration::from_secs(12));
		let bounds = Bounds::centered(None, gpui::size(gpui::px(1200.), gpui::px(720.)), cx);
		let root = cx
			.open_window(
				WindowOptions {
					titlebar: Some(TitlebarOptions {
						title: Some("Composer resize qualification".into()),
						..Default::default()
					}),
					window_bounds: Some(WindowBounds::Windowed(bounds)),
					..ui_frame::window_options()
				},
				|window, cx| {
					ui_theme::window_material::configure(window);
					let surface = cx.new(|cx| {
						let mut surface = AgentSurface::new(cx);
						surface.visual_workspace_fixture(cx);
						surface
					});
					cx.new(|_| ComposerWorkspace(surface))
				},
			)
			.expect("open isolated native composer fixture");
		cx.activate(true);
		cx.spawn(async move |cx| {
			for width in [1200., 800., 480., 760., 1400., 800.] {
				cx.background_executor().timer(Duration::from_secs(1)).await;
				root.update(cx, |probe, window, cx| {
					window.resize(gpui::size(gpui::px(width), gpui::px(720.)));
					probe.0.update(cx, |surface, cx| surface.toggle_agent_tree(cx));
					cx.notify();
				})
				.expect("resize native composer fixture");
			}
			cx.background_executor().timer(Duration::from_secs(7)).await;
			cx.update(|cx| cx.quit());
		})
		.detach();
	});
}

pub(super) fn run() {
	assert!(env::var_os("DECODEX_FRAME_TRACE").is_some(), "explicit trace output");
	assert!(env::var_os("DECODEX_VISUAL_DRAFT_ROOT").is_some(), "isolated draft root");
	preferences();
	gpui_platform::application().run(|cx| {
		ui_theme::appearance::Appearance::Dark.apply(cx);
		frame_trace::start_for(cx, Duration::from_secs(12));
		let bounds = Bounds::centered(None, gpui::size(gpui::px(1200.), gpui::px(800.)), cx);
		let root = cx
			.open_window(
				WindowOptions {
					titlebar: Some(TitlebarOptions {
						title: Some("Workspace motion qualification".into()),
						..Default::default()
					}),
					window_bounds: Some(WindowBounds::Windowed(bounds)),
					..ui_frame::window_options()
				},
				|window, cx| {
					ui_theme::window_material::configure(window);
					cx.new(|cx| {
						let mut surface = AgentSurface::new(cx);
						surface.visual_workspace_fixture(cx);
						if !surface.workspace_panels()[2].0 {
							surface.toggle_agent_tree(cx);
						}
						surface
					})
				},
			)
			.expect("open isolated workspace fixture");
		cx.activate(true);
		cx.spawn(async move |cx| {
			cx.background_executor().timer(Duration::from_secs(1)).await;
			for step in 0..12 {
				root.update(cx, |surface, _, cx| {
					if !(4..8).contains(&step) {
						surface.toggle_workspace_sidebar(cx);
					}
					if step >= 4 {
						surface.toggle_agent_tree(cx);
					}
					frame_trace::surface_render(if step % 2 == 0 {
						"workspace-closing"
					} else {
						"workspace-opening"
					});
				})
				.expect("toggle fixture panel");
				let delay = if step >= 8 && step % 2 == 0 { 80 } else { 500 };
				cx.background_executor().timer(Duration::from_millis(delay)).await;
			}
			cx.background_executor().timer(Duration::from_secs(6)).await;
			cx.update(|cx| cx.quit());
		})
		.detach();
	});
}

// NSArgumentDomain is process-local and does not change the app's saved appearance.
fn preferences() {
	unsafe {
		let defaults: Retained<AnyObject> = objc2::msg_send![
			AnyClass::get(c"NSUserDefaults").expect("Foundation"),
			standardUserDefaults
		];
		let values: Retained<AnyObject> =
			objc2::msg_send![AnyClass::get(c"NSMutableDictionary").expect("Foundation"), new];
		for (key, value) in [
			("windowMaterial", "liquid-glass"),
			("DecodexGlassClear", "YES"),
			("fullBackgroundRendering", "YES"),
		] {
			let _: () = objc2::msg_send![&*values,
				setObject: &*NSString::from_str(value), forKey: &*NSString::from_str(key)
			];
		}
		let _: () = objc2::msg_send![&*defaults,
			setVolatileDomain: &*values, forName: &*NSString::from_str("NSArgumentDomain")
		];
	}
}
