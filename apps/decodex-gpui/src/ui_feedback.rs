//! Operation feedback carries its meaning from the source to one compact presentation.
use std::ops::Deref;

use gpui::{
	self, Div, ElementId, InteractiveElement as _, ParentElement as _, Role, Stateful,
	StatefulInteractiveElement as _, Styled as _,
};

use crate::ui_theme::{AMBER, GREEN, TEXT_MUTED};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Tone {
	#[default]
	Neutral,
	Success,
	Warning,
	Error,
}

impl Tone {
	pub(crate) fn color(self) -> u32 {
		match self {
			Self::Neutral => TEXT_MUTED,
			Self::Success => GREEN,
			Self::Warning => AMBER,
			Self::Error => 0xf28b82,
		}
	}

	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Neutral => "Status",
			Self::Success => "Success",
			Self::Warning => "Attention",
			Self::Error => "Error",
		}
	}

	pub(crate) fn icon(self) -> impl gpui::IntoElement {
		let data: &'static [u8] = match self {
			Self::Neutral => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round"><circle cx="8" cy="8" r="6"/><path d="M8 7v4M8 4.7v.1"/></g></svg>"#,
			Self::Success => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="6"/><path d="m5 8 2 2 4-4"/></g></svg>"#,
			Self::Warning => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><path d="M7 2.7a1.15 1.15 0 0 1 2 0l5 8.9a1.15 1.15 0 0 1-1 1.7H3a1.15 1.15 0 0 1-1-1.7Z"/><path d="M8 6v3M8 11v.1"/></g></svg>"#,
			Self::Error => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round"><circle cx="8" cy="8" r="6"/><path d="M8 4.7v4M8 11v.1"/></g></svg>"#,
		};
		gpui::svg().data(data).size(gpui::px(14.)).flex_none().text_color(gpui::rgb(self.color()))
	}

	pub(crate) fn row(self, id: impl Into<ElementId>, message: &str) -> Stateful<Div> {
		gpui::div()
			.id(id)
			.role(Role::Status)
			.aria_label(format!("{}: {message}", self.label()))
			.flex()
			.items_start()
			.gap(gpui::px(7.))
			.text_size(gpui::px(12.))
			.line_height(gpui::px(18.))
			.text_color(gpui::rgb(self.color()))
			.child(gpui::div().mt(gpui::px(2.)).flex_none().child(self.icon()))
			.child(gpui::div().min_w_0().child(message.to_owned()))
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Feedback {
	pub message: String,
	pub tone: Tone,
}

impl Feedback {
	pub(crate) fn neutral(message: impl Into<String>) -> Self {
		Self { message: message.into(), tone: Tone::Neutral }
	}

	pub(crate) fn success(message: impl Into<String>) -> Self {
		Self { message: message.into(), tone: Tone::Success }
	}

	pub(crate) fn warning(message: impl Into<String>) -> Self {
		Self { message: message.into(), tone: Tone::Warning }
	}

	pub(crate) fn error(message: impl Into<String>) -> Self {
		Self { message: message.into(), tone: Tone::Error }
	}

	pub(crate) fn clear(&mut self) {
		*self = Self::default();
	}
}

impl Deref for Feedback {
	type Target = str;

	fn deref(&self) -> &str {
		&self.message
	}
}
