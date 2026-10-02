//! Task-scoped native integration observations; discovery never grants capability.
use std::rc::Rc;

use gpui::{AnyElement, KeyDownEvent};
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotResult;
use crate::{
	shell::{
		agent_surface,
		agent_surface::{
			AgentClient, AgentSnapshotDto, AgentSurface, Context, EntityId, InteractiveElement,
			IntoElement, ParentElement, Role, SharedString, StatefulInteractiveElement, Styled,
		},
	},
	ui_loading,
};
use decodex_protocol::{
	AgentAppInventory, AgentIntegrationsResult, AgentMcpInventory, AgentPluginInventory,
};

impl AgentSurface {
	pub(super) fn reset_integrations(&mut self) {
		self.integrations_task = None;
		self.integrations = None;
	}

	pub(super) fn invalidate_integrations(&mut self, next: &AgentSnapshotDto) {
		let Some((work, _)) = &self.integrations else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if !matches!((before, after), (Some(a), Some(b)) if a.codex_thread_id == b.codex_thread_id)
			|| self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
		{
			self.reset_integrations();
		}
	}

	#[cfg(feature = "visual-capture")]
	#[allow(
		dead_code,
		reason = "Used by the separate capture binary; this module also builds into the main binary"
	)]
	pub(crate) fn visual_integrations(&mut self, cx: &mut Context<Self>) {
		self.visual_workspace_fixture(cx);

		self.profile = None;
		self.graph_visible = false;
		self.composer_menu = Some("agent-settings");
		self.composer_menu_content = Some("agent-settings");

		let apps = [
			("calendar-disabled", false, false),
			("calendar-empty", true, false),
			("calendar-ready", true, true),
		]
		.into_iter()
		.map(|(id, enabled, callable)| decodex_protocol::AgentAppStatusDto {
			id: id.into(),
			runtime_name: Some("Calendar".into()),
			enabled,
			callable,
		})
		.collect();

		self.integrations = Some((
			self.selected.clone().expect("fixture selection"),
			Some(AgentIntegrationsResult::Available {
				cwd: "/isolated/integration-fixture".into(),
				mcp: AgentMcpInventory::Available {
					servers: vec![decodex_protocol::AgentMcpStatusDto {
						name: "Local MCP fixture".into(),
						plugin_id: None,
						presentation: None,
						runtime_status: Some("authenticationRequired".into()),
						auth_status: "notLoggedIn".into(),
						tool_count: 0,
						tools_error: Some("Synthetic discovery failure".into()),
						resource_count: 0,
						template_count: 0,
						advertised_capabilities: Some(vec![
							"tools".into(),
							"extensions/openai/settings".into(),
						]),
					}],
				},
				plugins: AgentPluginInventory::Available {
					plugins: vec![],
					errors: vec!["Synthetic marketplace failure".into()],
				},
				apps: AgentAppInventory::Available { apps },
			}),
		));

		cx.notify();
	}

	pub(super) fn integrations_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		let opened = self.integrations.as_ref().filter(|(owner, _)| owner == work);
		let work_id = work.to_owned();
		let mut panel = gpui::div().flex().flex_col().gap_2().child(integration_button(
			"integration-toggle",
			"Tool status",
			cx,
			move |s, cx| {
				if s.integrations.as_ref().is_some_and(|(owner, _)| owner == &work_id) {
					s.reset_app_exposure();
					s.reset_integrations();
					cx.notify();
				} else {
					s.load_integrations(&work_id, cx);
				}
			},
		));

		if let Some((_, result)) = opened {
			panel = panel.child(agent_surface::muted(
				"Configure plugins and connections in Codex for this account.",
			));

			let refresh = work.to_owned();

			panel = panel.child(integration_button(
				"integration-refresh",
				"Refresh status",
				cx,
				move |s, cx| s.load_integrations(&refresh, cx),
			));

			if let Some(AgentIntegrationsResult::Available {
				apps: AgentAppInventory::Available { apps },
				..
			}) = result
			{
				for (index, app) in apps.iter().enumerate() {
					let (owner, connector) = (work.to_owned(), app.id.clone());

					panel = panel.child(integration_button(
						format!("app-exposure-open-{index}"),
						format!(
							"Tool visibility for {} ({})",
							app.runtime_name.as_deref().unwrap_or(&app.id),
							app.id
						),
						cx,
						move |s, cx| s.update_app_exposure(&owner, &connector, false, cx),
					));
				}
			}

			panel = panel.child(self.app_exposure_panel(work, cx));

			let text = match result {
				None => ui_loading::loading("Loading tools and plugins").into_any_element(),
				Some(result) => gpui::div().child(integration_text(result)).into_any_element(),
			};

			panel = panel.child(
				gpui::div()
					.id("integration-status")
					.debug_selector(|| "integration-status".into())
					.max_h(gpui::px(300.))
					.overflow_y_scroll()
					.child(text),
			);
		}

		panel.into_any_element()
	}

	fn load_integrations(&mut self, work: &str, cx: &mut Context<Self>) {
		// Dropping the previous GPUI task cancels its completion before replacement.
		self.integrations_task = None;

		if !self.integrations.as_ref().is_some_and(|(owner, _)| owner == work) {
			self.integrations = Some((work.into(), None));
		}

		let Some(profile) = self.profile.clone() else {
			self.integrations = Some((work.into(), Some(AgentIntegrationsResult::Unavailable)));

			cx.notify();

			return;
		};
		let work = work.to_owned();
		let requested = work.clone();
		let query = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime
				.block_on(AgentClient::new(profile).integrations(EntityId::new(requested).ok()?))
				.ok()
		});

		self.integrations_task = Some(cx.spawn(async move |surface, cx| {
			let result = query.await.unwrap_or(AgentIntegrationsResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.selected.as_deref() == Some(work.as_str())
					&& s.integrations.as_ref().is_some_and(|(owner, _)| owner == &work)
				{
					s.integrations = Some((work, Some(result)));
					s.integrations_task = None;

					cx.notify();
				}
			});
		}));

		cx.notify();
	}
}

