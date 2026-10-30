//! Presentation filters over the status labels already shown by each list.
use std::collections::BTreeSet;

use gpui::{Div, ParentElement as _, Styled as _};

use crate::{
	shell::agent_surface::{AgentSurface, Context, mcp_forms},
	ui_preferences,
};

#[derive(Clone, Copy)]
pub(super) enum View {
	Conversations,
	Work,
}
impl View {
	fn key(self) -> &'static str {
		match self {
			Self::Conversations => "DecodexConversationListStatus",
			Self::Work => "DecodexWorkListStatus",
		}
	}
}

pub(super) struct Filters {
	conversations: Option<String>,
	work: Option<String>,
}
impl Default for Filters {
	fn default() -> Self {
		Self { conversations: saved(View::Conversations), work: saved(View::Work) }
	}
}
impl Filters {
	pub(super) fn selected(&self, view: View) -> Option<&str> {
		match view {
			View::Conversations => self.conversations.as_deref(),
			View::Work => self.work.as_deref(),
		}
	}

	pub(super) fn matches(&self, view: View, status: &str) -> bool {
		self.selected(view).is_none_or(|selected| selected == status)
	}

	fn select(&mut self, view: View, status: Option<String>) {
		ui_preferences::string(view.key(), Some(status.as_deref().unwrap_or("")));
		match view {
			View::Conversations => self.conversations = status,
			View::Work => self.work = status,
		}
	}
}

fn saved(view: View) -> Option<String> {
	ui_preferences::string(view.key(), None).filter(|value| !value.is_empty())
}

impl AgentSurface {
	pub(super) fn list_status_filters<'a>(
		&self,
		view: View,
		statuses: impl Iterator<Item = &'a str>,
		cx: &mut Context<Self>,
	) -> Div {
		let selected = self.workspace.list_status.selected(view);
		// Keep a saved selection visible even when no row currently has that status.
		let mut statuses = statuses.map(str::to_owned).collect::<BTreeSet<_>>();
		if let Some(selected) = selected {
			statuses.insert(selected.to_owned());
		}
		let mut row = gpui::div().flex().flex_wrap().flex_none().gap_1().text_size(gpui::px(11.));
		for status in std::iter::once(None).chain(statuses.into_iter().map(Some)) {
			let label = status.as_deref().unwrap_or("All statuses").to_owned();
			row = row.child(mcp_forms::mcp_button(
				format!("{}-{label}", view.key()),
				label,
				selected == status.as_deref(),
				cx,
				move |s, cx| {
					s.workspace.list_status.select(view, status.clone());
					cx.notify();
				},
			));
		}
		row
	}
}

#[cfg(test)]
#[path = "agent_list_status_tests.rs"]
mod tests;
