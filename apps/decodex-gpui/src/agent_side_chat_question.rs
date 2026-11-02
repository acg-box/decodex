//! Source-bound question actions open the existing native Side chat flow.
use std::rc::Rc;

use gpui::{
	AnyElement, AppContext as _, InteractiveElement as _, IntoElement as _, KeyDownEvent,
	ParentElement as _, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};

use crate::{
	shell::agent_surface::{AgentSurface, Context, selectable_text::AskSelection},
	ui_theme,
};

impl AgentSurface {
	pub(in crate::shell::agent_surface) fn side_chat_selection_action(
		&self,
		cx: &mut Context<Self>,
	) -> Option<AskSelection> {
		if !self.can_start_side_chat() {
			return None;
		}
		let work = self.selected.clone()?;
		let source = self.snapshot.as_ref()?.runtime_source.clone();
		let thread = self.conversation_record(&work)?.codex_thread_id.clone()?;
		let owner = cx.entity().downgrade();
		Some(Rc::new(move |reference, _, cx| {
			let _ = owner.update(cx, |s, cx| {
				if s.selected.as_ref() != Some(&work)
					|| s.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.as_ref())
						!= source.as_ref()
					|| s.conversation_record(&work).and_then(|work| work.codex_thread_id.as_ref())
						!= Some(&thread)
				{
					return;
				}
				s.start_side_chat(Some(reference), cx);
			});
		}))
	}

	pub(in crate::shell::agent_surface) fn side_chat_response_action(
		&self,
		text: &str,
		identity: &str,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let ask = self.side_chat_selection_action(cx)?;
		let text = text.to_owned();
		let key = format!("ask-response-{identity}");
		let group = format!("history-response-{identity}");
		Some(
			question_button(&key, move |window, cx| ask(text.clone(), window, cx))
				.opacity(0.)
				.group_hover(SharedString::from(group), |style| style.opacity(1.))
				.focus_visible(|style| style.opacity(1.))
				.into_any_element(),
		)
	}

	pub(in crate::shell::agent_surface) fn stage_side_chat_reference(
		&mut self,
		work: &str,
		reference: &str,
		cx: &mut Context<Self>,
	) {
		if self.selected.as_deref() != Some(work) || self.composer_manager.as_deref() != Some(work)
		{
			return;
		}
		let text = reference_draft(reference);
		if self.composer.update(cx, |input, cx| input.append_recovered_text(&text, cx)) {
			self.save_draft_document(cx);
		} else {
			self.feedback = crate::ui_feedback::Feedback::error(
				"Could not add the passage. Your current draft is kept.",
			);
		}
		#[cfg(all(target_os = "macos", not(test)))]
		self.focus_native_message(cx);
		cx.notify();
	}
}

fn reference_draft(reference: &str) -> String {
	let mut text = String::from("About this passage:\n\n");
	for line in reference.lines() {
		text.push_str("> ");
		text.push_str(line);
		text.push('\n');
	}
	// Keep room for the question within the ordinary composer limit.
	let limit = crate::composer_input::MAX_COMPOSER_BYTES / 2;
	if text.len() > limit {
		text.truncate(text.floor_char_boundary(limit));
		text.push_str("\n> … (excerpt)\n");
	}
	text.push('\n');
	text
}

fn question_button(
	key: &str,
	action: impl Fn(&mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
	let action = Rc::new(action);
	let keyboard = action.clone();
	let selector = key.to_owned();
	gpui::div().id(SharedString::from(key.to_owned())).debug_selector(move || selector.clone())
		.role(Role::Button).aria_label("Ask in side chat").tab_index(0)
		.size(gpui::px(24.)).flex().items_center().justify_center().rounded_md()
		.cursor_pointer().text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
		.hover(|style| style.bg(ui_theme::rgba(ui_theme::HOVER_FILL)).text_color(ui_theme::rgb(ui_theme::TEXT)))
		.tooltip(|_, cx| cx.new(|_| crate::shell::agent_surface::composer::ComposerTip("Ask in side chat".into())).into())
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
#[path = "agent_side_chat_question_tests.rs"]
mod tests;