fn integration_text(result: &AgentIntegrationsResult) -> String {
	let AgentIntegrationsResult::Available { cwd, mcp, plugins, apps } = result else {
		return match result {
			AgentIntegrationsResult::CapacityExceeded =>
				"The complete integration inventory exceeds the display limit.",
			_ => "Integration status is unavailable for the current task and connection.",
		}
		.into();
	};
	let mut lines = vec![
		format!("Configured repository: {cwd}"),
        app_inventory_text(apps),
		"Plugin inventory uses the configured repository. MCP status reflects the loaded task; a running turn keeps its previous environment until the next turn.".into(),
	];

	match mcp {
		AgentMcpInventory::Available { servers } => {
			if servers.is_empty() {
				lines.push("No MCP servers were reported for this task.".into());
			}

			for server in servers {
				let runtime = match server.runtime_status.as_deref() {
					Some("notStarted") => "Not started",
					Some("starting") => "Connecting",
					Some("connected") => "Connected",
					Some("authenticationRequired") => "Sign-in required",
					Some("failed") => "Connection failed",
					Some("cancelled") => "Cancelled",
					Some("disabled") => "Disabled",
					_ => "Connection status unavailable",
				};
				let auth = match server.auth_status.as_str() {
					"notLoggedIn" => "Not signed in",
					"bearerToken" => "Bearer credentials configured",
					"oAuth" | "oauth" => "OAuth credentials recorded",
					"unsupported" => "No supported sign-in method",
					_ => "Authentication status unknown",
				};

				lines.push(format!("{} — {runtime}; {auth}", server.name));

				if let Some(presentation) = &server.presentation {
					lines.push(presentation.clone());
				}

				lines.push(match &server.advertised_capabilities {
					None => "Advertised capabilities: unavailable".into(),
					Some(names) if names.is_empty() => "Advertised capabilities: none".into(),
					Some(names) => format!("Advertised capabilities: {}", names.join(", ")),
				});

				if let Some(error) = &server.tools_error {
					lines.push(format!("Tool discovery failed: {error}"));
				} else {
					lines.push(format!(
						"Reported tools: {} (catalog may be cached)",
						server.tool_count
					));
				}
				if let Some(plugin) = &server.plugin_id {
					lines.push(format!("Plugin: {plugin}"));
				}

				lines.push(format!(
					"Reported resources: {}; templates: {}",
					server.resource_count, server.template_count
				));
			}
		},
		AgentMcpInventory::Unsupported =>
			lines.push("This provider does not support MCP status discovery.".into()),
		AgentMcpInventory::CapacityExceeded =>
			lines.push("MCP inventory exceeds the display limit.".into()),
		AgentMcpInventory::Unavailable => lines.push("MCP status could not be read.".into()),
	}
	match plugins {
		AgentPluginInventory::Available { plugins, errors } => {
			if plugins.is_empty() && errors.is_empty() {
				lines.push("No installed plugins were reported for this repository.".into());
			}

			for plugin in plugins {
				lines.push(format!(
					"{} — {}; {}; policy: {}",
					plugin.name,
					if plugin.installed { "Installed" } else { "Not installed" },
					if plugin.enabled {
						"Enabled in configuration"
					} else {
						"Disabled in configuration"
					},
					plugin.availability
				));

				if let Some(reason) = &plugin.disabled_reason {
					lines.push(format!("Unavailable reason: {reason}"));
				}
			}
			for error in errors {
				lines.push(format!("Plugin discovery incomplete: {error}"));
			}
		},
		AgentPluginInventory::Unsupported =>
			lines.push("This provider does not support installed-plugin discovery.".into()),
		AgentPluginInventory::CapacityExceeded =>
			lines.push("Plugin inventory exceeds the display limit.".into()),
		AgentPluginInventory::Unavailable =>
			lines.push("Plugin configuration could not be read.".into()),
	}

	lines.join("\n\n")
}

