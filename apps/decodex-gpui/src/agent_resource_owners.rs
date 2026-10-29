//! Related conversations for a selected registered native resource.
use gpui::AnyElement;
use tokio::runtime::Builder;

use crate::shell::agent_surface::{
	AgentClient, AgentSurface, Context, EntityId, IntoElement, ParentElement, Styled, Task,
	WireText,
};
use decodex_protocol::{AgentResourceDto, ResourceOwnersRequest, ResourceOwnersResult};

#[derive(Default)]
pub(super) struct Panel {
	request: Option<ResourceOwnersRequest>,
	result: Option<ResourceOwnersResult>,
	task: Option<Task<()>>,
	epoch: u64,
}

impl AgentSurface {
	pub(super) fn reset_resource_owners(&mut self) {
		self.related_resources =
			Panel { epoch: self.related_resources.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn resource_owner_rows(
		&self,
		work: &str,
		resource: &AgentResourceDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let request = ResourceOwnersRequest {
			work_id: EntityId::new(work).expect("selected work"),
			attachment_type: WireText::new(&resource.attachment_type)
				.expect("native resource type"),
			identity_key: WireText::new(&resource.identity_key).expect("native resource key"),
			archived: None,
			cursor: None,
		};
		let selected = request.clone();
		let mut row = gpui::div().flex().flex_col().gap_1().child(self.workspace_action(
			format!("resource-related-{}", resource.id),
			"Related conversations".into(),
			move |s, cx| s.load_resource_owners(selected.clone(), cx),
			cx,
		));
		if self.related_resources.request.as_ref().is_none_or(|current| {
			current.work_id != request.work_id
				|| current.identity_key != request.identity_key
				|| current.attachment_type != request.attachment_type
		}) {
			return row.into_any_element();
		}
		match &self.related_resources.result {
			Some(ResourceOwnersResult::Available { owners, next_cursor, request, .. }) => {
				for (label, filter) in
					[("All", None), ("Current", Some(false)), ("Archived", Some(true))]
				{
					if request.archived != filter {
						let mut filtered = request.clone();
						filtered.archived = filter;
						filtered.cursor = None;
						row = row.child(self.workspace_action(
							format!("resource-filter-{label}"),
							label.into(),
							move |s, cx| s.load_resource_owners(filtered.clone(), cx),
							cx,
						));
					}
				}
				if owners.is_empty() {
					row = row.child("No matching conversations on this page.");
				}
				for owner in owners {
					let account = owner
						.account_id
						.as_ref()
						.and_then(|id| {
							self.accounts
								.iter()
								.find(|(known, _)| known == id.as_str())
								.map(|(_, label)| label.as_str())
						})
						.unwrap_or("Account not retained");
					row = row.child(format!(
						"{} · {account}{}",
						owner.title,
						if owner.archived { " · Archived" } else { "" }
					));
					if let Some(work) = self.resource_owner_destination(owner) {
						let target = work.to_owned();
						row = row.child(self.workspace_action(
							format!("resource-open-{target}"),
							"Open conversation".into(),
							move |s, cx| s.open_page(&target, cx),
							cx,
						));
					} else {
						row = row.child(crate::shell::agent_surface::markdown::copy_button(
							&format!("resource-owner-{}", owner.thread_id.as_str()),
							"Copy conversation reference",
							owner.thread_id.as_str().to_owned(),
						));
					}
				}
				row = row.child(
					"Registered associations in this native store. Other uploads are not searched.",
				);
				if let Some(next) = next_cursor {
					let mut next_request = request.clone();
					next_request.cursor = Some(next.clone());
					row = row.child(self.workspace_action(
						"resource-related-next".into(),
						"Next conversations".into(),
						move |s, cx| s.load_resource_owners(next_request.clone(), cx),
						cx,
					));
				}
			},
			Some(ResourceOwnersResult::Unsupported) =>
				row = row.child("Related conversations are not supported by this native store."),
			Some(ResourceOwnersResult::Unavailable) =>
				row = row.child("Related conversations unavailable. Refresh to retry."),
			None => row = row.child(crate::ui_loading::loading("Finding related conversations")),
		}
		row.into_any_element()
	}

	fn resource_owner_destination<'a>(
		&self,
		owner: &'a decodex_protocol::ResourceOwner,
	) -> Option<&'a str> {
		let work = owner.work_id.as_ref()?;
		self.snapshot
			.as_ref()?
			.work_items
			.iter()
			.any(|current| {
				current.id == work.as_str()
					&& current.codex_thread_id.as_deref() == Some(owner.thread_id.as_str())
			})
			.then_some(work.as_str())
	}

	fn load_resource_owners(&mut self, request: ResourceOwnersRequest, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let source = self.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.clone());
		if source.is_none() || self.selected.as_deref() != Some(request.work_id.as_str()) {
			return;
		}
		self.reset_resource_owners();
		let epoch = self.related_resources.epoch;
		self.related_resources.request = Some(request.clone());
		let expected = request.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(AgentClient::new(profile).resource_owners(request)).ok()
		});
		self.related_resources.task = Some(cx.spawn(async move |s, cx| {
			let result = future.await.unwrap_or(ResourceOwnersResult::Unavailable);
			let _ = s.update(cx, |s, cx| {
				if s.selected.as_deref() != Some(expected.work_id.as_str())
					|| s.related_resources.epoch != epoch
					|| s.related_resources.request.as_ref() != Some(&expected)
					|| s.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.as_ref())
						!= source.as_ref()
				{
					return;
				}
				s.related_resources.task = None;
				s.related_resources.result = Some(result);
				cx.notify();
			});
		}));
		cx.notify();
	}
}

