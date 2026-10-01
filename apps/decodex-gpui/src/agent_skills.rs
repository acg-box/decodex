//! Select native skill references for the existing composer and draft owner.
use gpui::{AnyElement, AppContext as _};
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::Render;
#[cfg(test)] use crate::shell::agent_surface::Window;
use crate::shell::agent_surface::{
	AgentClient, AgentSurface, ComposerInput, Context, ConversationWorkingDirectory, Entity,
	EntityId, InteractiveElement, IntoElement, LoadState, ParentElement,
	StatefulInteractiveElement, Styled, SubmitComposer, Task, WireText, div, muted, px, rgb,
	ui_theme::TEXT_MUTED,
};
use decodex_protocol::{
	AgentAttachmentDto, AgentSkillDto, AgentSkillsResult, AgentSkillsTarget, AgentWorkKindDto,
	InitialModelCatalogRequest, ModelCatalogPurpose,
};

#[derive(Default)]
pub(super) struct Picker {
	search: Option<Entity<ComposerInput>>,
	source: Option<Source>,
	state: Option<AgentSkillsResult>,
	task: Option<Task<()>>,
	epoch: u64,
}

#[derive(Clone, PartialEq)]
struct Source {
	target: AgentSkillsTarget,
	thread: Option<String>,
	runtime: Option<EntityId>,
}