fn integration_button(
	id: impl Into<String>,
	label: impl Into<String>,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let id = id.into();
	let label = label.into();
	let action = Rc::new(action);
	let click = action.clone();

	gpui::div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id)
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label.clone())
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

fn app_inventory_text(inventory: &AgentAppInventory) -> String {
	match inventory {
		AgentAppInventory::Available { apps } if apps.is_empty() =>
			"No installed Apps were reported in the runtime snapshot.".into(),
		AgentAppInventory::Available { apps } => {
			let mut lines = vec!["Installed Apps (runtime snapshot):".to_owned()];

			for app in apps {
				let name = app.runtime_name.as_deref().unwrap_or(&app.id);
				let status = if !app.enabled {
					"Disabled by effective configuration"
				} else if app.callable {
					"Enabled; callable tools available"
				} else {
					"Enabled; no callable tools reported"
				};

				lines.push(format!("{name} ({}) — {status}", app.id));
			}

			lines.join("\n")
		},
		AgentAppInventory::Unsupported =>
			"This provider does not support installed Apps status.".into(),
		AgentAppInventory::Unavailable => "Installed Apps status could not be read.".into(),
		AgentAppInventory::CapacityExceeded =>
			"Installed Apps inventory exceeds the display limit.".into(),
	}
}

#[cfg(test)]
mod tests {
	use futures_util::{SinkExt as _, StreamExt as _};
	#[cfg(test)] use gpui::AppContext as _;
	use tokio_tungstenite::tungstenite::Message;

	#[cfg(test)] use crate::shell::agent_surface::integrations::AgentSnapshotResult;
	use crate::shell::agent_surface::{
		integrations::{
			self, AgentAppInventory, AgentIntegrationsResult, AgentMcpInventory,
			AgentPluginInventory, AgentSurface, EntityId,
		},
		wire_test_support,
	};
	use decodex_protocol::{
		CURRENT_VERSION, ClientMessage, QueryPayload, QueryResultEnvelope, QueryResultPayload,
		ServerId, ServerMessage,
	};

