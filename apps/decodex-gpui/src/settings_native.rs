//! Show the existing source-bound native preferences in global Settings.
use gpui::{AnyElement, Context, Entity, IntoElement as _, ParentElement as _};

use super::SettingsSurface;
use crate::shell::agent_surface::AgentSurface;

impl SettingsSurface {
	pub(crate) fn bind_native_agent(
		&mut self,
		agent: Entity<AgentSurface>,
		cx: &mut Context<Self>,
	) {
		cx.observe(&agent, |_, _, cx| cx.notify()).detach();
		self.native_agent = Some(agent);
	}

	pub(super) fn native_defaults(&self, cx: &mut Context<Self>) -> AnyElement {
		self.native_agent.as_ref().map_or_else(
			|| {
				gpui::div()
					.child("Select a connected Agent to read native defaults.")
					.into_any_element()
			},
			|agent| agent.update(cx, |agent, cx| agent.native_defaults_settings(cx)),
		)
	}
}
