//! Native task resource inspection, refreshed only while the selected panel is open.
use std::{rc::Rc, time::Duration};

use gpui::{AnyElement, Div, KeyDownEvent};
use reqwest::Url;
use serde_json::Value;
use tokio::runtime::Builder;

#[cfg(test)]
use crate::shell::agent_surface::{
	AgentDispatchStateDto, AgentSnapshotResult, AgentWorkItemDto, AgentWorkStatusDto,
};
use crate::{
	shell::{
		agent_surface,
		agent_surface::{
			AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
			Context, EntityId, IdempotencyKey, InteractiveElement, IntoElement, ParentElement,
			Role, SharedString, StatefulInteractiveElement, Styled, SubmitComposer, WireText,
		},
	},
	ui_loading,
};
use decodex_protocol::AgentResourcesResult;

impl AgentSurface {
	pub(super) fn reset_resources(&mut self) {
		let opened = self.resources.take().is_some();

		self.resources_task = None;

		if self.resource_mutation_task.take().is_some() {
			self.resource_feedback = "The resource change is unconfirmed. Reopen task resources to inspect the current state; it was not retried.".into();
		} else if opened {
			self.resource_feedback.clear();
		}
	}

	pub(super) fn invalidate_resources(&mut self, next: &AgentSnapshotDto) {
		let work = self
			.resources
			.as_ref()
			.map(|(work, _)| work.as_str())
			.or_else(|| self.resource_mutation_task.as_ref().and(self.selected.as_deref()));
		let Some(work) = work else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == work));
		let after = next.work_items.iter().find(|w| w.id == work);

		if !matches!((before, after), (Some(a), Some(b)) if a.codex_thread_id == b.codex_thread_id)
			|| self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
		{
			self.reset_resources();
		}
	}

	pub(super) fn resources_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		let opened = self.resources.as_ref().filter(|(owner, _)| owner == work);
		let click = work.to_owned();
		let key = click.clone();
		let mut panel = gpui::div().flex().flex_col().gap_2().child(
			gpui::div()
				.id("agent-resources-toggle")
				.debug_selector(|| "agent-resources-toggle".into())
				.role(Role::Button)
				.tab_index(0)
				.aria_label("Task resources")
				.aria_expanded(opened.is_some())
				.cursor_pointer()
				.on_click(cx.listener(move |s, _, _, cx| s.toggle_resources(&click, cx)))
				.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
					if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
						cx.stop_propagation();
						s.toggle_resources(&key, cx);
					}
				}))
				.child("Task resources"),
		);

		if let Some((_, result)) = opened {
			let body = match result {
				None => gpui::div().child(ui_loading::loading("Loading resources")),
				Some(AgentResourcesResult::Unsupported) =>
					gpui::div().child("This Codex provider does not support task resources."),
				Some(AgentResourcesResult::Unavailable) =>
					gpui::div().child("Task resources are unavailable. Retrying…"),
				Some(AgentResourcesResult::CapacityExceeded) =>
					gpui::div().child("The resource list exceeds the display limit."),
				Some(AgentResourcesResult::Available { resources }) => {
					let mut list = self.resource_editor(work, cx);

					if resources.is_empty() {
						list = list.child("No task resources.");
					}

					for resource in resources {
						let owner = work.to_owned();
						let kind = resource.attachment_type.clone();
						let key = resource.identity_key.clone();
						let payload = serde_json::from_str::<Value>(&resource.payload_json)
							.unwrap_or_default();
						let title = payload["title"]
							.as_str()
							.or_else(|| payload["name"].as_str())
							.unwrap_or(&resource.identity_key)
							.to_owned();
						let link = payload["url"]
							.as_str()
							.and_then(|url| Url::parse(url).ok())
							.filter(|url| {
								matches!(url.scheme(), "http" | "https")
									&& url.username().is_empty()
									&& url.password().is_none()
							});

						list = list.child(
							gpui::div()
								.p_2()
								.rounded(gpui::px(6.))
								.bg(gpui::rgba(0xffffff06))
								.child(title)
								.child(if resource.payload_omitted {
									"Resource details are unavailable for display.".into()
								} else {
									resource.payload_json.clone()
								}),
						);

						if let Some(link) = link {
							list = list.child(resource_button(
								format!("resource-open-{}", resource.id),
								"Open link".into(),
								cx,
								move |_, cx| cx.open_url(link.as_str()),
							));
						}

						list = list.child(resource_button(
							format!("resource-remove-{}", resource.id),
							"Remove association".into(),
							cx,
							move |s, cx| s.remove_resource_association(&owner, &kind, &key, cx),
						));
					}

					list
				},
			};

			panel = panel.child(
				gpui::div()
					.id("agent-resources-body")
					.debug_selector(|| "agent-resources-body".into())
					.max_h(gpui::px(280.))
					.overflow_y_scroll()
					.child(body),
			);
		}

		let work = work.to_owned();

		panel
			.on_action(cx.listener(move |s, _: &SubmitComposer, _, cx| {
				cx.stop_propagation();
				s.add_resource_link(&work, cx);
			}))
			.into_any_element()
	}

	fn remove_resource_association(
		&mut self,
		owner: &str,
		kind: &str,
		key: &str,
		cx: &mut Context<Self>,
	) {
		let (Ok(work_id), Ok(attachment_type), Ok(identity_key)) = (
			EntityId::new(owner.to_owned()),
			WireText::new(kind.to_owned()),
			WireText::new(key.to_owned()),
		) else {
			return;
		};

		self.change_resource(
			owner,
			AgentActionDto::RemoveResource { work_id, attachment_type, identity_key },
			cx,
		);
	}

	fn resource_editor(&self, work: &str, cx: &mut Context<Self>) -> Div {
		let add_work = work.to_owned();

		gpui::div()
			.flex()
			.flex_col()
			.gap_2()
			.child(gpui::div().h(gpui::px(40.)).child(self.resource_title.clone()))
			.child(gpui::div().h(gpui::px(40.)).child(self.resource_url.clone()))
			.child(resource_button(
				"resource-add-link".into(),
				"Add link".into(),
				cx,
				move |s, cx| s.add_resource_link(&add_work, cx),
			))
	}

	fn add_resource_link(&mut self, work: &str, cx: &mut Context<Self>) {
		let (Ok(work_id), Ok(title), Ok(url)) = (
			EntityId::new(work.to_owned()),
			WireText::new(self.resource_title.read(cx).content().to_owned()),
			WireText::new(self.resource_url.read(cx).content().to_owned()),
		) else {
			self.resource_feedback = "Enter a title and a link within the text limit.".into();

			cx.notify();

			return;
		};

		self.change_resource(work, AgentActionDto::AddResourceLink { work_id, title, url }, cx);
	}

	fn change_resource(&mut self, work: &str, action: AgentActionDto, cx: &mut Context<Self>) {
		if self.resource_mutation_task.is_some() || self.selected.as_deref() != Some(work) {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.resource_feedback = "No service profile is configured.".into();

			cx.notify();

			return;
		};

		self.resources_task = None;

		let work = work.to_owned();
		let key = IdempotencyKey::new(agent_surface::unique_command()).expect("bounded identity");

		self.resource_feedback = "Waiting for resource confirmation…".into();

		let read_work = work.clone();
		let expected = action.clone();
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let result = runtime.block_on(client.execute(action, key)).ok();
			let readback = runtime.block_on(client.resources(EntityId::new(read_work).ok()?)).ok();

			Some((result, readback))
		});

		self.resource_mutation_task=Some(cx.spawn(async move |surface,cx| {
            let (result,readback)=request.await.unwrap_or((None,None));
            let observed=readback.as_ref().is_some_and(|resources|resource_change_observed(&expected,resources));
            let _=surface.update(cx,|s,cx| {
                if s.selected.as_deref()!=Some(work.as_str()) {return;}

                s.resource_mutation_task=None;

                s.resource_feedback=if observed {
                    "The current resource list confirms the requested state."
                } else {match result {
                    Some(AgentCommandResponse::Accepted {..}) => "Saved, but the refreshed list does not yet confirm the state. Check it before making another change.",
                    Some(AgentCommandResponse::Rejected {..}) => "The resource change was not accepted. Check the link and task connection.",
                    _ => "The change is not confirmed. Check the refreshed resource list before trying again.",
                }}.into();

                if s.resources.as_ref().is_some_and(|(owner,_)|owner==&work) {
                    s.resources=None;

                    s.toggle_resources(&work,cx);
                    s.resources=Some((work,Some(readback.unwrap_or(AgentResourcesResult::Unavailable))));
                }

                cx.notify();
            });
        }));

		cx.notify();
	}

	pub(super) fn toggle_resources(&mut self, work: &str, cx: &mut Context<Self>) {
		self.resources_task = None;

		if self.resources.as_ref().is_some_and(|(owner, _)| owner == work) {
			self.resources = None;

			cx.notify();

			return;
		}

		self.resources = Some((work.into(), None));

		let Some(profile) = self.profile.clone() else {
			self.resources = Some((work.into(), Some(AgentResourcesResult::Unavailable)));

			cx.notify();

			return;
		};
		let work = work.to_owned();

		self.resources_task = Some(cx.spawn(async move |surface, cx| {
			loop {
				let profile = profile.clone();
				let requested = work.clone();
				let result = cx
					.background_executor()
					.spawn(async move {
						let runtime = Builder::new_current_thread().enable_all().build().ok()?;

						runtime
							.block_on(
								AgentClient::new(profile).resources(EntityId::new(requested).ok()?),
							)
							.ok()
					})
					.await
					.unwrap_or(AgentResourcesResult::Unavailable);
				let keep = surface
					.update(cx, |s, cx| {
						if s.selected.as_deref() != Some(work.as_str())
							|| !s.resources.as_ref().is_some_and(|(owner, _)| owner == &work)
						{
							return false;
						}

						let supported = !matches!(
							result,
							AgentResourcesResult::Unsupported
								| AgentResourcesResult::CapacityExceeded
						);

						s.resources = Some((work.clone(), Some(result)));

						cx.notify();

						supported
					})
					.unwrap_or(false);

				if !keep {
					break;
				}

				cx.background_executor().timer(Duration::from_secs(5)).await;
			}
		}));

		cx.notify();
	}
}