	#[test]
	fn incomplete_plugin_discovery_and_mcp_failure_never_render_as_empty_success() {
		let text = integrations::integration_text(&AgentIntegrationsResult::Available {
			apps: AgentAppInventory::Unavailable,
			cwd: "/repo".into(),
			mcp: AgentMcpInventory::Available {
				servers: vec![decodex_protocol::AgentMcpStatusDto {
					name: "test".into(),
					plugin_id: None,
					presentation: Some("Reference docs · 1.2".into()),
					runtime_status: Some("authenticationRequired".into()),
					auth_status: "notLoggedIn".into(),
					tool_count: 0,
					tools_error: Some("Provider discovery failed".into()),
					resource_count: 0,
					template_count: 0,
					advertised_capabilities: Some(vec![
						"tools".into(),
						"extensions/openai/settings".into(),
					]),
				}],
			},
			plugins: AgentPluginInventory::Available {
				plugins: vec![],
				errors: vec!["Invalid repository configuration".into()],
			},
		});

		assert!(text.contains("Reference docs · 1.2"));
		assert!(text.contains("Sign-in required"));
		assert!(text.contains("Tool discovery failed"));
		assert!(text.contains("Advertised capabilities: tools, extensions/openai/settings"));
		assert!(text.contains("Plugin discovery incomplete"));
		assert!(!text.contains("Reported tools: 0"));
		assert!(!text.contains("No installed plugins"));
		assert!(text.contains("Installed Apps status could not be read"));
		assert!(!text.contains("No installed Apps"));
	}

	#[test]
	fn apps_distinguish_policy_eligibility_from_installation_and_read_failure() {
		let apps = [("disabled", false, false), ("empty", true, false), ("ready", true, true)]
			.into_iter()
			.map(|(id, enabled, callable)| decodex_protocol::AgentAppStatusDto {
				id: id.into(),
				runtime_name: None,
				enabled,
				callable,
			})
			.collect();
		let text = integrations::app_inventory_text(&AgentAppInventory::Available { apps });

		assert!(text.contains("disabled (disabled) — Disabled by effective configuration"));
		assert!(text.contains("empty (empty) — Enabled; no callable tools reported"));
		assert!(text.contains("ready (ready) — Enabled; callable tools available"));

		for state in [
			AgentAppInventory::Unsupported,
			AgentAppInventory::Unavailable,
			AgentAppInventory::CapacityExceeded,
		] {
			assert!(!integrations::app_inventory_text(&state).contains("No installed Apps"));
		}
	}

	#[gpui::test]
	fn ordinary_refresh_keeps_integration_status_read(cx: &mut gpui::TestAppContext) {
		let (_dir, profile, server) = wire_test_support::fixture(|listener| async move {
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("text query")
			};
			let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
				panic!("read only")
			};

			assert!(
				matches!(query.payload, QueryPayload::GetAgentIntegrations { ref work_id } if work_id.as_str() == "agent")
			);

			let response = ServerMessage::QueryResult(QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(super::super::wire_test_support::SERVER).unwrap(),
				query_id: query.query_id,
				payload: QueryResultPayload::AgentIntegrations(
					AgentIntegrationsResult::CapacityExceeded,
				),
			});

			socket
				.send(Message::Text(serde_json::to_string(&response).unwrap().into()))
				.await
				.unwrap();
		});
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.profile = Some(profile);

			s.load_integrations("agent", cx);

			assert!(s.integrations_task.is_some());

			s.generation += 1;

			s.apply_result(Ok(AgentSnapshotResult::Available(s.snapshot.clone().unwrap())));
		});

		cx.run_until_parked();
		server.join().unwrap();
		surface.read_with(cx, |s, _| {
			assert!(s.integrations_task.is_none(), "refresh must not strand a completed read");
			assert!(matches!(
				s.integrations,
				Some((_, Some(AgentIntegrationsResult::CapacityExceeded)))
			));
		});
	}

	#[gpui::test]
	fn integration_status_does_not_survive_a_changed_or_disconnected_source(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		for change in ["thread", "source", "removed", "disconnect", "failed"] {
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				let original = s.snapshot.clone().unwrap();

				s.integrations =
					Some(("agent".into(), Some(AgentIntegrationsResult::CapacityExceeded)));

				let mut next = original.clone();

				match change {
					"thread" =>
						next.work_items
							.iter_mut()
							.find(|w| w.id == "agent")
							.unwrap()
							.codex_thread_id = Some("replacement-thread".into()),
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

				assert!(s.integrations.is_none(), "stale inventory returned after {change}");
			});
		}
	}
}
