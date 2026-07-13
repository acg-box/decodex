use std::time::Duration;

use decodex_gpui_spike::{WorkspaceSpike, text_input};
use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;

fn main() {
	application().run(|cx: &mut App| {
		text_input::bind_keys(cx);
		let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);
		cx.open_window(
			WindowOptions {
				titlebar: Some(gpui::TitlebarOptions {
					title: Some("Decodex GPUI Feasibility".into()),
					..Default::default()
				}),
				window_bounds: Some(WindowBounds::Windowed(bounds)),
				..Default::default()
			},
			|window, cx| cx.new(|cx| WorkspaceSpike::new(window, cx)),
		)
		.expect("open GPUI spike window");
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
