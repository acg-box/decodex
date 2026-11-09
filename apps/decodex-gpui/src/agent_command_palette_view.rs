//! Compact search presentation with one input, grouped results, and contextual key hints.
use super::{
	super::icons, AcceptResult, Action, AgentSurface, Command, CommandKey, Context, EraseSearch,
	ExitPalette, Group, NextResult, PreviousResult,
};
use crate::{
	shell::workspace_symbols::{self, Symbol},
	ui_motion::{self, HoverOverflowLabel},
	ui_theme,
};
use gpui::{AnyElement, Focusable as _, FontWeight, Role, SharedString, Window, prelude::*};

const ROW_HEIGHT: f32 = ui_theme::CONTROL_SIZE + ui_theme::CONTROL_INSET;
const HEADER_HEIGHT: f32 = ui_theme::CONTROL_SIZE + ui_theme::CONTROL_MARGIN * 2.;
const GROUP_HEIGHT: f32 = ui_theme::CHROME_CONTROL_SIZE;

impl AgentSurface {
	#[cfg(feature = "visual-capture")]
	#[allow(dead_code, reason = "The dedicated capture binary calls this shared view.")]
	pub(crate) fn visual_command_content(
		&self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		ui_motion::menu_surface(gpui::div())
			.font_family(ui_theme::FONT_FAMILY)
			.text_size(gpui::px(ui_theme::BODY_SIZE))
			.line_height(gpui::px(ui_theme::BODY_LINE_HEIGHT))
			.child(self.command_palette_content(360., window, cx))
			.into_any_element()
	}

	pub(in crate::shell::agent_surface) fn render_global_commands(
		&self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let owner = cx.entity().downgrade();
		let dismiss = owner.clone();
		let focus = self.workspace.navigation.search.as_ref().map(|input| input.focus_handle(cx));
		let list_limit = (f32::from(window.viewport_size().height)
			- 64.
			- HEADER_HEIGHT
			- ui_theme::CONTROL_SIZE
			- ui_theme::POPOVER_GAP * 2.)
			.clamp(88., 360.);
		gpui::div()
			.absolute()
			.top(gpui::px(64.))
			.left_0()
			.w_full()
			.flex()
			.justify_center()
			.child(
				gpui::div().w(gpui::px(560.)).max_w_full().child(
					ui_motion::popover_lazy(
						"global-command-palette",
						self.workspace.navigation.command_open,
						move |window, cx| {
							owner
								.update(cx, |s, cx| {
									s.command_palette_content(list_limit, window, cx)
								})
								.unwrap_or_else(|_| gpui::div().into_any_element())
						},
					)
					.focus(focus)
					.on_dismiss(move |cx| {
						let _ = dismiss.update(cx, |s, cx| {
							s.workspace.navigation.command_open = false;
							cx.notify();
						});
					}),
				),
			)
			.into_any_element()
	}

	fn command_palette_content(
		&self,
		list_limit: f32,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Some(search) = &self.workspace.navigation.search else {
			return gpui::div().into_any_element();
		};
		let commands = self.global_commands(cx);
		let selected =
			self.workspace.navigation.command_index.min(commands.len().saturating_sub(1));
		gpui::div()
			.id("global-command-dialog")
			.debug_selector(|| "global-command-dialog".into())
			.role(Role::Dialog)
			.aria_label("Search and commands")
			.w_full()
			.flex()
			.flex_col()
			.overflow_hidden()
			.text_color(ui_theme::rgb(ui_theme::TEXT))
			.key_context("GlobalCommands")
			.on_action(cx.listener(|s, _: &PreviousResult, window, cx| {
				s.command_key(CommandKey::Previous, window, cx)
			}))
			.on_action(cx.listener(|s, _: &NextResult, window, cx| {
				s.command_key(CommandKey::Next, window, cx)
			}))
			.on_action(cx.listener(|s, _: &AcceptResult, window, cx| {
				s.command_key(CommandKey::Accept, window, cx)
			}))
			.on_action(cx.listener(|s, _: &crate::composer_input::SubmitComposer, window, cx| {
				s.command_key(CommandKey::Accept, window, cx)
			}))
			.on_action(cx.listener(|s, _: &ExitPalette, window, cx| {
				s.command_key(CommandKey::Exit, window, cx)
			}))
			.on_action(cx.listener(|s, _: &EraseSearch, window, cx| {
				s.command_key(CommandKey::Erase, window, cx)
			}))
			.child(self.command_search_header(search, cx))
			.child(self.command_result_list(&commands, selected, list_limit, window, cx))
			.when(!commands.is_empty(), |dialog| dialog.child(command_footer(&commands[selected])))
			.into_any_element()
	}

