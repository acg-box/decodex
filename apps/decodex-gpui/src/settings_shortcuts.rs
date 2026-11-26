//! Record real keys in a focus-scoped editor. Lookup never changes a binding.
use crate::{
	composer_input::{ComposerEvent, ComposerInput},
	shell::{
		shortcuts::{self, Shortcuts},
		workspace_symbols::{self, Symbol},
	},
	ui_motion::HoverHint as _,
	ui_scroll::SmoothScrollArea,
	ui_theme,
};
use gpui::{
	App, Context, Entity, FocusHandle, Focusable, KeyDownEvent, Keystroke, OperatingSystem, Render,
	Role, SharedString, Subscription, Window, div, prelude::*, px,
};

#[derive(Clone, Copy, PartialEq)]
enum Target {
	Lookup,
	Command(usize),
}

pub(super) struct ShortcutEditor {
	focus: FocusHandle,
	target: Option<Target>,
	candidate: Vec<String>,
	search: Entity<ComposerInput>,
	recorded: Option<String>,
	show_system: bool,
	focus_search: bool,
	message: Option<String>,
	_keys: Subscription,
}
impl ShortcutEditor {
	pub fn new(cx: &mut Context<Self>) -> Self {
		let focus = cx.focus_handle();
		let search = cx.new(|cx| {
			ComposerInput::search("Search commands or shortcuts", "Search shortcuts", cx)
		});
		cx.subscribe(&search, |s: &mut Self, input, event, cx| {
			if matches!(event, ComposerEvent::Changed) {
				if s.recorded.as_ref().is_some_and(|k| {
					shortcuts::display(k, cx.platform_info().os) != input.read(cx).content()
				}) {
					s.recorded = None;
				}
				s.cancel(cx);
			}
		})
		.detach();
		let owner = cx.entity().downgrade();
		let keys = cx.intercept_keystrokes(move |event, window, cx| {
			let _ = owner.update(cx, |s: &mut Self, cx| {
				if s.target.is_none()
					|| !s.focus.is_focused(window)
					|| !event.context_stack.iter().any(|c| c.contains("ShortcutRecorder"))
				{
					return;
				}
				cx.stop_propagation();
				window.prevent_default();
				let stroke = &event.keystroke;
				if stroke.is_modifier_key() {
					return;
				}
				if stroke.key == "escape"
					&& stroke.modifiers == Default::default()
					&& matches!(s.target, Some(Target::Command(_)))
				{
					s.cancel(cx);
				} else if stroke.key == "enter"
					&& stroke.modifiers == Default::default()
					&& !s.candidate.is_empty()
					&& let Some(Target::Command(i)) = s.target
				{
					s.save(i, cx);
				} else {
					s.record(stroke, window, cx);
				}
			});
		});
		Self {
			focus,
			target: None,
			candidate: vec![],
			search,
			recorded: None,
			show_system: false,
			focus_search: true,
			message: None,
			_keys: keys,
		}
	}

	fn cancel(&mut self, cx: &mut Context<Self>) {
		self.target = None;
		self.candidate.clear();
		self.message = None;
		cx.notify();
	}

	fn start(&mut self, target: Target, w: &mut Window, cx: &mut Context<Self>) {
		self.cancel(cx);
		self.target = Some(target);

		self.focus.focus(w, cx);
		cx.notify();
	}

