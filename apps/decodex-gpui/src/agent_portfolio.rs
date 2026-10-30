//! Work outcomes and scoped contributions in the existing desktop surface.
use std::{iter, path::Path};

use gpui::{AnyElement, Div, FontWeight, Role, Stateful, StatefulInteractiveElement};
use tokio::runtime::Builder;

use crate::{
	shell::agent_surface::{
		AgentSurface, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
		Styled, mcp_forms,
	},
	ui_theme::{self, TEXT_MUTED},
};
use decodex_protocol::{
	AgentActionDto, AgentClient, EntityId, PrivateReviewDetailsResult, ProjectDto, WireText,
	guardian::ReviewDetails,
	work::{ResultSource, Work, WorkCommand, WorkPortfolio, WorkResult, WorkState},
};

impl AgentSurface {
	pub(super) fn render_work_portfolio(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut list = gpui::div()
			.id("work-portfolio")
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap_3()
			.p_4()
			.child(gpui::div().text_lg().child("Work"))
			.child(self.workspace_choices(false, cx));
		let Some(snapshot) = &self.snapshot else {
			return list.child("Connect to read Work.").into_any_element();
		};

		list = list.child(self.assistant_role(cx));
		list = list.child(self.list_status_filters(
			super::list_status::View::Work,
			snapshot.portfolio.works.iter().map(state),
			cx,
		));

		let filter = self.workspace.workspace_filter.as_deref();
		let query = self.work_search.read(cx).content().trim().to_lowercase();
		let mut count = 0;

		for project in snapshot.projects.iter().map(Some).chain(iter::once(None)) {
			let id = project.map(|p| p.id.as_str());

			if filter.is_some() && filter != id {
				continue;
			}

			let works: Vec<_> = snapshot
				.portfolio
				.works
				.iter()
				.filter(|w| {
					w.project_id.as_deref() == id
						&& self
							.workspace
							.list_status
							.matches(super::list_status::View::Work, state(w))
						&& (query.is_empty() || w.title.to_lowercase().contains(&query))
				})
				.collect();

			if works.is_empty()
				&& (project.is_none()
					|| self
						.workspace
						.list_status
						.selected(super::list_status::View::Work)
						.is_some())
			{
				continue;
			}

			let title = project.map(|p| p.name.clone()).unwrap_or_else(|| "No Project".into());
			let mut group = gpui::div()
				.flex()
				.flex_col()
				.gap_2()
				.child(gpui::div().font_weight(FontWeight::SEMIBOLD).child(title));

			if let Some(project) = project {
				group = group.child(self.project_lead(project, cx));
			}

			for work in works {
				count += 1;
				group = group.child(self.work_row(work, cx));
			}

			list = list.child(group);
		}

		if count == 0 {
			list = list.child(label(
				if self.workspace.list_status.selected(super::list_status::View::Work).is_some()
					|| !query.is_empty()
				{
					"No matching Work."
				} else {
					"Discuss your goal with an Agent to create Work."
				},
			));
		}
		if snapshot.portfolio.has_more {
			list = list.child(label("More Work and evidence are available in storage."));
		}

		list.into_any_element()
	}

