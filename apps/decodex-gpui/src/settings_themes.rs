//! Theme previews share the Settings page's single scroll area.
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
	ui_theme::{
		self, ACCENT, CANVAS, COMPOSER_MATERIAL, CONTENT_MATERIAL, ColorScheme, ColorTheme,
		HOVER_FILL, MESSAGE_MATERIAL, PRESSED_FILL, PRIMARY_FILL, SELECTED_BORDER, SELECTED_FILL,
		SIDEBAR_MATERIAL, TEXT, TEXT_MUTED,
	},
};

impl SettingsSurface {
	pub(super) fn theme_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.id("color-theme-picker")
			.debug_selector(|| "color-theme-picker".into())
			.role(Role::RadioGroup)
			.aria_label("Color theme")
			.w_full()
			.flex()
			.flex_wrap()
			.gap_y(gpui::px(8.))
			.children(ColorTheme::ALL.into_iter().map(|theme| self.theme_option(theme, cx)))
	}

	fn theme_option(&self, theme: ColorTheme, cx: &mut Context<Self>) -> Stateful<Div> {
		let selected = theme == ColorScheme::current().color_theme();

		gpui::div()
			.id(SharedString::from(format!("color-theme-{}", theme.id())))
			.debug_selector(move || theme.id().to_owned())
			.role(Role::RadioButton)
			.aria_label(format!("{} color theme", theme.name()))
			.aria_selected(selected)
			.aria_toggled(if selected { Toggled::True } else { Toggled::False })
			.tab_index(0)
			.w(gpui::relative(0.2))
			.min_w(gpui::px(96.))
			.flex_none()
			.px(gpui::px(4.))
			.py(gpui::px(2.))
			.flex()
			.flex_col()
			.gap(gpui::px(4.))
			.rounded(gpui::px(8.))
			.border_1()
			.border_color(gpui::transparent_black())
			.text_size(gpui::px(11.5))
			.line_height(gpui::px(15.))
			.text_color(ui_theme::rgb(TEXT))
			.cursor_pointer()
			.hover(|row| row.bg(ui_theme::rgba(HOVER_FILL)))
			.active(|row| row.bg(ui_theme::rgba(PRESSED_FILL)))
			.focus_visible(|row| row.border_color(ui_theme::rgb(ACCENT)))
			.child(
				gpui::div()
					.w_full()
					.h(gpui::px(36.))
					.flex_none()
					.rounded(gpui::px(6.))
					.overflow_hidden()
					.border_1()
					.border_color(ui_theme::rgba(if selected {
						SELECTED_BORDER
					} else {
						0xffffff20
					}))
					.child(preview(theme)),
			)
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap(gpui::px(4.))
					.min_w_0()
					.px(gpui::px(3.))
					.font_weight(FontWeight::MEDIUM)
					.child(gpui::div().min_w_0().truncate().child(theme.name()))
					.when(selected, |label| {
						label.child(
							gpui::div().flex_none().text_color(ui_theme::rgb(ACCENT)).child("✓"),
						)
					}),
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
	let colors = ColorScheme::new(ui_theme::appearance::Theme::current(), theme);

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
				gpui::fill(rect(0., 0., 34., 80.), colors.rgba(SIDEBAR_MATERIAL)).corner_radii(
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
				gpui::fill(rect(43., 60., 107., 15.), colors.rgba(COMPOSER_MATERIAL))
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		desktop_settings::DesktopSettingsController,
		settings_surface::SettingsCategory,
		ui_theme::appearance::{Appearance, Theme},
	};
	use gpui::{TestAppContext, VisualTestContext};

	fn assert_painted_card(
		visual: &mut VisualTestContext,
		id: &'static str,
		colors: ColorScheme,
		selected: bool,
	) {
		let bounds = visual.debug_bounds(id).expect("visible theme card");
		visual.update(|window, cx| {
			crate::ui_frame::draw_frame(window, cx);
			let quads = window.painted_quads();
			let card = bounds.scale(window.scale_factor());
			let label = gpui::point(bounds.center().x, bounds.bottom() - gpui::px(5.))
				.scale(window.scale_factor());
			assert!(!quads.iter().any(|q| q.bounds.contains(&label)
				&& q.background == ui_theme::rgba(SELECTED_FILL).into()));
			let outline = gpui::Hsla::from(ui_theme::rgba(SELECTED_BORDER));
			assert_eq!(
				quads
					.iter()
					.any(|q| card.contains(&q.bounds.center()) && q.border_color == outline),
				selected,
				"{id}"
			);
			for color in
				[colors.rgb(CANVAS), colors.rgba(SIDEBAR_MATERIAL), colors.rgba(COMPOSER_MATERIAL)]
			{
				let fill = color.into();
				assert!(
					quads.iter().any(|q| card.contains(&q.bounds.center()) && q.background == fill),
					"{id} must paint its own palette color {color:?}"
				);
			}
		});
	}

	#[gpui::test]
	fn theme_cards_repaint_selection_and_miniature_materials(cx: &mut TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, cx| {
			ColorTheme::Decodex.apply(cx);
			Appearance::Light.apply(cx);
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.category = SettingsCategory::Appearance;
			settings
		});
		visual.simulate_resize(gpui::size(gpui::px(820.), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_frame);
		let assert_cards = |visual: &mut VisualTestContext, appearance, selected| {
			for family in [ColorTheme::Decodex, ColorTheme::Dracula] {
				assert_painted_card(
					visual,
					family.id(),
					ColorScheme::new(appearance, family),
					family == selected,
				);
			}
		};
		assert_cards(visual, Theme::Light, ColorTheme::Decodex);
		let button = visual.debug_bounds(ColorTheme::Dracula.id()).unwrap();
		visual.simulate_click(button.center(), Default::default());
		visual.simulate_mouse_move(Default::default(), None, Default::default());
		assert_cards(visual, Theme::Light, ColorTheme::Dracula);

		// GPUI’s test platform reports a light system appearance.
		for (appearance, expected) in
			[(Appearance::Dark, Theme::Dark), (Appearance::System, Theme::Light)]
		{
			visual.update(|_, cx| appearance.apply(cx));
			assert_cards(visual, expected, ColorTheme::Dracula);
		}
	}
}
