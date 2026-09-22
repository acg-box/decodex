//! Native task resource inspection, refreshed only while the selected panel is open.
use super::*;
use decodex_protocol::ChiefResourcesResult;

impl ChiefSurface {
	pub(super) fn resources_panel(&self, work: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
		let opened = self.resources.as_ref().filter(|(owner, _)| owner == work);
		let click = work.to_owned();
		let key = click.clone();
		let mut panel = div().flex().flex_col().gap_2().child(
			div()
				.id("chief-resources-toggle")
				.debug_selector(|| "chief-resources-toggle".into())
				.role(Role::Button)
				.tab_index(0)
				.aria_label("Task resources")
				.aria_expanded(opened.is_some())
				.cursor_pointer()
				.on_click(cx.listener(move |s, _, _, cx| s.toggle_resources(&click, cx)))
				.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
					if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
						cx.stop_propagation();
						s.toggle_resources(&key, cx);
					}
				}))
				.child("Task resources"),
		);
		if let Some((_, result)) = opened {
			let body = match result {
				None => div().child("Loading task resources…"),
				Some(ChiefResourcesResult::Unsupported) =>
					div().child("This Codex provider does not support task resources."),
				Some(ChiefResourcesResult::Unavailable) =>
					div().child("Task resources are unavailable. Retrying…"),
				Some(ChiefResourcesResult::CapacityExceeded) =>
					div().child("The resource list exceeds the display limit."),
				Some(ChiefResourcesResult::Available { resources }) => {
					let mut list = self.resource_editor(work, cx);
					if resources.is_empty() {
						list = list.child("No task resources.");
					}
					for resource in resources {
						let owner = work.to_owned();
						let kind = resource.attachment_type.clone();
						let key = resource.identity_key.clone();
						let payload =
							serde_json::from_str::<serde_json::Value>(&resource.payload_json)
								.unwrap_or_default();
						let title = payload["title"]
							.as_str()
							.or_else(|| payload["name"].as_str())
							.unwrap_or(&resource.identity_key)
							.to_owned();
						let link = payload["url"]
							.as_str()
							.and_then(|url| reqwest::Url::parse(url).ok())
							.filter(|url| {
								matches!(url.scheme(), "http" | "https")
									&& url.username().is_empty()
									&& url.password().is_none()
							});
						list = list.child(
							div().p_2().rounded(px(6.)).bg(rgba(0xffffff06)).child(title).child(
								if resource.payload_omitted {
									"Resource details are unavailable for display.".into()
								} else {
									resource.payload_json.clone()
								},
							),
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
							move |s, cx| {
								let (Ok(work_id), Ok(attachment_type), Ok(identity_key)) = (
									EntityId::new(owner.clone()),
									WireText::new(kind.clone()),
									WireText::new(key.clone()),
								) else {
									return;
								};
								s.change_resource(
									&owner,
									ChiefActionDto::RemoveResource {
										work_id,
										attachment_type,
										identity_key,
									},
									cx,
								);
							},
						));
					}
					list
				},
			};
			panel = panel.child(
				div()
					.id("chief-resources-body")
					.debug_selector(|| "chief-resources-body".into())
					.max_h(px(280.))
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

	fn resource_editor(&self, work: &str, cx: &mut Context<Self>) -> gpui::Div {
		let add_work = work.to_owned();
		div()
			.flex()
			.flex_col()
			.gap_2()
			.child(div().h(px(40.)).child(self.resource_title.clone()))
			.child(div().h(px(40.)).child(self.resource_url.clone()))
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
		self.change_resource(work, ChiefActionDto::AddResourceLink { work_id, title, url }, cx);
	}

	fn change_resource(&mut self, work: &str, action: ChiefActionDto, cx: &mut Context<Self>) {
		if self.resource_mutation_task.is_some() || self.selected.as_deref() != Some(work) {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			self.resource_feedback = "No service profile is configured.".into();
			cx.notify();
			return;
		};
		self.resources_task = None;
		let generation = self.generation;
		let work = work.to_owned();
		let key = IdempotencyKey::new(unique_command()).expect("bounded identity");
		self.resource_feedback = "Waiting for resource confirmation…".into();

		let read_work = work.clone();
		let expected = action.clone();
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let result = runtime.block_on(client.execute(action, key)).ok();
			let readback = runtime.block_on(client.resources(EntityId::new(read_work).ok()?)).ok();
			Some((result, readback))
		});
		self.resource_mutation_task=Some(cx.spawn(async move |surface,cx| {
            let (result,readback)=request.await.unwrap_or((None,None));
            let observed=readback.as_ref().is_some_and(|resources|resource_change_observed(&expected,resources));
            let _=surface.update(cx,|s,cx| {
                if s.generation!=generation || s.selected.as_deref()!=Some(work.as_str()) {return;}
                s.resource_mutation_task=None;
                s.resource_feedback=if observed {
                    "The current resource list confirms the requested state."
                } else {match result {
                    Some(ChiefCommandResponse::Accepted {..}) => "Saved, but the refreshed list does not yet confirm the state. Check it before making another change.",
                    Some(ChiefCommandResponse::Rejected {..}) => "The resource change was not accepted. Check the link and task connection.",
                    _ => "The change is not confirmed. Check the refreshed resource list before trying again.",
                }}.into();
                if s.resources.as_ref().is_some_and(|(owner,_)|owner==&work) {
                    s.resources=None;
                    s.toggle_resources(&work,cx);
                    s.resources=Some((work,Some(readback.unwrap_or(ChiefResourcesResult::Unavailable))));
                }
                cx.notify();
            });
        }));
		cx.notify();
	}

	fn toggle_resources(&mut self, work: &str, cx: &mut Context<Self>) {
		self.resources_task = None;
		if self.resources.as_ref().is_some_and(|(owner, _)| owner == work) {
			self.resources = None;
			cx.notify();
			return;
		}
		self.resources = Some((work.into(), None));
		let Some(profile) = self.profile.clone() else {
			self.resources = Some((work.into(), Some(ChiefResourcesResult::Unavailable)));
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
						let runtime = tokio::runtime::Builder::new_current_thread()
							.enable_all()
							.build()
							.ok()?;
						runtime
							.block_on(
								ChiefClient::new(profile).resources(EntityId::new(requested).ok()?),
							)
							.ok()
					})
					.await
					.unwrap_or(ChiefResourcesResult::Unavailable);
				let keep = surface
					.update(cx, |s, cx| {
						if s.selected.as_deref() != Some(work.as_str())
							|| !s.resources.as_ref().is_some_and(|(owner, _)| owner == &work)
						{
							return false;
						}
						let supported = !matches!(
							result,
							ChiefResourcesResult::Unsupported
								| ChiefResourcesResult::CapacityExceeded
						);
						s.resources = Some((work.clone(), Some(result)));
						cx.notify();
						supported
					})
					.unwrap_or(false);
				if !keep {
					break;
				}
				cx.background_executor().timer(std::time::Duration::from_secs(5)).await;
			}
		}));
		cx.notify();
	}
}