	fn work_contract_detail(&self, work: &Work, cx: &mut Context<Self>) -> AnyElement {
		let Some(portfolio) = &self.workspace.work_evidence else {
			return gpui::div().into_any_element();
		};
		let owner = work.owner_agent_id.clone();
		let mut panel = gpui::div()
			.id(SharedString::from(format!("work-detail-{}", work.id)))
			.pl_3()
			.flex()
			.flex_col()
			.gap_2()
			.child(label(format!(
				"Requirements {} · {}",
				work.requirements.id, work.requirements.source
			)))
			.child(self.work_usage_panel(work, cx))
			.child(readable(work.requirements.goal.clone()))
			.child(label(format!("Completion criteria: {}", work.requirements.criteria)));

		panel = panel.child(mcp_forms::mcp_button(
			format!("work-owner-{}", work.id),
			"Discuss with owner".into(),
			false,
			cx,
			move |s, cx| {
				s.workspace.browsing = false;
				s.workspace.portfolio_mode = false;

				s.open_page(&owner, cx);
			},
		));

		if let Some(reason) = &work.reason {
			panel = panel.child(label(format!(
				"{reason} · {}",
				work.resume_condition.as_deref().unwrap_or("No resumption requested")
			)));
		}

		panel = self.work_state_controls(panel, work, cx);
		panel = panel.child(self.work_materials(work, portfolio, cx));
		panel = panel.child(self.work_collaborations(work, portfolio, cx));

		for native in portfolio.native_contributions.iter().filter(|c| c.work_id == work.id) {
			panel = panel
				.child(label(format!(
					"Subagent {} · {} · requirements {}",
					native.id,
					native
						.child_thread_id
						.as_deref()
						.unwrap_or("Binding unconfirmed. Check history before spawning again"),
					native.requirements_id
				)))
				.child(readable(native.instruction.clone()));
		}

		panel = panel.child(label("Results"));

		for result in portfolio.results.iter().filter(|r| r.work_id == work.id) {
			let key = format!("result:{}", result.id);
			let selected = self.workspace.contribution_detail.as_ref() == Some(&key);

			panel = panel.child(mcp_forms::mcp_button(
				format!("open-result-{}", result.id),
				result.summary.clone(),
				selected,
				cx,
				move |s, cx| {
					s.workspace.contribution_detail = Some(key.clone());

					cx.notify();
				},
			));

			if selected {
				panel = panel.child(self.result_details(result, cx));
			}
		}

		if portfolio.has_more {
			panel = panel.child(label(
				"More records are available. Ask the owner to review them before assessment.",
			));
		}

		if let Some(assessment) = &work.assessment {
			panel = panel
				.child(label("Owner assessment"))
				.child(readable(assessment.summary.clone()))
				.child(label(format!(
					"Requirements {} · Results {} · Limits: {}",
					assessment.requirements_id,
					assessment.result_ids.join(", "),
					assessment.limits
				)));
		}

		panel.into_any_element()
	}

	fn work_state_controls(
		&self,
		mut panel: Stateful<Div>,
		work: &Work,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		if matches!(work.state, WorkState::Open | WorkState::Blocked) {
			panel = panel.child(self.work_hold_control(work, cx));
		}

		panel
	}
}

impl AgentSurface {
	fn work_hold_control(&self, work: &Work, cx: &mut Context<Self>) -> impl IntoElement {
		let panel = gpui::div().flex().flex_col().gap_2().child(label(
			"Pause blocks new contributions and completion. Running actions continue.",
		));
		let work = work.clone();
		let paused = work.user_hold;

		panel.child(mcp_forms::mcp_button(
			format!("work-hold-{}", work.id),
			if paused { "Resume Work" } else { "Pause Work" }.into(),
			false,
			cx,
			move |s, cx| {
				s.execute(
					AgentActionDto::ManageWork {
						agent_id: EntityId::new(work.owner_agent_id.clone()).expect("saved owner"),
						command: WorkCommand::SetState {
							work_id: work.id.clone(),
							expected_requirements: work.requirements.id.clone(),
							state: if paused { WorkState::Open } else { WorkState::Blocked },
							reason: if paused {
								None
							} else {
								Some("User paused this Work".into())
							},
							resume_condition: if paused {
								None
							} else {
								Some("Explicit user release".into())
							},
							user_hold: !paused,
						},
					},
					None,
					cx,
				);
			},
		))
	}
}

