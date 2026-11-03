//! Personal Assistant settings share the existing service command path.
use crate::shell::agent_surface::{AgentActionDto, AgentSurface, ComposerInput, EntityId};
use decodex_protocol::{AssistantCommand, AssistantSettings, RagflowConnection};
use gpui::{
	AnyElement, AppContext as _, Context, Entity, IntoElement as _, ParentElement as _, Styled as _,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Panel {
	owner: Option<String>,
	fields: BTreeMap<&'static str, Entity<ComposerInput>>,
	import: bool,
	error: String,
}

impl AgentSurface {
	pub(crate) fn assistant_settings_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
		let Some(state) = self.snapshot.as_ref().map(|s| (*s.assistant).clone()) else {
			return gpui::div().child("Connect to read Assistant settings.").into_any_element();
		};
		let Some(owner) = state.agent_id.clone() else {
			return gpui::div()
				.child("Choose a personal Assistant in Work before configuring RAGFlow.")
				.into_any_element();
		};
		let mut panel=gpui::div().w_full().flex().flex_col().gap_2().child("Personal Assistant").child("Give it continuing responsibilities in chat. It can create persistent Agents, arrange teams and use scoped Work collaboration. The local service must be running for scheduled duties.");
		if state.context_pending {
			panel = panel.child(
				"Context refresh is pending confirmation. You can continue in the current conversation.",
			);
		}
		if let Some(status) = &state.memory_status {
			panel = panel.child(status.clone());
		}
		if self.assistant_settings.owner.as_deref() == Some(&owner) {
			for (key, label) in [
				("mandate", "Continuing instructions"),
				("turns", "Refresh after completed turns (0 disables periodic refresh)"),
				("url", "RAGFlow URL (blank disables the connection)"),
				("credential", "API key environment variable in the service"),
				("user", "RAGFlow user ID"),
				("datasets", "Knowledge dataset IDs (comma separated; blank disables retrieval)"),
				("memory", "Memory ID (blank keeps Codex memory)"),
			] {
				if let Some(field) = self.assistant_settings.fields.get(key) {
					panel = panel.child(label).child(field.clone());
				}
			}
			panel=panel.child(self.workspace_action("assistant-import".into(),format!("Import completed chats ({})",if self.assistant_settings.import {"on"}else{"off"}),|s,cx|{s.assistant_settings.import = !s.assistant_settings.import;cx.notify();},cx))
				.child("Selecting a RAGFlow Memory disables Codex memory generation and use for this Assistant and each new context. Native compaction and history remain enabled. The change waits for an idle boundary. Chat import starts after you save; original history stays in Codex.")
				.child(self.workspace_action("assistant-save".into(),"Save Assistant settings".into(),|s,cx|s.save_assistant_settings(cx),cx))
				.child(self.workspace_action("assistant-cancel".into(),"Cancel edit".into(),|s,cx|{s.assistant_settings=Panel::default();cx.notify();},cx));
		} else {
			let memory =
				state.settings.ragflow.as_ref().and_then(|r| r.memory_id.as_ref()).is_some();
			panel = panel
				.child(format!(
					"Context refresh ({} completed turns). RAGFlow ({}). Memory policy ({} selected).",
					state.settings.renew_after_turns,
					if state.settings.ragflow.is_some() { "configured" } else { "off" },
					if memory { "RAGFlow" } else { "Codex settings" }
				))
				.child(self.workspace_action(
					"assistant-edit".into(),
					"Edit Assistant and RAGFlow settings".into(),
					|s, cx| s.edit_assistant_settings(cx),
					cx,
				));
		}
		if !self.assistant_settings.error.is_empty() {
			panel = panel.child(self.assistant_settings.error.clone());
		}
		for team in &state.teams {
			panel = panel.child(format!("{} ({} members)", team.name, team.members.len()));
			for member in &team.members {
				panel = panel.child(format!("{}: {}", member.agent_id, member.responsibility));
			}
		}
		for wake in state.wakes.iter().filter(|w| w.due_at_micros.is_some()) {
			let wake_id = wake.id.clone();
			let id = owner.clone();
			panel = panel.child(format!("{}: {}", wake.id, wake.instruction)).child(
				self.workspace_action(
					format!("assistant-cancel-wake-{}", wake.id),
					"Cancel this duty".into(),
					move |s, cx| {
						s.execute(
							AgentActionDto::ManageAssistant {
								agent_id: EntityId::new(id.clone()).expect("saved Agent"),
								command: AssistantCommand::CancelWake { wake_id: wake_id.clone() },
							},
							None,
							cx,
						)
					},
					cx,
				),
			);
		}
		panel.into_any_element()
	}

	fn edit_assistant_settings(&mut self, cx: &mut Context<Self>) {
		let Some(state) = self.snapshot.as_ref().map(|s| s.assistant.as_ref()) else {
			return;
		};
		let Some(owner) = state.agent_id.clone() else {
			return;
		};
		let r = state.settings.ragflow.as_ref();
		let values = [
			("mandate", state.settings.mandate.clone()),
			("turns", state.settings.renew_after_turns.to_string()),
			("url", r.map(|r| r.url.clone()).unwrap_or_default()),
			(
				"credential",
				r.map(|r| r.api_key_env.clone())
					.unwrap_or_else(|| "DECODEX_RAGFLOW_API_KEY".into()),
			),
			("user", r.map(|r| r.user_id.clone()).unwrap_or_default()),
			("datasets", r.map(|r| r.dataset_ids.join(", ")).unwrap_or_default()),
			("memory", r.and_then(|r| r.memory_id.clone()).unwrap_or_default()),
		];
		let mut fields = BTreeMap::new();
		for (key, value) in values {
			let input = cx.new(|cx| ComposerInput::with_placeholder(40, "", key, cx));
			input.update(cx, |i, cx| i.set_content(&value, cx));
			fields.insert(key, input);
		}
		self.assistant_settings = Panel {
			owner: Some(owner),
			fields,
			import: r.is_some_and(|r| r.ingest_chats),
			error: String::new(),
		};
		cx.notify();
	}

	fn save_assistant_settings(&mut self, cx: &mut Context<Self>) {
		let Some(owner) = self.assistant_settings.owner.clone().filter(|owner| {
			self.snapshot.as_ref().is_some_and(|s| s.assistant.agent_id.as_ref() == Some(owner))
		}) else {
			return;
		};
		let read = |key| {
			self.assistant_settings
				.fields
				.get(key)
				.map(|f| f.read(cx).content().trim().to_owned())
				.unwrap_or_default()
		};
		let Some(turns) = read("turns").parse::<u32>().ok().filter(|turns| *turns <= 200) else {
			self.assistant_settings.error = "Enter a completed-turn count from 0 to 200.".into();
			cx.notify();
			return;
		};
		let url = read("url");
		let memory = read("memory");
		if !url.is_empty() && self.assistant_settings.import && memory.is_empty() {
			self.assistant_settings.error =
				"Choose a Memory ID before enabling chat import.".into();
			cx.notify();
			return;
		}
		let settings = AssistantSettings {
			mandate: read("mandate"),
			renew_after_turns: turns,
			ragflow: (!url.is_empty()).then(|| RagflowConnection {
				url,
				api_key_env: read("credential"),
				user_id: read("user"),
				dataset_ids: read("datasets")
					.split(',')
					.map(str::trim)
					.filter(|s| !s.is_empty())
					.map(str::to_owned)
					.collect(),
				memory_id: (!memory.is_empty()).then_some(memory),
				ingest_chats: self.assistant_settings.import,
			}),
		};
		self.execute(
			AgentActionDto::ManageAssistant {
				agent_id: EntityId::new(owner).expect("saved Agent"),
				command: AssistantCommand::Configure { settings },
			},
			None,
			cx,
		);
		self.assistant_settings = Panel::default();
		cx.notify();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn assistant_edit_cannot_cross_runtime_or_role_while_chat_draft_stays_saved(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot.as_mut().unwrap().assistant.agent_id = Some("agent".into());
			s.composer.update(cx, |input, cx| input.set_content("Keep the chat draft", cx));
			s.edit_assistant_settings(cx);
			assert_eq!(s.assistant_settings.owner.as_deref(), Some("agent"));
			let mut next = s.snapshot.clone().unwrap();
			next.runtime_source = Some(EntityId::new("replacement-process").unwrap());
			s.invalidate_snapshot_panels(&next);
			assert!(s.assistant_settings.owner.is_none());
			assert!(s.assistant_settings.fields.is_empty());
			assert_eq!(s.composer.read(cx).content(), "Keep the chat draft");
			s.snapshot = Some(next.clone());
			s.edit_assistant_settings(cx);
			assert!(s.assistant_settings.owner.is_some());
			next.assistant.agent_id = Some("other-assistant".into());
			s.invalidate_snapshot_panels(&next);
			assert!(s.assistant_settings.owner.is_none());
		});
	}
}