impl AgentSurface {
	fn skill_source(&self, cx: &Context<Self>) -> Option<Source> {
		if !self.draft_owner_available() {
			return None;
		}

		let owner = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot
				.work_items
				.iter()
				.find(|work| {
					Some(&work.id) == self.composer_manager.as_ref().or(self.selected.as_ref())
						&& work.kind == AgentWorkKindDto::Manager
				})
				.or_else(|| snapshot.work_items.iter().find(|work| work.parent_goal_id.is_none()))
		});
		let (target, thread) = if let Some(work) = owner {
			(
				AgentSkillsTarget::Existing { work_id: EntityId::new(&work.id).ok()? },
				work.codex_thread_id.clone(),
			)
		} else {
			if self.state != LoadState::Ready {
				return None;
			}

			let account = self.account.read(cx).content().trim();

			(
				AgentSkillsTarget::New {
					request: InitialModelCatalogRequest {
						working_directory: ConversationWorkingDirectory::new(
							self.cwd.read(cx).content().trim(),
						)
						.ok()?,
						purpose: ModelCatalogPurpose::Agent,
						account_id: if account.is_empty() {
							None
						} else {
							Some(EntityId::new(account).ok()?)
						},
					},
				},
				None,
			)
		};

		Some(Source {
			target,
			thread,
			runtime: self.snapshot.as_ref().and_then(|s| s.runtime_source.clone()),
		})
	}

	pub(super) fn reset_skill_picker(&mut self) {
		self.skills = Picker { epoch: self.skills.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn open_skill_picker(&mut self, cx: &mut Context<Self>) {
		self.reset_skill_picker();

		self.escape_stop = None;
		self.skills.search = Some(cx.new(|cx| {
			ComposerInput::with_placeholder(0, "Search skills", "Search native skills", cx)
		}));
		self.composer_menu = Some("skills");
		self.composer_menu_content = Some("skills");

		self.load_skills(cx);
	}

	fn load_skills(&mut self, cx: &mut Context<Self>) {
		if self.skills.task.is_some() {
			return;
		}

		let (Some(profile), Some(source)) = (self.profile.clone(), self.skill_source(cx)) else {
			self.skills.state = Some(AgentSkillsResult::Unavailable);

			cx.notify();

			return;
		};
		let filter =
			self.skills.search.as_ref().map(|input| input.read(cx).content()).unwrap_or("");
		let Ok(filter) = WireText::new(filter) else {
			self.feedback = "Skill search is too long.".into();

			cx.notify();

			return;
		};

		self.skills.epoch = self.skills.epoch.wrapping_add(1);

		let epoch = self.skills.epoch;
		let generation = self.generation;

		self.skills.source = Some(source.clone());
		self.skills.state = None;

		let target = source.target.clone();
		let pending = cx.background_executor().spawn(async move {
			let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
				return AgentSkillsResult::Unavailable;
			};

			runtime
				.block_on(AgentClient::new(profile).skills(target, filter))
				.unwrap_or(AgentSkillsResult::Unavailable)
		});

		self.skills.task = Some(cx.spawn(async move |surface, cx| {
			let state = pending.await;
			let _ = surface.update(cx, |s, cx| {
				if s.skills.epoch != epoch {
					return;
				}

				s.skills.task = None;

				if s.generation != generation || s.skill_source(cx).as_ref() != Some(&source) {
					s.skills.state = Some(AgentSkillsResult::Unavailable);
				} else {
					s.skills.state = Some(state);
				}

				cx.notify();
			});
		}));

		cx.notify();
	}

	fn select_skill(&mut self, source: &Source, skill: &AgentSkillDto, cx: &mut Context<Self>) {
		if self.sending
			|| self.uncertain
			|| self.skill_source(cx).as_ref() != Some(source)
			|| !matches!(&self.skills.state,Some(AgentSkillsResult::Available {target,page}) if target==&source.target && page.skills.contains(skill))
		{
			return;
		}
		if self.attachments.len() >= 16 {
			self.feedback = "Select at most 16 files, folders or skills.".into();
		} else {
			let attachment = AgentAttachmentDto {
				path: skill.path.clone(),
				image: false,
				skill_name: Some(skill.name.clone()),
			};

			if !self.attachments.contains(&attachment) {
				self.attachments.push(attachment);
			}

			self.composer_menu = None;
		}

		cx.notify();
	}

	pub(super) fn skill_options(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut panel = div().flex().flex_col().gap_2().on_action(cx.listener(
			|s, _: &SubmitComposer, _, cx| {
				s.load_skills(cx);
				cx.stop_propagation();
			},
		));

		if let Some(search) = &self.skills.search {
			panel = panel.child(div().h(px(36.)).child(search.clone()));
		}

		panel = panel.child(self.workspace_action(
			"skill-search".into(),
			"Find skills".into(),
			|s, cx| s.load_skills(cx),
			cx,
		));

		if self.skills.task.is_some() {
			return panel.child(muted("Reading available skills…")).into_any_element();
		}

		let Some(source) = self
			.skills
			.source
			.as_ref()
			.filter(|source| self.skill_source(cx).as_ref() == Some(*source))
		else {
			return panel
				.child(muted("Choose an available conversation or project, then refresh skills."))
				.into_any_element();
		};
		let Some(AgentSkillsResult::Available { page, .. }) = &self.skills.state else {
			return panel
				.child(muted("Skills are unavailable. Find skills to try again."))
				.into_any_element();
		};
		let mut list =
			div().id("skill-results").max_h(px(280.)).overflow_y_scroll().flex().flex_col().gap_1();

		for (index, skill) in page.skills.iter().enumerate() {
			let source = source.clone();
			let selected = skill.clone();

			list = list.child(
				div()
					.flex()
					.flex_col()
					.child(self.workspace_action(
						format!("skill-choice-{index}"),
						skill.name.as_str().into(),
						move |s, cx| s.select_skill(&source, &selected, cx),
						cx,
					))
					.child(
						div()
							.px_2()
							.text_size(px(10.))
							.text_color(rgb(TEXT_MUTED))
							.child(skill.description.as_str().to_owned()),
					)
					.child(
						div()
							.px_2()
							.text_size(px(9.))
							.text_color(rgb(TEXT_MUTED))
							.child(skill.path.as_str().to_owned()),
					),
			);
		}

		if page.skills.is_empty() {
			list = list.child(muted("No matching enabled skills."));
		}

		panel = panel.child(list);

		if page.truncated {
			panel = panel.child(muted("More skills match. Refine your search."));
		}
		if page.errors > 0 {
			panel = panel
				.child(muted(format!("Codex reported {} skill discovery errors.", page.errors)));
		}

		panel.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::skills::*;
	use gpui::Focusable as _;
	struct SkillPanel(Entity<AgentSurface>);
	impl Render for SkillPanel {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			let panel = self.0.update(cx, |s, cx| s.skill_options(cx));

			div().child(panel).on_action(|_: &SubmitComposer, _, _| {
				panic!("skill search must not submit the main composer")
			})
		}
	}
	#[gpui::test]
	fn skill_picker_click_keeps_exact_reference_and_rejects_changed_source(
		cx: &mut gpui::TestAppContext,
	) {
		cx.update(crate::composer_input::bind_keys);

		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);

			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.skills.search = Some(cx.new(|cx| {
					ComposerInput::with_placeholder(0, "Search skills", "Search native skills", cx)
				}));

				let source = s.skill_source(cx).unwrap();

				s.skills.source = Some(source.clone());
				s.skills.state = Some(AgentSkillsResult::Available {
					target: source.target,
					page: decodex_protocol::AgentSkillsPage {
						skills: vec![AgentSkillDto {
							name: WireText::new("fixture-skill").unwrap(),
							description: WireText::new("Selected native skill").unwrap(),
							path: ConversationWorkingDirectory::new("/tmp/skills (exact)/SKILL.md")
								.unwrap(),
						}],
						truncated: false,
						errors: 0,
					},
				});
			});

			SkillPanel(surface)
		});
		let surface = view.read_with(visual, |v, _| v.0.clone());

		visual.update(|w, cx| {
			w.resize(gpui::size(px(380.), px(500.)));
			w.draw(cx).clear();
		});

		let button = visual.debug_bounds("skill-choice-0").unwrap();

		visual.simulate_click(button.center(), Default::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.attachments.len(), 1);

			let selected = s.attachments[0].clone();

			assert_eq!(selected.skill_name.unwrap().as_str(), "fixture-skill");
			assert_eq!(selected.path.as_str(), "/tmp/skills (exact)/SKILL.md");

			let source = s.skills.source.clone().unwrap();
			let Some(AgentSkillsResult::Available { page, .. }) = &s.skills.state else {
				panic!("catalog")
			};
			let skill = page.skills[0].clone();

			s.attachments.clear();

			s.snapshot.as_mut().unwrap().runtime_source =
				Some(EntityId::new("replacement-runtime").unwrap());

			s.select_skill(&source, &skill, cx);

			assert!(s.attachments.is_empty());
		});

		let search = surface.read_with(visual, |s, _| s.skills.search.clone().unwrap());

		visual.update(|w, cx| {
			w.focus(&search.focus_handle(cx), cx);
			w.draw(cx).clear();
		});
		visual.simulate_keystrokes("enter");
		visual.run_until_parked();
		surface.read_with(visual, |s, _| {
			assert!(matches!(s.skills.state, Some(AgentSkillsResult::Unavailable)))
		});
	}
}