impl AgentSurface {
	pub(super) fn render_global_guardian(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut panel = gpui::div()
			.id("global-guardian-history")
			.debug_selector(|| "global-guardian-history".into())
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap_3()
			.p_4()
			.child(gpui::div().text_lg().child("Guardian"));
		let Some(snapshot) = &self.snapshot else {
			return panel.child(label("Connect to view review history.")).into_any_element();
		};

		if snapshot.guardian.details.is_empty() {
			panel = panel.child(
				label("No saved review details.")
					.debug_selector(|| "guardian-history-empty".into()),
			);
		}

		for details in &snapshot.guardian.details {
			let key = format!(
				"review-detail-{}-{}-{}",
				details.reviewer_thread_id, details.reviewer_turn_id, details.item_id
			);
			let selected = key.clone();
			let record = details.clone();

			panel = panel.child(mcp_forms::mcp_button(
				key.clone(),
				format!(
					"Review details · {} · {}",
					details.reviewer_turn_id,
					details.review_id.as_deref().unwrap_or("Action association unavailable")
				),
				false,
				cx,
				move |s, cx| {
					s.workspace.contribution_detail = Some(selected.clone());

					s.load_private_review_record(selected.clone(), record.clone(), cx);
					cx.notify();
				},
			));

			if self.workspace.contribution_detail.as_ref() == Some(&key) {
				panel = panel
					.child(label(format!(
						"Reviewer thread {} · Turn {} · Item {} · Parent {}",
						details.reviewer_thread_id,
						details.reviewer_turn_id,
						details.item_id,
						details.parent_thread_id.as_deref().unwrap_or("Unknown")
					)))
					.child(
						self.workspace
							.private_detail_text
							.as_ref()
							.filter(|(id, _)| id == &key)
							.map(|(_, text)| text.clone())
							.unwrap_or_else(|| details.record.clone()),
					);

				if details.truncated {
					panel = panel
						.child(label("Partial review observation; full transcript not included."));
				}
			}
		}

		panel.into_any_element()
	}
}

impl AgentSurface {
	fn load_private_review_record(
		&mut self,
		key: String,
		record: ReviewDetails,
		cx: &mut Context<Self>,
	) {
		let Some(profile) = self.profile.clone() else { return };
		let captured = profile.clone();

		self.workspace.private_detail_text = Some((key.clone(), "Reading saved record…".into()));

		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Could not read the saved record")?;

			runtime.block_on(async move {
				let client = AgentClient::new(profile);
				let mut offset = 0;
				let mut complete = String::new();
				let mut expected = None;

				loop {
					let result = client
						.private_review_details(
							WireText::new(record.reviewer_thread_id.clone())
								.map_err(|_| "Invalid source identity")?,
							WireText::new(record.reviewer_turn_id.clone())
								.map_err(|_| "Invalid turn identity")?,
							WireText::new(record.item_id.clone())
								.map_err(|_| "Invalid item identity")?,
							offset,
						)
						.await
						.map_err(|_| "Saved record unavailable")?;
					let PrivateReviewDetailsResult::Available {
						offset: actual,
						total_bytes,
						text,
						next_offset,
					} = result
					else {
						return Err("Record unavailable or no longer retained");
					};

					if actual != offset
						|| total_bytes > 65_536
						|| expected.is_some_and(|total| total != total_bytes)
						|| text.is_empty()
					{
						return Err("Records changed. Refresh to retry");
					}

					expected = Some(total_bytes);

					complete.push_str(&text);

					if complete.len() > total_bytes {
						return Err("Invalid record page length");
					}

					if let Some(next) = next_offset {
						if next != complete.len() || next <= offset {
							return Err("Record paging did not advance");
						}

						offset = next;
					} else {
						if complete.len() != total_bytes {
							return Err("Record incomplete");
						}

						return Ok(complete);
					}
				}
			})
		});

		cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.profile.as_ref() != Some(&captured)
					|| s.workspace.contribution_detail.as_ref() != Some(&key)
				{
					return;
				}

				s.workspace.private_detail_text = Some((key, result.unwrap_or_else(str::to_owned)));

				cx.notify();
			});
		})
		.detach();
	}
}