	fn record(&mut self, k: &Keystroke, window: &mut Window, cx: &mut Context<Self>) {
		let key = k.unparse(cx.platform_info().os);
		self.message = None;
		match self.target {
			Some(Target::Lookup) => {
				self.search.update(cx, |input, cx| {
					input.set_content(&shortcuts::display(&key, cx.platform_info().os), cx)
				});
				self.recorded = Some(key);
				self.target = None;
				window.focus(&self.search.focus_handle(cx), cx);
				window.dispatch_action(Box::new(crate::composer_input::SelectAll), cx);
			},
			Some(Target::Command(i)) => {
				let map = cx.global::<Shortcuts>();
				if let Some(owner) =
					map.conflicts(i, &key).into_iter().find(|&j| !map.commands[j].editable)
				{
					self.message = Some(format!(
						"{} is reserved for {}.",
						shortcuts::display(&key, cx.platform_info().os),
						map.commands[owner].label
					));
					self.candidate.clear();
				} else if k.key.chars().count() == 1
					&& !k.modifiers.platform
					&& !k.modifiers.control
					&& !k.modifiers.alt
				{
					self.message =
						Some("Add a modifier so this shortcut works while typing.".into());
					self.candidate.clear();
				} else {
					self.candidate = vec![key];
				}
			},
			None => return,
		}
		cx.notify();
	}

	fn conflicts(&self, i: usize, cx: &App) -> Vec<usize> {
		let map = cx.global::<Shortcuts>();
		let mut conflicts =
			self.candidate.iter().flat_map(|k| map.conflicts(i, k)).collect::<Vec<_>>();
		conflicts.sort_unstable();
		conflicts.dedup();
		conflicts
	}

	fn save(&mut self, i: usize, cx: &mut Context<Self>) {
		shortcuts::assign(i, std::mem::take(&mut self.candidate), cx);
		self.cancel(cx);
	}

	fn button(
		&self,
		id: String,
		label: String,
		accessible: String,
		cx: &mut Context<Self>,
		action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
	) -> impl IntoElement {
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		ui_theme::settings_button(SharedString::from(id), SharedString::from(label))
			.aria_label(accessible)
			.on_click(cx.listener(move |s, _, w, cx| action(s, w, cx)))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, w, cx| {
				if !e.is_held && matches!(e.keystroke.key.as_str(), "space" | "enter") {
					cx.stop_propagation();
					keyboard(s, w, cx);
				}
			}))
	}

	fn row(&self, i: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
		let map = cx.global::<Shortcuts>();
		let cmd = &map.commands[i];
		let label = cmd.label.clone();
		let changed = map.changed(i);
		let keys = map
			.keys(i)
			.iter()
			.map(|k| shortcuts::display(k, cx.platform_info().os))
			.collect::<Vec<_>>()
			.join("  /  ");
		let editing = self.target == Some(Target::Command(i));
		let mut row = div()
			.id(("shortcut-row", i))
			.w_full()
			.h(px(32.))
			.px(px(8.))
			.flex()
			.items_center()
			.gap_2()
			.rounded(px(5.))
			.hover(|row| row.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.child(div().flex_1().min_w_0().child(label.clone()));
		if !cmd.editable {
			return row
				.child(div().text_color(ui_theme::rgb(ui_theme::TEXT_MUTED)).child(keys))
				.into_any_element();
		}
		if changed && !editing {
			row = row.child(self.button(
				format!("reset-shortcut-{i}"),
				"Reset".into(),
				format!("Reset {label}"),
				cx,
				move |s, w, cx| {
					let defaults = cx.global::<Shortcuts>().defaults(i).to_vec();
					s.start(Target::Command(i), w, cx);
					s.candidate = defaults;
					if s.conflicts(i, cx).is_empty() {
						s.save(i, cx);
					}
				},
			));
		}
		let chip = if editing && self.candidate.is_empty() {
			"Press shortcut".into()
		} else if editing {
			self.candidate
				.iter()
				.map(|k| shortcuts::display(k, cx.platform_info().os))
				.collect::<Vec<_>>()
				.join("  /  ")
		} else if keys.is_empty() {
			"Unassigned".into()
		} else {
			keys.clone()
		};
		row = row.child(self.button(
			format!("bind-shortcut-{i}"),
			chip,
			format!("Record shortcut for {label}"),
			cx,
			move |s, w, cx| s.start(Target::Command(i), w, cx),
		));
		let mut block = gpui::div().flex().flex_col().child(row);
		if editing {
			let conflicts = self.conflicts(i, cx);
			let owner_names = conflicts
				.iter()
				.map(|j| cx.global::<Shortcuts>().commands[*j].label.as_str())
				.collect::<Vec<_>>()
				.join(", ");
			let status = self.message.clone().unwrap_or_else(|| {
				if !conflicts.is_empty() {
					format!("Assigned to {owner_names}.")
				} else if self.candidate.is_empty() {
					"Press a shortcut.".into()
				} else {
					"Press Return to save.".into()
				}
			});
			let mut footer = div()
				.flex()
				.items_center()
				.gap_2()
				.px(px(8.))
				.py(px(4.))
				.child(div().flex_1().child(ui_theme::settings_description(status)));
			if self.candidate.is_empty() && self.message.is_none() && !keys.is_empty() {
				footer = footer.child(self.button(
					format!("remove-shortcut-{i}"),
					"Remove".into(),
					format!("Remove shortcut for {label}"),
					cx,
					move |s, _, cx| s.save(i, cx),
				));
			}
			footer = footer.child(self.button(
				format!("cancel-shortcut-{i}"),
				"Cancel".into(),
				"Cancel recording".into(),
				cx,
				|s, _, cx| s.cancel(cx),
			));
			if !self.candidate.is_empty() {
				let verb = if conflicts.is_empty() { "Save" } else { "Reassign" };
				footer = footer.child(self.button(
					format!("save-shortcut-{i}"),
					verb.into(),
					format!("{verb} shortcut"),
					cx,
					move |s, _, cx| s.save(i, cx),
				));
			}
			block = block.child(footer);
		}
		block.into_any_element()
	}
}

