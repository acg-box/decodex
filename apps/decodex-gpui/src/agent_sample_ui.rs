//! Direct question entry points share the existing one-shot sampler and popover host.
use std::{cell::Cell, rc::Rc};

use gpui::{
	Anchor, AnyElement, AppContext as _, Focusable as _, KeyDownEvent, Point, Role, SharedString,
	Window,
};

use super::*;
use crate::{shell::agent_surface::selectable_text::AskSelection, ui_motion};

impl AgentSurface {
	fn can_ask_separately(&self) -> bool {
		self.can_open_sample()
			&& self.running_turn().is_none()
			&& !self.voice_on_current_conversation()
			&& !(self.sample.suggestion && self.sample.busy())
	}

	pub(in crate::shell::agent_surface) fn sample_selection_action(
		&self,
		cx: &mut Context<Self>,
	) -> Option<AskSelection> {
		if !self.can_ask_separately() {
			return None;
		}
		let work = self.selected.clone()?;
		let source = self.snapshot.as_ref()?.runtime_source.clone();
		let thread = self
			.snapshot
			.as_ref()?
			.work_items
			.iter()
			.find(|w| w.id == work)?
			.codex_thread_id
			.clone()?;
		let owner = cx.entity().downgrade();
		Some(Rc::new(move |reference, window, cx| {
			let _ = owner.update(cx, |s, cx| {
				let current = s
					.snapshot
					.as_ref()
					.and_then(|snapshot| snapshot.work_items.iter().find(|w| w.id == work));
				if s.selected.as_ref() != Some(&work)
					|| s.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.as_ref())
						!= source.as_ref()
					|| current.and_then(|w| w.codex_thread_id.as_ref()) != Some(&thread)
					|| !s.can_ask_separately()
				{
					return;
				}
				let busy = s.sample.busy();
				s.open_sample(cx);
				if !busy {
					// Leave room for the question inside the native 4000-byte request limit.
					let end = reference.floor_char_boundary(
						(MAX_AGENT_SAMPLE_PROMPT_BYTES / 2).min(reference.len()),
					);
					s.sample.reference = Some(if end < reference.len() {
						format!("{}\n… (excerpt)", &reference[..end])
					} else {
						reference
					});
					s.sample.submitted_prompt = None;
					if let Some(input) = &s.sample.input {
						input.update(cx, |input, cx| input.set_content("", cx));
					}
				}
				s.sample.anchor = Some(window.mouse_position());
				if let Some(input) = &s.sample.input {
					window.focus(&input.focus_handle(cx), cx);
				}
				cx.notify();
			});
		}))
	}

	pub(in crate::shell::agent_surface) fn sample_response_action(
		&self,
		text: &str,
		identity: &str,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let ask = self.sample_selection_action(cx)?;
		let text = text.to_owned();
		let key = format!("ask-response-{identity}");
		let group = format!("history-response-{identity}");
		Some(
			question_button(&key, "Ask about this response", move |window, cx| {
				ask(text.clone(), window, cx);
			})
			.opacity(0.)
			.group_hover(SharedString::from(group), |style| style.opacity(1.))
			.focus_visible(|style| style.opacity(1.))
			.into_any_element(),
		)
	}

	pub(in crate::shell::agent_surface) fn sample_toolbar_action(
		&self,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		if !self.can_ask_separately() {
			return None;
		}
		let owner = cx.entity().downgrade();
		Some(
			question_button("ask-separately", "Ask separately", move |window, cx| {
				let _ = owner.update(cx, |s, cx| {
					if s.can_ask_separately() {
						s.dismiss_composer_menu(cx);
						s.open_sample(cx);
						s.sample.anchor = Some(window.mouse_position());
						if let Some(input) = &s.sample.input {
							window.focus(&input.focus_handle(cx), cx);
						}
						cx.notify();
					}
				});
			})
			.into_any_element(),
		)
	}

	pub(in crate::shell::agent_surface) fn sample_popover(
		&self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let visible = !self.sample.suggestion
			&& self.sample.work.is_some()
			&& self.sample.work == self.selected;
		let viewport = window.viewport_size();
		let position = self
			.sample
			.anchor
			.unwrap_or_else(|| Point::new(gpui::px(24.), viewport.height - gpui::px(100.)));
		let width = f32::from(viewport.width).clamp(240., 396.) - 16.;
		let max_height = (f32::from(viewport.height) - 32.).max(120.);
		let owner = cx.entity();
		let dismiss = owner.downgrade();
		let focus = self.sample.input.as_ref().map(|input| input.focus_handle(cx));
		gpui::deferred(
			gpui::anchored()
				.anchor(Anchor::BottomLeft)
				.position(position)
				.snap_to_window_with_margin(gpui::px(8.))
				.child(
					gpui::div().w(gpui::px(width)).child(
						ui_motion::popover_lazy(
							"separate-question-popover",
							visible,
							move |_, cx| {
								owner.update(cx, |s, cx| {
									gpui::div()
										.id("sample-popover")
										.debug_selector(|| "sample-popover".into())
										.max_h(gpui::px(max_height))
										.overflow_y_scroll()
										.on_mouse_down_out(cx.listener(|s, _, _, cx| {
											s.reset_sample();
											cx.notify();
										}))
										.on_key_down(cx.listener(
											|s, event: &KeyDownEvent, _, cx| {
												if event.keystroke.key == "escape" {
													s.reset_sample();
													cx.notify();
													cx.stop_propagation();
												}
											},
										))
										.child(s.sample_content(cx))
										.into_any_element()
								})
							},
						)
						.above()
						.focus(focus)
						.trigger(Rc::new(Cell::new(Some(gpui::Bounds::new(
							position,
							gpui::size(gpui::px(1.), gpui::px(1.)),
						)))))
						.on_dismiss(move |cx| {
							let _ = dismiss.update(cx, |s, cx| {
								if !s.sample.suggestion {
									s.reset_sample();
									cx.notify();
								}
							});
						}),
					),
				),
		)
		.with_priority(5)
		.into_any_element()
	}
}

fn question_button(
	key: &str,
	label: &'static str,
	action: impl Fn(&mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
	let action = Rc::new(action);
	let keyboard = action.clone();
	let selector = key.to_owned();
	gpui::div().id(SharedString::from(key.to_owned())).debug_selector(move || selector.clone())
		.role(Role::Button).aria_label(label).tab_index(0)
		.size(gpui::px(24.)).flex().items_center().justify_center().rounded_md()
		.cursor_pointer().text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
		.hover(|style| style.bg(ui_theme::rgba(ui_theme::HOVER_FILL)).text_color(ui_theme::rgb(ui_theme::TEXT)))
		.tooltip(move |_, cx| cx.new(|_| crate::shell::agent_surface::composer::ComposerTip(label.into())).into())
		.on_click(move |_, window, cx| { action(window, cx); cx.stop_propagation(); })
		.on_key_down(move |event: &KeyDownEvent, window, cx| {
			if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				keyboard(window, cx);
				cx.stop_propagation();
			}
		})
		.child(gpui::svg().data(br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path d="M13.5 7.3a5.5 5.5 0 0 1-5.5 5.3H6l-3.5 2v-4A5.1 5.1 0 0 1 2.5 7.3a5.5 5.5 0 0 1 11 0Z" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M6.4 5.8a1.6 1.6 0 1 1 2.4 1.4c-.6.3-.8.6-.8 1.2M8 10h.01" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>"#.as_slice()).size(gpui::px(16.)))
}

#[cfg(test)]
#[path = "agent_sample_ui_tests.rs"]
mod tests;