fn resource_change_observed(action: &AgentActionDto, result: &AgentResourcesResult) -> bool {
	let AgentResourcesResult::Available { resources } = result else {
		return false;
	};

	match action {
		AgentActionDto::AddResourceLink { url, .. } => {
			let Ok(url) = Url::parse(url.as_str()) else {
				return false;
			};

			resources.iter().any(|resource| {
				resource.attachment_type == "decodex.link"
					&& !resource.payload_omitted
					&& serde_json::from_str::<Value>(&resource.payload_json)
						.ok()
						.is_some_and(|payload| payload["url"].as_str() == Some(url.as_str()))
			})
		},
		AgentActionDto::RemoveResource { attachment_type, identity_key, .. } =>
			!resources.iter().any(|resource| {
				resource.attachment_type == attachment_type.as_str()
					&& resource.identity_key == identity_key.as_str()
			}),
		_ => false,
	}
}

fn resource_button(
	id: String,
	label: String,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let action = Rc::new(action);
	let click = action.clone();

	gpui::div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id)
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label.clone())
		.p_2()
		.cursor_pointer()
		.on_click(cx.listener(move |s, _, _, cx| click(s, cx)))
		.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
			if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				cx.stop_propagation();

				action(s, cx);
			}
		}))
		.child(label)
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use std::{future, thread};

	use futures_util::{SinkExt as _, StreamExt as _};
	#[cfg(test)] use gpui::AppContext as _;
	use tokio_tungstenite::tungstenite::Message;

	#[cfg(test)]
	use crate::shell::agent_surface::resources::{
		AgentDispatchStateDto, AgentSnapshotResult, AgentWorkItemDto, AgentWorkStatusDto,
	};
	use crate::shell::agent_surface::{
		resources::{
			self, AgentActionDto, AgentResourcesResult, AgentSnapshotDto, AgentSurface, EntityId,
			WireText,
		},
		wire_test_support,
	};
	use decodex_protocol::{
		CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope,
		QueryResultPayload, ServerId, ServerMessage,
	};

	#[test]
	fn mutation_readback_requires_complete_list_and_exact_resource() {
		let add = AgentActionDto::AddResourceLink {
			work_id: EntityId::new("root").unwrap(),
			title: WireText::new("Review").unwrap(),
			url: WireText::new("https://EXAMPLE.test").unwrap(),
		};
		let remove = AgentActionDto::RemoveResource {
			work_id: EntityId::new("root").unwrap(),
			attachment_type: WireText::new("decodex.link").unwrap(),
			identity_key: WireText::new("key").unwrap(),
		};

		for unavailable in [
			AgentResourcesResult::Unavailable,
			AgentResourcesResult::Unsupported,
			AgentResourcesResult::CapacityExceeded,
		] {
			assert!(!resources::resource_change_observed(&add, &unavailable));
			assert!(!resources::resource_change_observed(&remove, &unavailable));
		}

		let empty = AgentResourcesResult::Available { resources: vec![] };

		assert!(!resources::resource_change_observed(&add, &empty));
		assert!(resources::resource_change_observed(&remove, &empty));

		let row = decodex_protocol::AgentResourceDto {
			id: "native".into(),
			attachment_type: "decodex.link".into(),
			identity_key: "key".into(),
			payload_json: r#"{"title":"Original","url":"https://example.test/"}"#.into(),
			payload_omitted: false,
			created_at: 1,
		};
		let present = AgentResourcesResult::Available { resources: vec![row.clone()] };

		assert!(resources::resource_change_observed(&add, &present));
		assert!(!resources::resource_change_observed(&remove, &present));

		let mut hidden = row;

		hidden.payload_omitted = true;

		assert!(!resources::resource_change_observed(
			&add,
			&AgentResourcesResult::Available { resources: vec![hidden] }
		));
	}

	#[gpui::test]
	fn resource_panel_does_not_treat_disconnect_as_empty_and_clears_on_navigation(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, _| {
			s.composer_menu = Some("agent-settings");
			s.composer_menu_content = Some("agent-settings");

			let work = |id: &str| AgentWorkItemDto {
				id: id.into(),
				parent_goal_id: None,
				kind: decodex_protocol::AgentWorkKindDto::Goal,
				title: id.into(),
				codex_thread_id: Some(format!("thread-{id}")),
				active_turn_id: None,
				dispatch_state: AgentDispatchStateDto::Idle,
				status: AgentWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			};

			s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				context_references: vec![],
				connection_initializing: false,
				runtime_source: None,
				workspaces: vec![],
				work_items: vec![work("root"), work("other")],
				dependencies: vec![],
				pending_events: vec![],
			})));
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
			window.draw(cx).clear(cx);
		});

		for _ in 0..2 {
			thread::sleep(std::time::Duration::from_millis(200));

			visual.update(|window, cx| {
				window.draw(cx).clear(cx);
			});
		}

		let bounds = visual.debug_bounds("agent-resources-toggle").expect("task resource control");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert!(
				matches!(&s.resources,Some((owner,Some(AgentResourcesResult::Unavailable))) if owner=="root")
			);
			assert!(s.resources_task.is_none());

			s.open_page("other", cx);

			assert!(s.resources.is_none());
			assert!(s.resources_task.is_none());
		});
	}

	#[gpui::test]
	fn ordinary_refresh_keeps_resource_mutation_readback_without_retry(
		cx: &mut gpui::TestAppContext,
	) {
		for remove in [false, true] {
			let (_dir, profile, server) = wire_test_support::fixture(move |listener| async move {
				for index in 0..2 {
					let mut socket = wire_test_support::accept(&listener).await;
					let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
						panic!("text request")
					};
					let request: ClientMessage = serde_json::from_str(&text).unwrap();

					if index == 0 {
						let ClientMessage::Command(command) = request else {
							panic!("one resource command")
						};
						let CommandPayload::Agent { action } = command.payload else {
							panic!("Agent action")
						};

						assert!(
							matches!(&*action, AgentActionDto::RemoveResource { work_id, .. } if remove && work_id.as_str() == "agent")
								|| matches!(&*action, AgentActionDto::AddResourceLink { work_id, .. } if !remove && work_id.as_str() == "agent")
						);

						socket.close(None).await.unwrap();

						continue;
					}

					let ClientMessage::Query(query) = request else {
						panic!("readback, never retry")
					};

					assert!(
						matches!(query.payload, QueryPayload::GetAgentResources { ref work_id } if work_id.as_str() == "agent")
					);

					let response = ServerMessage::QueryResult(QueryResultEnvelope {
						version: CURRENT_VERSION,
						server_id: ServerId::new(super::super::wire_test_support::SERVER).unwrap(),
						query_id: query.query_id,
						payload: QueryResultPayload::AgentResources(
							AgentResourcesResult::Available { resources: vec![] },
						),
					});

					socket
						.send(Message::Text(serde_json::to_string(&response).unwrap().into()))
						.await
						.unwrap();
				}
			});
			let surface = cx.new(AgentSurface::new);

			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.profile = Some(profile);

				let action = if remove {
					AgentActionDto::RemoveResource {
						work_id: EntityId::new("agent").unwrap(),
						attachment_type: WireText::new("decodex.link").unwrap(),
						identity_key: WireText::new("key").unwrap(),
					}
				} else {
					AgentActionDto::AddResourceLink {
						work_id: EntityId::new("agent").unwrap(),
						title: WireText::new("Review").unwrap(),
						url: WireText::new("https://example.test/").unwrap(),
					}
				};

				s.change_resource("agent", action, cx);

				assert!(s.resource_mutation_task.is_some());

				s.generation += 1;

				s.apply_result(Ok(AgentSnapshotResult::Available(s.snapshot.clone().unwrap())));
			});

			cx.run_until_parked();
			server.join().unwrap();
			surface.read_with(cx, |s, _| {
				assert!(
					s.resource_mutation_task.is_none(),
					"refresh must not strand a completed mutation"
				);
				assert!(s.resource_feedback.contains(if remove {
					"confirms the requested state"
				} else {
					"not confirmed"
				}));
			});
		}
	}

	#[gpui::test]
	fn changed_resource_source_clears_inventory_and_keeps_pending_outcome_unknown(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		for pending in [false, true] {
			for change in ["thread", "source", "removed", "disconnect", "failed"] {
				surface.update(cx, |s, cx| {
					s.visual_workspace_fixture(cx);

					let original = s.snapshot.clone().unwrap();

					s.resources = (!pending).then(|| {
						(
							"agent".into(),
							Some(AgentResourcesResult::Available { resources: vec![] }),
						)
					});

					s.resource_feedback.clear();

					if pending {
						s.resource_mutation_task =
							Some(cx.spawn(async |_, _| future::pending::<()>().await));
					}

					let mut next = original.clone();

					match change {
						"thread" =>
							next.work_items
								.iter_mut()
								.find(|w| w.id == "agent")
								.unwrap()
								.codex_thread_id = Some("replacement".into()),
						"source" =>
							next.runtime_source = Some(EntityId::new("replacement-source").unwrap()),
						"removed" => next.work_items.retain(|w| w.id != "agent"),
						"disconnect" => s.mark_stale(cx),
						"failed" => s.apply_result(Err(())),
						_ => unreachable!(),
					}

					if !matches!(change, "disconnect" | "failed") {
						s.apply_result(Ok(AgentSnapshotResult::Available(next)));
					}

					s.apply_result(Ok(AgentSnapshotResult::Available(original)));

					assert!(s.resources.is_none(), "old inventory after {change}");
					assert!(s.resource_mutation_task.is_none(), "old mutation after {change}");

					if pending {
						assert!(
							s.resource_feedback.contains("unconfirmed"),
							"cancellation is not proof of non-delivery"
						);
					}
				});
			}
		}
	}
}