#[cfg(test)]
mod tests {
	use super::{AgentSurface, EntityId, ResourceOwnersRequest, ResourceOwnersResult, WireText};
	use crate::shell::agent_surface::wire_test_support;
	use decodex_protocol::{ClientMessage, QueryPayload, QueryResultPayload};
	use futures_util::StreamExt as _;
	use gpui::AppContext as _;
	use tokio_tungstenite::tungstenite::Message;

	struct Quiet(gpui::Entity<AgentSurface>);
	impl gpui::Render for Quiet {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			_: &mut gpui::Context<Self>,
		) -> impl gpui::IntoElement {
			gpui::div()
		}
	}
	struct OwnersView(gpui::Entity<AgentSurface>);
	impl gpui::Render for OwnersView {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			cx: &mut gpui::Context<Self>,
		) -> impl gpui::IntoElement {
			self.0.update(cx, |s, cx| {
				s.resource_owner_rows(
					"agent",
					&decodex_protocol::AgentResourceDto {
						id: "resource".into(),
						attachment_type: "decodex.link".into(),
						identity_key: "exact-resource".into(),
						payload_json: "{}".into(),
						payload_omitted: false,
						created_at: 1,
					},
					cx,
				)
			})
		}
	}

	#[gpui::test]
	fn related_navigation_requires_a_current_local_thread(cx: &mut gpui::TestAppContext) {
		for (work, thread, can_open, open_selector, copy_selector) in [
			("agent", "thread", true, "resource-open-agent", "resource-owner-thread"),
			(
				"archived",
				"old-thread",
				false,
				"resource-open-archived",
				"resource-owner-old-thread",
			),
			(
				"agent",
				"replaced-thread",
				false,
				"resource-open-agent",
				"resource-owner-replaced-thread",
			),
		] {
			let (_, visual) = cx.add_window_view(|_, cx| {
				let surface = cx.new(AgentSurface::new);
				surface.update(cx, |s, cx| {
					s.visual_workspace_fixture(cx);
					s.snapshot
						.as_mut()
						.unwrap()
						.work_items
						.iter_mut()
						.find(|w| w.id == "agent")
						.unwrap()
						.codex_thread_id = Some("thread".into());
					let request = ResourceOwnersRequest {
						work_id: EntityId::new("agent").unwrap(),
						attachment_type: WireText::new("decodex.link").unwrap(),
						identity_key: WireText::new("exact-resource").unwrap(),
						archived: None,
						cursor: None,
					};
					s.related_resources.request = Some(request.clone());
					s.related_resources.result = Some(ResourceOwnersResult::Available {
						request,
						source: EntityId::new("source").unwrap(),
						next_cursor: None,
						owners: vec![decodex_protocol::ResourceOwner {
							thread_id: WireText::new(thread).unwrap(),
							archived: !can_open,
							work_id: Some(EntityId::new(work).unwrap()),
							title: "Known conversation".into(),
							account_id: None,
						}],
					});
				});
				OwnersView(surface)
			});
			visual.update(crate::ui_frame::draw_frame);
			assert_eq!(visual.debug_bounds(open_selector).is_some(), can_open);
			let copy = visual.debug_bounds(copy_selector);
			assert_eq!(copy.is_some(), !can_open);
			if let Some(copy) = copy {
				visual.simulate_click(copy.center(), Default::default());
				visual.update(|_, cx| {
					assert_eq!(
						cx.read_from_clipboard().and_then(|v| v.text()).as_deref(),
						Some(thread)
					)
				});
			}
		}
	}

	#[gpui::test]
	fn related_conversations_keep_filter_cursor_and_reject_a_changed_source(
		cx: &mut gpui::TestAppContext,
	) {
		for changed in [false, true] {
			let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
				let mut socket = wire_test_support::accept(&listener).await;
				let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
					panic!("query")
				};
				let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
					panic!("query")
				};
				let QueryPayload::GetResourceOwners { request } = query.payload else {
					panic!("resource owners")
				};
				assert_eq!(request.identity_key.as_str(), "exact-resource");
				assert_eq!(request.archived, Some(true));
				assert_eq!(request.cursor.as_ref().unwrap().as_str(), "native-page");
				wire_test_support::answer_query(
					&mut socket,
					query.query_id,
					QueryResultPayload::ResourceOwners(ResourceOwnersResult::Available {
						request,
						source: EntityId::new("source").unwrap(),
						owners: vec![],
						next_cursor: None,
					}),
				)
				.await;
			});
			let (view, visual) = cx.add_window_view(|_, cx| Quiet(cx.new(AgentSurface::new)));
			let surface = view.read_with(visual, |view, _| view.0.clone());
			surface.update(visual, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.profile = Some(profile);
				s.snapshot.as_mut().unwrap().runtime_source =
					Some(EntityId::new("source").unwrap());
				s.load_resource_owners(
					ResourceOwnersRequest {
						work_id: EntityId::new("agent").unwrap(),
						attachment_type: WireText::new("decodex.link").unwrap(),
						identity_key: WireText::new("exact-resource").unwrap(),
						archived: Some(true),
						cursor: Some(WireText::new("native-page").unwrap()),
					},
					cx,
				);
				if changed {
					s.snapshot.as_mut().unwrap().runtime_source =
						Some(EntityId::new("other").unwrap());
				}
			});
			visual.run_until_parked();
			server.join().unwrap();
			surface.read_with(visual, |s, _| {
				assert_eq!(s.related_resources.result.is_some(), !changed)
			});
		}
	}
}
