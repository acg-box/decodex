//! Inline theme previews retain comparison space without growing the settings page.
use gpui::{
	Bounds, Context, Corners, Div, FontWeight, IntoElement, KeyDownEvent, Role, SharedString,
	Stateful,
	accesskit::Toggled,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	settings_surface::SettingsSurface,
	ui_scroll::SmoothScrollArea,
	ui_theme::{
		self, ACCENT, CANVAS, COMPOSER_MATERIAL, CONTENT_MATERIAL, ColorScheme, ColorTheme,
		HOVER_FILL, MESSAGE_MATERIAL, PRESSED_FILL, PRIMARY_FILL, SELECTED_BORDER, SELECTED_FILL,
		SIDEBAR_MATERIAL, TEXT, TEXT_MUTED, appearance::Theme,
	},
};

impl SettingsSurface {
	pub(super) fn theme_description(&self) -> String {
		let saved = ColorTheme::configured();

		if Theme::current() == Theme::Light && saved != ColorTheme::Decodex {
			format!("Light appearance uses Decodex. Switch to dark to restore {}.", saved.name())
		} else {
			"Decodex supports light and dark. Other presets use dark appearance.".into()
		}
	}

	pub(super) fn theme_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div().w_full().h(gpui::px(192.)).child(
			gpui::div()
				.id("color-theme-picker")
				.debug_selector(|| "color-theme-picker".into())
				.role(Role::RadioGroup)
				.aria_label("Color theme")
				.size_full()
				.overflow_y_scroll()
				.pr(gpui::px(14.))
				.flex()
				.flex_col()
				.child(
					gpui::div().w_full().flex_none().flex().flex_wrap().gap(gpui::px(8.)).children(
						ColorTheme::ALL.into_iter().map(|theme| self.theme_option(theme, cx)),
					),
				)
				.smooth_scroll("color-theme-scroll-motion")
				.with_scrollbar("color-theme-scrollbar", "Scroll color themes"),
		)
	}

	fn theme_option(&self, theme: ColorTheme, cx: &mut Context<Self>) -> Stateful<Div> {
		let selected = theme == ColorScheme::current().color_theme();
		let id = format!("color-theme-{}", theme.id());

		gpui::div()
			.id(SharedString::from(id.clone()))
			.debug_selector(move || id.clone())
			.role(Role::RadioButton)
			.aria_label(format!("{} color theme", theme.name()))
			.aria_selected(selected)
			.aria_toggled(if selected { Toggled::True } else { Toggled::False })
			.tab_index(0)
			.w(gpui::px(128.))
			.flex_none()
			.min_w_0()
			.h(gpui::px(90.))
			.p(gpui::px(2.))
			.flex()
			.flex_col()
			.gap(gpui::px(4.))
			.rounded(gpui::px(8.))
			.border_1()
			.border_color(gpui::transparent_black())
			.text_size(gpui::px(11.5))
			.line_height(gpui::px(17.))
			.text_color(ui_theme::rgb(TEXT))
			.cursor_pointer()
			.hover(|row| row.bg(ui_theme::rgba(HOVER_FILL)))
			.active(|row| row.bg(ui_theme::rgba(PRESSED_FILL)))
			.focus_visible(|row| row.border_color(ui_theme::rgb(ACCENT)))
			.child(
				gpui::div()
					.relative()
					.w_full()
					.h(gpui::px(62.))
					.flex_none()
					.rounded(gpui::px(6.))
					.overflow_hidden()
					.border_1()
					.border_color(ui_theme::rgba(if selected {
						SELECTED_BORDER
					} else {
						0xffffff20
					}))
					.child(preview(theme))
					.when(selected, |preview| {
						preview.child(
							gpui::div()
								.absolute()
								.top(gpui::px(2.))
								.right(gpui::px(5.))
								.text_size(gpui::px(10.))
								.child("✓"),
						)
					}),
			)
			.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.px(gpui::px(3.))
					.font_weight(FontWeight::MEDIUM)
					.truncate()
					.child(theme.name()),
			)
			.on_click(cx.listener(move |_, _, _, cx| theme.select(cx)))
			.on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					theme.select(cx);
					cx.stop_propagation();
				}
			}))
	}
}

fn preview(theme: ColorTheme) -> impl IntoElement {
	let appearance = if theme == ColorTheme::Decodex { Theme::current() } else { Theme::Dark };
	let colors = ColorScheme::new(appearance, theme);

	gpui::canvas(
		|_, _, _| (),
		move |bounds, _, window, _| {
			let width = f32::from(bounds.size.width);
			let height = f32::from(bounds.size.height);
			let rect = |x, y, w, h| {
				Bounds::new(
					bounds.origin
						+ gpui::point(gpui::px(x * width / 160.), gpui::px(y * height / 80.)),
					gpui::size(gpui::px(w * width / 160.), gpui::px(h * height / 80.)),
				)
			};

			window.paint_quad(gpui::fill(bounds, colors.rgb(CANVAS)).corner_radii(gpui::px(5.)));
			window.paint_quad(
				gpui::fill(rect(0., 0., 34., 80.), colors.rgb(SIDEBAR_MATERIAL >> 8)).corner_radii(
					Corners {
						top_left: gpui::px(5.),
						bottom_left: gpui::px(5.),
						..Default::default()
					},
				),
			);
			window.paint_quad(
				gpui::fill(rect(34., 0., 126., 80.), colors.rgba(CONTENT_MATERIAL)).corner_radii(
					Corners {
						top_right: gpui::px(5.),
						bottom_right: gpui::px(5.),
						..Default::default()
					},
				),
			);
			window.paint_quad(
				gpui::fill(rect(5., 23., 24., 10.), colors.rgba(SELECTED_FILL))
					.corner_radii(gpui::px(2.)),
			);
			window.paint_quad(
				gpui::fill(rect(98., 20., 51., 13.), colors.rgba(MESSAGE_MATERIAL))
					.corner_radii(gpui::px(3.)),
			);
			window.paint_quad(
				gpui::fill(rect(43., 60., 107., 15.), colors.rgb(COMPOSER_MATERIAL >> 8))
					.corner_radii(gpui::px(4.)),
			);
			window.paint_quad(
				gpui::fill(rect(135., 64., 8., 8.), colors.rgb(PRIMARY_FILL))
					.corner_radii(gpui::px(3.)),
			);

			for (x, y, width, height, token) in [
				(9., 10., 18., 2., TEXT_MUTED),
				(9., 27., 16., 2., TEXT),
				(9., 40., 14., 2., TEXT_MUTED),
				(44., 9., 43., 2., TEXT),
				(104., 26., 34., 2., TEXT),
				(44., 40., 92., 2., TEXT),
				(44., 47., 66., 2., TEXT_MUTED),
				(48., 66., 47., 2., TEXT_MUTED),
			] {
				window.paint_quad(
					gpui::fill(rect(x, y, width, height), colors.rgb(token))
						.corner_radii(gpui::px(1.)),
				);
			}
		},
	)
	.size_full()
}
