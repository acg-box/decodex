use std::time::Duration;

use decodex_gpui_spike::WorkspaceSpike;
use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;

fn main() {
	application().run(|cx: &mut App| {
		decodex_gpui_spike::bind_keys(cx);
		let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);
		let window = cx
			.open_window(
				WindowOptions {
					titlebar: Some(gpui::TitlebarOptions {
						title: Some("Decodex GPUI Feasibility".into()),
						..Default::default()
					}),
					window_bounds: Some(WindowBounds::Windowed(bounds)),
					focus: false,
					show: false,
					..Default::default()
				},
				|window, cx| cx.new(|cx| WorkspaceSpike::new(window, cx)),
			)
			.expect("open GPUI spike window");
		window
			.update(cx, |_, window, _| window.activate_window())
			.expect("activate GPUI spike after accessibility adapter installation");
		cx.activate(true);

		if let Ok(milliseconds) =
			std::env::var("DECODEX_GPUI_SPIKE_AUTO_QUIT_MS").unwrap_or_default().parse::<u64>()
		{
			cx.spawn(async move |cx| {
				cx.background_executor().timer(Duration::from_millis(milliseconds)).await;
				cx.update(|cx| cx.quit());
			})
			.detach();
		}
	});
}