impl AgentSurface {
	pub(super) fn load_work_detail(&mut self, id: String, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else { return };
		let captured = profile.clone();

		self.workspace.work_evidence = None;
		self.workspace.work_read_error = None;

		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Work reader is unavailable")?;

			runtime.block_on(async move {
				AgentClient::new(profile)
					.work_detail(EntityId::new(id).map_err(|_| "Invalid Work identity")?)
					.await
					.map_err(|_| "Work changed or unavailable. Refresh to retry.")
			})
		});

		cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.profile.as_ref() != Some(&captured) {
					return;
				}

				match result {
					Ok(portfolio)
						if portfolio
							.works
							.iter()
							.any(|w| Some(&w.id) == s.workspace.work_detail.as_ref()) =>
						s.workspace.work_evidence = Some(portfolio),
					Err(error) => s.workspace.work_read_error = Some(error.into()),
					_ => {},
				}

				cx.notify();
			});
		})
		.detach();
	}

	fn result_details(&self, result: &WorkResult, cx: &mut Context<Self>) -> AnyElement {
		let mut detail = gpui::div()
			.flex()
			.flex_col()
			.gap_1()
			.child(label(format!(
				"Result {} · Agent {} · requirements {}",
				result.id, result.agent_id, result.requirements_id
			)))
			.child(label(format!("Limits: {}", result.limits)));

		match &result.source {
			ResultSource::NativeItem { thread_id, turn_id, item_id } => {
				detail = detail
					.child(label(format!("Thread {thread_id} · Turn {turn_id} · Item {item_id}")));

				let agent = result.agent_id.clone();

				detail = detail.child(mcp_forms::mcp_button(
					format!("result-agent-{}", result.id),
					"Open contributing Agent".into(),
					false,
					cx,
					move |s, cx| {
						s.workspace.portfolio_mode = false;

						s.open_page(&agent, cx);
					},
				));
			},
			ResultSource::Artifact { path, sha256 } => {
				let path = path.clone();

				detail = detail.child(label(format!("{path} · SHA-256 {sha256}"))).child(
					mcp_forms::mcp_button(
						format!("result-file-{}", result.id),
						"Reveal Result file".into(),
						false,
						cx,
						move |_, cx| cx.reveal_path(Path::new(&path)),
					),
				);
			},
		}

		detail.into_any_element()
	}
}

impl AgentSurface {
	fn assistant_role(&self, cx: &mut Context<Self>) -> AnyElement {
		let Some(snapshot) = &self.snapshot else { return gpui::div().into_any_element() };
		let current = snapshot.agents.iter().find(|a| a.assistant);
		let mut row = gpui::div().flex().flex_col().gap_1().child(mcp_forms::mcp_button(
			"assistant-role".into(),
			format!("Assistant: {}", current.map(|a| a.name.as_str()).unwrap_or("None")),
			false,
			cx,
			|s, cx| {
				s.workspace.assistant_picker = !s.workspace.assistant_picker;

				cx.notify();
			},
		));

		if self.workspace.assistant_picker {
			for agent in snapshot.agents.iter() {
				let (id, enabled) = (agent.id.clone(), !agent.assistant);

				row = row.child(mcp_forms::mcp_button(
					format!("choose-assistant-{id}"),
					format!("{}{}", if enabled { "Use " } else { "Remove " }, agent.name),
					false,
					cx,
					move |s, cx| {
						s.execute(
							AgentActionDto::SetAssistant {
								agent_id: EntityId::new(id.clone()).expect("saved Agent"),
								enabled,
							},
							None,
							cx,
						);

						s.workspace.assistant_picker = false;
					},
				));
			}
		}

		row.into_any_element()
	}
}

