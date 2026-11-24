//! Project actions retain the project's conversations and reuse inline confirmation.
use decodex_protocol::{
	AgentActionDto, AgentClient, AgentCommandResponse, EntityId, IdempotencyKey,
};
use gpui::{
	AnyElement, Role, SharedString,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
use tokio::runtime::Builder;

use crate::{
	shell::{
		agent_surface::{AgentSurface, Context},
		workspace_symbols::{self, Symbol},
	},
	ui_motion::{self, HoverHint as _},
	ui_theme,
};

impl AgentSurface {
	pub(super) fn navigation_project_controls(
		&self,
		id: &str,
		name: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let project = id.to_owned();
		gpui::div()
			.flex()
			.items_center()
			.child(self.navigation_project_actions(id, name, cx))
			.child(self.navigation_create(
				"New chat in this project",
				format!("navigation-create-project-{id}"),
				move |s, cx| {
					s.new_project_agent(Some(&project), cx);
				},
				cx,
			))
			.into_any_element()
	}

	fn navigation_project_actions(
		&self,
		id: &str,
		name: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let key = format!("project-heading-{id}");
		let confirming = self.workspace.navigation.confirm_delete
			&& self.workspace.navigation.menu.as_deref() == Some(&key);
		let enabled = self.workspace.navigation.mutation.is_none();
		let project = id.to_owned();
		let keyboard = project.clone();
		let debug = format!("navigation-delete-project-{id}");
		let hint = if confirming {
			"Click again to remove this project. Chats and files are kept."
		} else {
			"Delete project"
		};
		gpui::div()
			.id(SharedString::from(debug.clone()))
			.debug_selector(move || debug.clone())
			.role(Role::Button)
			.tab_index(if enabled { 0 } else { -1 })
			.aria_label(format!(
				"{} ({name})",
				if confirming { "Confirm project deletion" } else { "Delete project" }
			))
			.size(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(ui_theme::SMALL_CONTROL_RADIUS))
			.opacity(if enabled { 1. } else { 0.35 })
			.when(enabled, |button| button.cursor_pointer())
			.text_color(ui_theme::rgb(if confirming {
				ui_theme::ERROR
			} else {
				ui_theme::TEXT_MUTED
			}))
			.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.focus_visible(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.hover_hint(hint)
			.child(ui_motion::ConfirmationNudge {
				started: if confirming { self.workspace.navigation.confirm_started } else { None },
				child: workspace_symbols::icon_sized(
					Symbol::Trash,
					ui_theme::NAVIGATION_ACTION_SIZE,
				),
			})
			.on_click(cx.listener(move |s, _, _, cx| {
				if enabled {
					s.request_project_deletion(&project, cx);
				}
				cx.stop_propagation();
			}))
			.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
				if enabled && !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					s.request_project_deletion(&keyboard, cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn request_project_deletion(&mut self, id: &str, cx: &mut Context<Self>) {
		let key = format!("project-heading-{id}");
		if !(self.workspace.navigation.confirm_delete
			&& self.workspace.navigation.menu.as_deref() == Some(&key))
		{
			self.workspace.navigation.menu = Some(key);
			self.workspace.navigation.confirm_delete = true;
			self.workspace.navigation.confirm_started = Some(std::time::Instant::now());
			cx.notify();
			return;
		}
		let Some(profile) = self.profile.clone() else { return };
		let connection = profile.clone();
		let epoch = self.command_epoch;
		let id = id.to_owned();
		let action = AgentActionDto::DeleteProject {
			project_id: EntityId::new(&id).expect("saved project identity"),
		};
		self.dismiss_navigation_confirmation(cx);
		self.workspace.navigation.feedback = None;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
			runtime
				.block_on(
					AgentClient::new(connection).execute(
						action,
						IdempotencyKey::new(super::super::unique_command())
							.expect("generated command identity"),
					),
				)
				.map_err(|e| e.to_string())
		});
		self.workspace.navigation.mutation = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.command_epoch != epoch || s.profile.as_ref() != Some(&profile) {
					return;
				}
				s.workspace.navigation.mutation = None;
				if matches!(result, Ok(AgentCommandResponse::Accepted { .. })) {
					s.detach_navigation_project(&id, cx);
					s.refresh(cx);
				} else {
					s.workspace.navigation.feedback = Some(
						"Project deletion was not confirmed. Refresh before trying again.".into(),
					);
				}
				cx.notify();
			});
		}));
	}

	pub(super) fn detach_navigation_project(&mut self, id: &str, cx: &mut Context<Self>) {
		if let Some(snapshot) = self.snapshot.as_mut() {
			snapshot.projects.retain(|p| p.id != id);
		}
		self.workspace.navigation.projects.remove(id);
		self.workspace.navigation.collapsed.remove(super::Section::Chats.key());
		if self.workspace.workspace_filter.as_deref() == Some(id) {
			self.workspace.workspace_filter = None;
		}
		if self.workspace.new_conversation_workspace.as_deref() == Some(id) {
			self.workspace.new_conversation_workspace = None;
		}
		let scope = self.profile.as_ref().map_or_else(|| "unbound".into(), |p| p.draft_scope_key());
		if let Some(drafts) = self.workspace.navigation.preferences.project_drafts.get_mut(&scope)
			&& let Some(draft) = drafts.remove(id)
		{
			drafts.entry(String::new()).or_insert(draft);
		}
		self.workspace.navigation.preferences.save();
		self.save_draft_document(cx);
		cx.notify();
	}
}
