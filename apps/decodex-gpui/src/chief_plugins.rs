//! Thread plugin exclusions; shared installation and active tool state remain native-owned.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::{ChiefPluginOutcome as Outcome, ChiefPluginSelectionState as State};

#[derive(Default)]
pub(super) struct Panel {
	state: Option<State>,
	work: Option<String>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
	reviewed: bool,
}
impl ChiefSurface {
	pub(super) fn reset_task_plugins(&mut self) {
		self.task_plugins =
			Panel { epoch: self.task_plugins.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_task_plugins(&mut self, next: &ChiefSnapshotDto) {
		let Some(work) = &self.task_plugins.work else {
			return;
		};
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);
		if !matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.dispatch_state==b.dispatch_state && a.active_turn_id==b.active_turn_id)
			|| self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
		{
			self.reset_task_plugins();
		}
	}

	fn plugin_selection_action(
		&self,
		work: &EntityId,
		thread: &str,
		plugin_id: WireText,
		enabled: bool,
	) -> Option<ChiefActionDto> {
		let State::Available {
			work_id,
			thread_id,
			review_token,
			disabled_plugin_ids,
			catalog,
			can_update: true,
			..
		} = self.task_plugins.state.as_ref()?
		else {
			return None;
		};
		if work_id != work
			|| thread_id.as_str() != thread
			|| self.task_plugins.work.as_deref() != Some(work.as_str())
			|| (enabled && !disabled_plugin_ids.contains(&plugin_id))
			|| (!enabled
				&& !matches!(catalog,decodex_protocol::ChiefPluginInventory::Available {plugins,..} if plugins.iter().any(|p|p.id==plugin_id.as_str() && p.installed)))
		{
			return None;
		}
		Some(ChiefActionDto::SetTaskPlugin {
			work_id: work.clone(),
			thread_id: thread_id.clone(),
			review_token: review_token.clone(),
			plugin_id,
			enabled,
		})
	}