impl AgentSurface {
	fn project_lead(&self, project: &ProjectDto, cx: &mut Context<Self>) -> AnyElement {
		let Some(snapshot) = &self.snapshot else { return gpui::div().into_any_element() };
		let lead = project
			.lead_agent_id
			.as_ref()
			.and_then(|id| snapshot.agents.iter().find(|a| &a.id == id));
		let project_id = project.id.clone();
		let mut group = gpui::div().flex().flex_col().gap_1();

		group = group.child(mcp_forms::mcp_button(
			format!("project-lead-{}", project.id),
			format!("Project lead: {}", lead.map(|a| a.name.as_str()).unwrap_or("Choose Agent")),
			false,
			cx,
			move |s, cx| {
				s.workspace.project_lead_picker = Some(project_id.clone());

				cx.notify();
			},
		));

		if self.workspace.project_lead_picker.as_ref() == Some(&project.id) {
			for candidate in snapshot.agents.iter() {
				let (project, agent) = (project.id.clone(), candidate.id.clone());

				group = group.child(mcp_forms::mcp_button(
					format!("choose-lead-{project}-{agent}"),
					candidate.name.clone(),
					false,
					cx,
					move |s, cx| {
						s.execute(
							AgentActionDto::SetProjectLead {
								project_id: EntityId::new(project.clone()).expect("saved Project"),
								agent_id: EntityId::new(agent.clone()).expect("saved Agent"),
							},
							None,
							cx,
						);

						s.workspace.project_lead_picker = None;
					},
				));
			}
		}

		group.into_any_element()
	}

	fn work_row(&self, work: &Work, cx: &mut Context<Self>) -> AnyElement {
		let Some(snapshot) = &self.snapshot else { return gpui::div().into_any_element() };
		let id = work.id.clone();
		let owner = snapshot
			.agents
			.iter()
			.find(|a| a.id == work.owner_agent_id)
			.map(|a| a.name.as_str())
			.unwrap_or(&work.owner_agent_id);
		let mut group = gpui::div().flex().flex_col().gap_1();

		group = group.child(mcp_forms::mcp_button(
			format!("work-{}", work.id),
			format!(
				"{}  ·  {}  ·  {}{}",
				work.title,
				owner,
				state(work),
				if snapshot.portfolio.awaiting_assessment.contains(&work.id) {
					" · Awaiting assessment"
				} else {
					""
				}
			),
			false,
			cx,
			move |s, cx| {
				s.workspace.work_detail = if s.workspace.work_detail.as_ref() == Some(&id) {
					None
				} else {
					Some(id.clone())
				};

				if s.workspace.work_detail.is_some() {
					s.load_work_detail(id.clone(), cx);
				}

				cx.notify();
			},
		));

		if self.workspace.work_detail.as_ref() == Some(&work.id) {
			group = group.child(mcp_forms::mcp_button(
				format!("refresh-work-{}", work.id),
				"Refresh Work".into(),
				false,
				cx,
				{
					let id = work.id.clone();

					move |s, cx| s.load_work_detail(id.clone(), cx)
				},
			));

			if let Some(selected) = self
				.workspace
				.work_evidence
				.as_ref()
				.and_then(|p| p.works.iter().find(|w| w.id == work.id))
			{
				if selected.requirements.id != work.requirements.id || selected.state != work.state
				{
					group = group.child(label("Work changed. Refresh before assessment."));
				}

				group = group.child(self.work_contract_detail(selected, cx));
			} else {
				group = group.child(label(
					self.workspace
						.work_read_error
						.clone()
						.unwrap_or_else(|| "Reading selected Work…".into()),
				));
			}
		}

		group.into_any_element()
	}

	fn work_materials(
		&self,
		work: &Work,
		portfolio: &WorkPortfolio,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut panel = gpui::div().flex().flex_col().gap_1();

		panel = panel.child(label("Materials"));

		for material in portfolio.materials.iter().filter(|m| m.work_id == work.id) {
			let reference = material.reference.clone();

			panel = panel.child(mcp_forms::mcp_button(
				format!("material-{}-{}", work.id, material.id),
				format!(
					"{} · {}{}",
					material.title,
					reference,
					material.sha256.as_ref().map(|d| format!(" · SHA-256 {d}")).unwrap_or_default()
				),
				false,
				cx,
				move |_, cx| {
					if reference.starts_with("https://") || reference.starts_with("http://") {
						cx.open_url(&reference);
					} else if Path::new(&reference).is_absolute() {
						cx.reveal_path(Path::new(&reference));
					}
				},
			));
		}

		panel.into_any_element()
	}