	fn command_search_header(
		&self,
		search: &gpui::Entity<crate::composer_input::ComposerInput>,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let choosing = self.workspace.navigation.choose_project;
		gpui::div()
			.h(gpui::px(HEADER_HEIGHT))
			.flex_none()
			.px(gpui::px(12.))
			.flex()
			.items_center()
			.gap(gpui::px(8.))
			.border_b(gpui::px(0.5))
			.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER))
			.child(
				gpui::div()
					.id("command-back")
					.size(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
					.when(choosing, |icon| {
						icon.role(Role::Button)
							.aria_label("Back to all commands")
							.cursor_pointer()
							.hover(|s| s.text_color(ui_theme::rgb(ui_theme::TEXT)))
							.on_click(cx.listener(|s, _, _, cx| {
								s.reset_command_search(false, cx);
								cx.stop_propagation();
								cx.notify();
							}))
					})
					.child(icons::Icon {
						glyph: if choosing { icons::Glyph::Back } else { icons::Glyph::Search },
						size: ui_theme::NAVIGATION_ICON_SIZE,
					}),
			)
			.child(gpui::div().flex_1().min_w_0().h_full().child(search.clone()))
			.child(
				gpui::div()
					.id("command-close")
					.role(Role::Button)
					.aria_label("Close search")
					.cursor_pointer()
					.child(keycap("esc"))
					.on_click(cx.listener(|s, _, _, cx| {
						s.workspace.navigation.command_open = false;
						cx.stop_propagation();
						cx.notify();
					})),
			)
			.into_any_element()
	}

	fn command_result_list(
		&self,
		commands: &[Command],
		selected: usize,
		list_limit: f32,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let query = self.command_query(cx);
		let mut previous = None;
		let mut extent = ui_theme::POPOVER_CONTENT_PADDING * 2.;
		let mut fitting_height = 64.;
		let mut rows =
			gpui::div().w_full().flex().flex_col().py(gpui::px(ui_theme::POPOVER_CONTENT_PADDING));
		for (index, command) in commands.iter().enumerate() {
			let group = command.group();
			if previous != Some(group) {
				extent += GROUP_HEIGHT;
				previous = Some(group);
				let label = match group {
					Group::Conversations if query.is_empty() => "Recent",
					Group::Conversations => "Conversations",
					Group::Projects => "Projects",
					Group::Work => "Work",
					Group::Commands => "Commands",
				};
				rows = rows.child(
					gpui::div()
						.h(gpui::px(GROUP_HEIGHT))
						.flex_none()
						.px(gpui::px(12.))
						.flex()
						.items_center()
						.text_size(gpui::px(ui_theme::CAPTION_SIZE))
						.font_weight(FontWeight::MEDIUM)
						.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
						.child(label),
				);
			}
			rows = rows.child(self.command_result(command, index, selected == index, cx));
			extent += ROW_HEIGHT;
			if extent <= list_limit {
				fitting_height = extent;
			}
		}
		let height = ui_motion::value(
			SharedString::from(format!(
				"command-height-{}",
				self.workspace.navigation.command_session
			)),
			fitting_height,
			window,
			cx,
		);
		if commands.is_empty() {
			rows = rows.child(
				gpui::div()
					.h(gpui::px(52.))
					.flex_none()
					.px(gpui::px(16.))
					.flex()
					.items_center()
					.gap(gpui::px(10.))
					.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
					.child(icons::Icon { glyph: icons::Glyph::Search, size: 16. })
					.child(
						gpui::div().text_size(gpui::px(ui_theme::BODY_SIZE)).child("No results"),
					),
			);
		}
		rows.id("global-command-results")
			.debug_selector(|| "global-command-results".into())
			.h(gpui::px(height))
			.flex_none()
			.track_scroll(&self.workspace.navigation.command_scroll)
			.overflow_y_scroll()
			.into_any_element()
	}

	fn command_result(
		&self,
		command: &Command,
		index: usize,
		selected: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let detail = match &command.action {
			Action::Conversation(item) =>
				if item.archived {
					format!("{} (Archived)", item.owner)
				} else {
					item.owner.clone()
				},
			_ => String::new(),
		};
		gpui::div()
			.id(SharedString::from(format!("global-command-result-{index}")))
			.debug_selector(move || format!("global-command-result-{index}"))
			.role(Role::Button)
			.aria_label(format!("{} ({})", command.label, command.detail))
			.aria_selected(selected)
			.h(gpui::px(ROW_HEIGHT))
			.flex_none()
			.mx(gpui::px(ui_theme::POPOVER_CONTENT_PADDING))
			.px(gpui::px(ui_theme::CONTROL_MARGIN))
			.rounded(gpui::px(ui_theme::SMALL_CONTROL_RADIUS))
			.bg(ui_theme::rgb(ui_theme::TEXT).opacity(if selected { 0.07 } else { 0. }))
			.flex()
			.items_center()
			.gap(gpui::px(10.))
			.cursor_pointer()
			.child(command_icon(command))
			.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.overflow_hidden()
					.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
					.child(HoverOverflowLabel {
						id: SharedString::from(format!("command-title-{}", command.identity()))
							.into(),
						text: command.label.clone().into(),
						active: selected,
					}),
			)
			.when(!detail.is_empty(), |row| {
				row.child(
					gpui::div()
						.max_w(gpui::px(140.))
						.min_w_0()
						.overflow_hidden()
						.text_size(gpui::px(ui_theme::CAPTION_SIZE))
						.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
						.child(HoverOverflowLabel {
							id: SharedString::from(format!(
								"command-detail-{}",
								command.identity()
							))
							.into(),
							text: detail.into(),
							active: selected,
						}),
				)
			})
			.on_hover(cx.listener(move |s, hovered: &bool, _, cx| {
				if *hovered && s.workspace.navigation.command_index != index {
					s.workspace.navigation.command_index = index;
					cx.notify();
				}
			}))
			.on_click(cx.listener(move |s, _, window, cx| {
				s.run_global_command(index, window, cx);
				cx.stop_propagation();
			}))
			.into_any_element()
	}
}