// A search accessory shares the text field's surface and keeps its own hit target.
fn search_accessory(
	id: &'static str,
	label: &'static str,
	icon: Symbol,
	size: f32,
	active: bool,
) -> gpui::Stateful<gpui::Div> {
	div()
		.id(id)
		.role(Role::Button)
		.aria_label(label)
		.tab_index(0)
		.size(px(25.))
		.flex_none()
		.flex()
		.items_center()
		.justify_center()
		.cursor_pointer()
		.rounded(px(4.))
		.text_color(ui_theme::rgb(if active { ui_theme::ACCENT } else { ui_theme::TEXT_MUTED }))
		.hover(|s| s.text_color(ui_theme::rgb(ui_theme::TEXT)))
		.focus_visible(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
		.hover_hint(label)
		.child(workspace_symbols::icon_sized(icon, size))
}
impl ShortcutEditor {
	fn search_bar(
		&self,
		lookup: bool,
		query: &str,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		let mut bar = div()
			.id("shortcuts-search-bar")
			.w_full()
			.h(px(34.))
			.flex_none()
			.flex()
			.items_center()
			.pl(px(10.))
			.pr(px(4.))
			.gap_1()
			.rounded(px(7.))
			.border_1()
			.border_color(
				ui_theme::rgb(if lookup { ui_theme::ACCENT } else { ui_theme::TEXT_MUTED })
					.opacity(if lookup { 0.6 } else { 0.28 }),
			)
			.bg(ui_theme::rgba(ui_theme::FIELD_MATERIAL))
			.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
			.child(workspace_symbols::icon_sized(Symbol::ActivitySearch, 14.))
			.child(div().flex_1().min_w_0().child(if lookup {
				div().child("Press a shortcut to find its command").into_any_element()
			} else {
				self.search.clone().into_any_element()
			}));
		if !query.is_empty() && !lookup {
			bar = bar.child(
				search_accessory("shortcut-clear", "Clear search", Symbol::Close, 10., false)
					.on_click(cx.listener(|s, _, w, cx| {
						s.recorded = None;
						s.search.update(cx, |input, cx| input.set_content("", cx));
						w.focus(&s.search.focus_handle(cx), cx);
					})),
			);
		}
		bar = bar.child(
			search_accessory(
				"shortcut-lookup",
				if lookup { "Stop recording" } else { "Record keys to find commands" },
				Symbol::Keyboard,
				16.,
				lookup,
			)
			.on_click(cx.listener(move |s, _, w, cx| {
				if lookup {
					s.cancel(cx);
					w.focus(&s.search.focus_handle(cx), cx);
				} else {
					s.start(Target::Lookup, w, cx)
				}
			})),
		);
		bar
	}
}
impl Render for ShortcutEditor {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		if !cx.has_global::<Shortcuts>() {
			shortcuts::initialize(cx, |_| {});
		}
		if self.focus_search {
			self.focus_search = false;
			if !window.last_input_was_keyboard() {
				window.focus(&self.search.focus_handle(cx), cx);
			}
		}
		let lookup = self.target == Some(Target::Lookup);
		let query = self.search.read(cx).content().to_owned();
		let bar = self.search_bar(lookup, &query, cx);
		let map = cx.global::<Shortcuts>();
		let mut visible =
			self.recorded.as_ref().map_or_else(|| map.search(&query), |k| map.owners(k));
		visible.sort_by(|a, b| map.commands[*a].label.cmp(&map.commands[*b].label));
		let (custom, system): (Vec<_>, Vec<_>) =
			visible.into_iter().partition(|i| map.commands[*i].editable);
		let mut list = div().flex().flex_col().w_full().pb(px(ui_theme::SETTINGS_INSET));
		if custom.is_empty() && system.is_empty() {
			list = list.child(ui_theme::settings_description("No matching shortcuts."));
		}
		if !custom.is_empty() {
			list = list
				.child(ui_theme::settings_section_title("App shortcuts"))
				.children(custom.into_iter().map(|i| self.row(i, cx)).collect::<Vec<_>>());
		}
		if !system.is_empty() {
			let system_title = match cx.platform_info().os {
				OperatingSystem::Mac => "macOS shortcuts (read-only)",
				OperatingSystem::Windows => "Windows shortcuts (read-only)",
				_ => "System shortcuts (read-only)",
			};
			let expanded = self.show_system || !query.trim().is_empty();
			list = list.child(
				div()
					.id("system-shortcuts-disclosure")
					.role(Role::Button)
					.aria_label(system_title)
					.aria_expanded(expanded)
					.cursor_pointer()
					.mt(px(10.))
					.h(px(30.))
					.flex()
					.items_center()
					.gap_2()
					.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
					.child(workspace_symbols::process_chevron("system-shortcuts-chevron", expanded))
					.child(system_title)
					.on_click(cx.listener(|s, _, _, cx| {
						s.show_system = !s.show_system;
						cx.notify();
					})),
			);
			if expanded {
				list =
					list.children(system.into_iter().map(|i| self.row(i, cx)).collect::<Vec<_>>());
			}
		}
		div()
			.id("shortcut-editor")
			.key_context("ShortcutRecorder")
			.track_focus(&self.focus)
			.size_full()
			.flex()
			.items_start()
			.justify_center()
			.px(px(ui_theme::SETTINGS_INSET))
			.pt(px(ui_theme::SETTINGS_TOP))
			.child(
				div()
					.w_full()
					.max_w(px(ui_theme::SETTINGS_WIDTH))
					.h_full()
					.min_h_0()
					.flex()
					.flex_col()
					.gap(px(12.))
					.child(ui_theme::settings_title("Shortcuts"))
					.child(bar)
					.child(
						div()
							.id("shortcut-results")
							.flex_1()
							.min_h_0()
							.overflow_y_scroll()
							.child(list)
							.smooth_scroll("shortcut-results-scroll")
							.with_scrollbar("shortcut-results-bar", "Scroll shortcuts"),
					),
			)
	}
}