	fn work_collaborations(
		&self,
		work: &Work,
		portfolio: &WorkPortfolio,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut panel = gpui::div().flex().flex_col().gap_1();

		panel = panel.child(label("Collaboration"));

		for request in portfolio.collaborations.iter().filter(|q| q.work_id == work.id) {
			let id = request.id.clone();

			panel = panel.child(mcp_forms::mcp_button(
				format!("collaboration-{id}"),
				format!(
					"{} · {:?}",
					self.snapshot
						.as_ref()
						.and_then(|s| s.agents.iter().find(|a| a.id == request.recipient_agent_id))
						.map(|a| a.name.as_str())
						.unwrap_or(&request.recipient_agent_id),
					request.state
				),
				false,
				cx,
				move |s, cx| {
					s.workspace.contribution_detail = Some(id.clone());

					cx.notify();
				},
			));

			if self.workspace.contribution_detail.as_ref() == Some(&request.id) {
				panel = panel
					.child(label(format!(
						"Instruction {} · requirements {} · transport {}",
						request.instruction.id,
						request.instruction.requirements_id,
						request
							.instruction
							.delivered_turn_id
							.as_deref()
							.map(|t| if t.is_empty() {
								"Unknown admission".into()
							} else {
								format!("Delivered to {t}")
							})
							.unwrap_or_else(|| "Not admitted".into())
					)))
					.child(readable(request.instruction.text.clone()))
					.child(label(format!(
						"Shared Materials: {}",
						request.instruction.material_ids.join(", ")
					)))
					.child(label(format!(
						"Assessment: {} · {}",
						request.outcome.as_deref().unwrap_or("Pending"),
						request.note.as_deref().unwrap_or("No assessment")
					)));
			}
		}

		panel.into_any_element()
	}

	pub(crate) fn render_guardian_settings(
		&mut self,
		_: &mut gpui::Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let settings = self.snapshot.as_ref().map(|s| &s.guardian.settings);
		let enabled = settings.is_some_and(|settings| settings.review_details);
		let revision = settings.map_or(0, |settings| settings.revision);
		let interactive = settings.is_some()
			&& self.profile.is_some()
			&& self.command_connection_ready()
			&& !self.sending
			&& !self.uncertain;
		let row = ui_theme::settings_row()
			.child(ui_theme::settings_label(
				"Save review details",
				"Keep Guardian’s review explanations for later. Permissions stay unchanged.",
			))
			.child(
				crate::settings_surface::settings_switch(
					"guardian-review-details",
					"guardian-review-details-knob",
					"Save review details",
					enabled,
					interactive,
				)
				.on_change(cx.listener(move |s, desired: &bool, _, cx| {
					s.execute(
						AgentActionDto::SetReviewDetails {
							settings_id: EntityId::new("guardian-global")
								.expect("global settings identity"),
							expected_revision: revision,
							review_details: *desired,
						},
						None,
						cx,
					);
				})),
			);
		ui_theme::settings_group("Guardian", vec![row.into_any_element()]).into_any_element()
	}
}

fn label(text: impl Into<SharedString>) -> Stateful<Div> {
	readable(text).text_color(ui_theme::rgb(TEXT_MUTED)).text_size(gpui::px(12.))
}

fn readable(text: impl Into<SharedString>) -> Stateful<Div> {
	let text = text.into();

	gpui::div().id(text.clone()).role(Role::Label).aria_label(text.clone()).child(text)
}

fn state(work: &Work) -> &'static str {
	match work.state {
		WorkState::Open => "Open",
		WorkState::Blocked => "Blocked",
		WorkState::Completed => "Completed",
		WorkState::Cancelled => "Cancelled",
	}
}

#[cfg(test)]
#[path = "agent_portfolio_tests.rs"]
mod tests;