fn resource_change_observed(action: &ChiefActionDto, result: &ChiefResourcesResult) -> bool {
	let ChiefResourcesResult::Available { resources } = result else {
		return false;
	};
	match action {
		ChiefActionDto::AddResourceLink { url, .. } => {
			let Ok(url) = reqwest::Url::parse(url.as_str()) else {
				return false;
			};
			resources.iter().any(|resource| {
				resource.attachment_type == "decodex.link"
					&& !resource.payload_omitted
					&& serde_json::from_str::<serde_json::Value>(&resource.payload_json)
						.ok()
						.is_some_and(|payload| payload["url"].as_str() == Some(url.as_str()))
			})
		},
		ChiefActionDto::RemoveResource { attachment_type, identity_key, .. } =>
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
	cx: &mut Context<ChiefSurface>,
	action: impl Fn(&mut ChiefSurface, &mut Context<ChiefSurface>) + 'static,
) -> gpui::AnyElement {
	let action = std::rc::Rc::new(action);
	let click = action.clone();
	div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id)
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label.clone())
		.p_2()
		.cursor_pointer()
		.on_click(cx.listener(move |s, _, _, cx| click(s, cx)))
		.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
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
	use super::*;
	#[test]
	fn mutation_readback_requires_complete_list_and_exact_resource() {
		let add = ChiefActionDto::AddResourceLink {
			work_id: EntityId::new("root").unwrap(),
			title: WireText::new("Review").unwrap(),
			url: WireText::new("https://EXAMPLE.test").unwrap(),
		};
		let remove = ChiefActionDto::RemoveResource {
			work_id: EntityId::new("root").unwrap(),
			attachment_type: WireText::new("decodex.link").unwrap(),
			identity_key: WireText::new("key").unwrap(),
		};
		for unavailable in [
			ChiefResourcesResult::Unavailable,
			ChiefResourcesResult::Unsupported,
			ChiefResourcesResult::CapacityExceeded,
		] {
			assert!(!resource_change_observed(&add, &unavailable));
			assert!(!resource_change_observed(&remove, &unavailable));
		}
		let empty = ChiefResourcesResult::Available { resources: vec![] };
		assert!(!resource_change_observed(&add, &empty));
		assert!(resource_change_observed(&remove, &empty));
		let row = decodex_protocol::ChiefResourceDto {
			id: "native".into(),
			attachment_type: "decodex.link".into(),
			identity_key: "key".into(),
			payload_json: r#"{"title":"Original","url":"https://example.test/"}"#.into(),
			payload_omitted: false,
			created_at: 1,
		};
		let present = ChiefResourcesResult::Available { resources: vec![row.clone()] };
		assert!(resource_change_observed(&add, &present));
		assert!(!resource_change_observed(&remove, &present));
		let mut hidden = row;
		hidden.payload_omitted = true;
		assert!(!resource_change_observed(
			&add,
			&ChiefResourcesResult::Available { resources: vec![hidden] }
		));
	}

	#[gpui::test]
	fn resource_panel_does_not_treat_disconnect_as_empty_and_clears_on_navigation(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, _| {
			let work = |id: &str| ChiefWorkItemDto {
				id: id.into(),
				parent_goal_id: None,
				kind: decodex_protocol::ChiefWorkKindDto::Goal,
				title: id.into(),
				codex_thread_id: Some(format!("thread-{id}")),
				active_turn_id: None,
				dispatch_state: ChiefDispatchStateDto::Idle,
				status: ChiefWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			};
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				workspaces: vec![],
				work_items: vec![work("root"), work("other")],
				dependencies: vec![],
				pending_events: vec![],
			})));
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.), px(1200.)));
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("chief-resources-toggle").expect("task resource control");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert!(
				matches!(&s.resources,Some((owner,Some(ChiefResourcesResult::Unavailable))) if owner=="root")
			);
			assert!(s.resources_task.is_none());
			s.open_page("other", cx);
			assert!(s.resources.is_none());
			assert!(s.resources_task.is_none());
		});
	}
}