	fn update_task_plugins(
		&mut self,
		work: String,
		selection: Option<(WireText, bool)>,
		cx: &mut Context<Self>,
	) {
		if self.task_plugins.task.is_some()
			|| self.selected.as_ref() != Some(&work)
			|| !self.command_connection_ready()
			|| self.native_agents.selected.is_some()
			|| (selection.is_some() && !self.task_plugins.reviewed)
		{
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Some(snapshot) = &self.snapshot else {
			return;
		};
		let Some(source) = snapshot.runtime_source.clone() else {
			return;
		};
		let Some(thread) = snapshot
			.work_items
			.iter()
			.find(|w| w.id == work)
			.and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		let Ok(work_id) = EntityId::new(work.clone()) else {
			return;
		};
		let action = if let Some((plugin_id, enabled)) = selection {
			let Some(action) = self.plugin_selection_action(&work_id, &thread, plugin_id, enabled)
			else {
				return;
			};
			Some(action)
		} else {
			None
		};
		let saving = action.is_some();
		let generation = self.generation;
		self.task_plugins.epoch = self.task_plugins.epoch.wrapping_add(1);
		let epoch = self.task_plugins.epoch;
		self.task_plugins.work = Some(work.clone());
		self.task_plugins.state = None;
		self.task_plugins.reviewed = false;
		self.task_plugins.feedback = if saving {
			"Submitting task plugin selection…"
		} else {
			"Reading native task plugin settings…"
		}
		.into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.plugin_selection(work_id)).unwrap_or(State::Unavailable);
			Some((outcome, state))
		});
		self.task_plugins.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation || s.task_plugins.epoch != epoch {
					return;
				}
				s.task_plugins.task = None;
				let current = s.command_connection_ready()
					&& s.native_agents.selected.is_none()
					&& s.selected.as_ref() == Some(&work)
					&& s.snapshot.as_ref().is_some_and(|snapshot| {
						snapshot.runtime_source.as_ref() == Some(&source)
							&& snapshot.work_items.iter().any(|w| {
								w.id == work && w.codex_thread_id.as_deref() == Some(&thread)
							})
					});
				if !current {
					s.reset_task_plugins();
					cx.notify();
					return;
				}
				let (outcome, state) = result.unwrap_or((None, State::Unavailable));
				s.task_plugins.reviewed = !saving;
				s.task_plugins.feedback = match outcome {
					Some(Ok(ChiefCommandResponse::Accepted { .. })) =>
						"Selection queued. The native state below determines whether it was saved for the next turn.",
					Some(Ok(ChiefCommandResponse::Rejected { .. })) =>
						"Selection was not accepted. Refresh plugin settings before choosing again.",
					Some(_) => "Selection could not be confirmed. It was not retried.",
					None if saving => "Selection could not be confirmed. Refresh its saved state.",
					None =>
						"Changes apply to subsequent turns. Shared plugin configuration is unchanged.",
				}
				.into();
				s.task_plugins.state = Some(match state {
					State::Available { ref work_id, ref thread_id, .. }
						if work_id.as_str() != work || thread_id.as_str() != thread =>
						State::Unavailable,
					other => other,
				});
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn task_plugins_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if work.codex_thread_id.is_none()
			|| self.native_agents.selected.is_some()
			|| !self.command_connection_ready()
		{
			return div().into_any_element();
		}
		let owner = work.id.clone();
		let mut panel = div().flex().flex_col().gap_2().child("Task plugins").child(mcp_button(
			"task-plugins-read".into(),
			"Review / refresh task plugins".into(),
			false,
			cx,
			move |s, cx| s.update_task_plugins(owner.clone(), None, cx),
		));
		if self.task_plugins.work.as_ref() != Some(&work.id) {
			return panel.into_any_element();
		}
		panel = panel.child(self.task_plugins.feedback.clone());
		match &self.task_plugins.state {
			Some(State::Available {
				disabled_plugin_ids,
				catalog,
				can_update,
				last_outcome,
				..
			}) => {
				panel=panel.child("Exclusions apply to subsequent turns. Shared installation, policy and hook trust still control availability.");
				if let Some(outcome) = last_outcome {
					panel = panel.child(format!("Last selection: {}", label(*outcome)));
				}
				let mut rows = std::collections::BTreeMap::new();
				if let decodex_protocol::ChiefPluginInventory::Available { plugins, errors } =
					catalog
				{
					for p in plugins.iter().filter(|p| p.installed) {
						rows.insert(
							p.id.clone(),
							format!(
								"{} · shared {}",
								p.name,
								if p.enabled { "enabled" } else { "disabled" }
							),
						);
					}
					if !errors.is_empty() {
						panel = panel.child("Some shared plugin sources could not be read.");
					}
				} else {
					panel = panel.child(
						"Shared plugin discovery is unavailable. Saved exclusions can still be removed.",
					);
				}
				for id in disabled_plugin_ids {
					rows.entry(id.as_str().to_owned())
						.or_insert_with(|| format!("{} · installation not reported", id.as_str()));
				}
				if rows.is_empty() {
					panel = panel.child("No installed plugins or task exclusions were reported.");
				}
				for (index, (id, name)) in rows.into_iter().enumerate() {
					let excluded = disabled_plugin_ids.iter().any(|v| v.as_str() == id);
					panel = panel.child(format!(
						"{name} · {}",
						if excluded {
							"excluded for this task"
						} else {
							"not excluded for this task"
						}
					));
					if *can_update
						&& self.task_plugins.task.is_none()
						&& self.task_plugins.reviewed
						&& let Ok(plugin) = WireText::new(id)
					{
						let owner = work.id.clone();
						panel = panel.child(mcp_button(
							format!("task-plugin-{index}"),
							if excluded { "Allow for this task" } else { "Exclude from this task" }
								.into(),
							false,
							cx,
							move |s, cx| {
								s.update_task_plugins(
									owner.clone(),
									Some((plugin.clone(), excluded)),
									cx,
								)
							},
						));
					}
				}
			},
			Some(State::Pending { state, .. }) => {
				panel = panel
					.child(label(*state))
					.child("Refresh to read confirmation. The selection will not be resent.");
			},
			Some(State::Unavailable) => {
				panel = panel.child(
					"Current task plugin settings are unavailable. Refresh when the task is connected.",
				);
			},
			None => {},
		}
		panel.into_any_element()
	}
}
fn label(state: Outcome) -> &'static str {
	match state {
		Outcome::Reserved => "Awaiting confirmation",
		Outcome::Queued => "Queued; awaiting native settings",
		Outcome::Unknown => "Unconfirmed",
		Outcome::Rejected => "Rejected",
		Outcome::TargetObserved => "Saved selection observed for subsequent turns",
		Outcome::Superseded => "Replaced by current native selection",
	}
}

#[cfg(test)]
#[path = "chief_plugins_wire_tests.rs"]
mod wire_tests;