fn command_icon(command: &Command) -> AnyElement {
	let colors = ui_theme::ColorScheme::current();
	let mut ink = colors.rgb(ui_theme::TEXT_MUTED);
	let icon = match &command.action {
		Action::Conversation(item) if item.archived =>
			icons::Icon { glyph: icons::Glyph::Archive, size: 14. }.into_any_element(),
		Action::Conversation(item) if item.assistant =>
			icons::Icon { glyph: icons::Glyph::Assistant, size: 14. }.into_any_element(),
		Action::Conversation(_) | Action::ChooseProject =>
			workspace_symbols::icon_sized(Symbol::TeamMembers, 14.),
		Action::Project(id) | Action::NewAgent(id) => {
			ink = colors.navigation_project(id).ink;
			icons::Icon { glyph: icons::Glyph::Folder, size: 14. }.into_any_element()
		},
		Action::NewAssistant =>
			icons::Icon { glyph: icons::Glyph::Assistant, size: 14. }.into_any_element(),
		Action::NewProject =>
			icons::Icon { glyph: icons::Glyph::Folder, size: 14. }.into_any_element(),
		Action::Archives =>
			icons::Icon { glyph: icons::Glyph::Archive, size: 14. }.into_any_element(),
		Action::Settings => workspace_symbols::icon_sized(Symbol::Settings, 14.),
		Action::Work(_, _) => workspace_symbols::icon_sized(Symbol::WorkActivity, 14.),
	};
	gpui::div()
		.size(gpui::px(20.))
		.flex_none()
		.flex()
		.items_center()
		.justify_center()
		.text_color(ink)
		.child(icon)
		.into_any_element()
}

fn command_footer(command: &Command) -> AnyElement {
	let path = matches!(command.action, Action::Project(_) | Action::NewAgent(_))
		.then(|| command.detail.clone());
	gpui::div()
		.h(gpui::px(ui_theme::CONTROL_SIZE))
		.flex_none()
		.px(gpui::px(12.))
		.border_t(gpui::px(0.5))
		.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER))
		.flex()
		.items_center()
		.gap(gpui::px(12.))
		.text_size(gpui::px(ui_theme::CAPTION_SIZE))
		.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
		.child(gpui::div().flex_1().min_w_0().overflow_hidden().child(if let Some(path) = path {
			HoverOverflowLabel {
				id: SharedString::from(format!("command-path-{}", command.identity())).into(),
				text: path.into(),
				active: true,
			}
			.into_any_element()
		} else {
			gpui::div()
				.flex()
				.items_center()
				.gap(gpui::px(4.))
				.child(keycap("↑"))
				.child(keycap("↓"))
				.child(gpui::div().ml(gpui::px(2.)).child("Navigate"))
				.into_any_element()
		}))
		.child(
			gpui::div()
				.flex_none()
				.flex()
				.items_center()
				.gap(gpui::px(6.))
				.child(command.verb())
				.child(keycap("↵")),
		)
		.into_any_element()
}

fn keycap(label: &'static str) -> AnyElement {
	gpui::div()
		.min_w(gpui::px(18.))
		.h(gpui::px(18.))
		.px(gpui::px(4.))
		.flex()
		.items_center()
		.justify_center()
		.rounded(gpui::px(4.))
		.border(gpui::px(0.5))
		.border_color(ui_theme::rgb(ui_theme::TEXT).opacity(0.1))
		.text_size(gpui::px(ui_theme::CAPTION_SIZE))
		.line_height(gpui::px(14.))
		.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
		.child(label)
		.into_any_element()
}
