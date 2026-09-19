//! Task-scoped native integration observations; discovery never grants capability.
use super::*;
use decodex_protocol::{ChiefIntegrationsResult, ChiefMcpInventory, ChiefPluginInventory};

impl ChiefSurface {
	pub(super) fn integrations_panel(
		&self,
		work: &str,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let opened = self.integrations.as_ref().filter(|(owner, _)| owner == work);
		let work_id = work.to_owned();
		let mut panel = div().flex().flex_col().gap_2().child(integration_button(
			"integration-toggle",
			"Tools and plugins",
			cx,
			move |s, cx| {
				if s.integrations.as_ref().is_some_and(|(owner, _)| owner == &work_id) {
					s.integrations = None;
					s.integrations_task = None;
					cx.notify();
				} else {
					s.load_integrations(&work_id, cx);
				}
			},
		));
		if let Some((_, result)) = opened {
			let refresh = work.to_owned();
			panel = panel.child(integration_button(
				"integration-refresh",
				"Refresh status",
				cx,
				move |s, cx| s.load_integrations(&refresh, cx),
			));
			let refresh_work = work.to_owned();
			panel=panel.child(integration_button("integration-reload","Sync plugins and reload MCP",cx,move |s,cx|s.refresh_native_integrations(&refresh_work,cx)))
                .child(muted("Updates shared plugin bundles and MCP configuration for loaded tasks. Connection status is checked separately."));
			if !self.integration_feedback.is_empty() {
				panel = panel.child(self.integration_feedback.clone());
			}
			if let Some(ChiefIntegrationsResult::Available {
				mcp: ChiefMcpInventory::Available { servers },
				..
			}) = result
			{
				for (index, server) in servers.iter().enumerate().filter(|(_, server)| {
					server.auth_status != "unsupported"
						&& server.runtime_status.as_deref() != Some("disabled")
				}) {
					let owner = work.to_owned();
					let server_name = server.name.clone();
					panel = panel.child(integration_button(
						format!("integration-signin-{index}"),
						format!("Sign in to {}", server.name),
						cx,
						move |s, cx| s.start_mcp_login(&owner, &server_name, cx),
					));
				}
			}
			if let Some((owner, _, status)) =
				self.mcp_login.as_ref().filter(|(owner, _, _)| owner == work)
			{
				panel = panel.child(status.message.as_str().to_owned());
				if status.authorization_url.is_some() {
					let owner = owner.clone();
					let session = status.session_id.clone();
					panel = panel.child(integration_button(
						"integration-open-signin",
						"Open authorization page",
						cx,
						move |s, cx| {
							if let Some((_, _, status)) =
								s.mcp_login.as_ref().filter(|(work, _, status)| {
									work == &owner && status.session_id == session
								}) && let Some(url) = status
								.authorization_url
								.as_ref()
								.and_then(|url| reqwest::Url::parse(url.as_str()).ok())
								.filter(|url| {
									matches!(url.scheme(), "http" | "https")
										&& url.username().is_empty() && url.password().is_none()
								}) {
								cx.open_url(url.as_str());
							}
						},
					));
				}
			}
			let text = match result {
				None => "Reading native integration status…".into(),
				Some(result) => integration_text(result),
			};
			panel = panel.child(
				div()
					.id("integration-status")
					.debug_selector(|| "integration-status".into())
					.max_h(px(300.))
					.overflow_y_scroll()
					.child(text),
			);
		}
		panel.into_any_element()
	}

	fn start_mcp_login(&mut self, work: &str, server: &str, cx: &mut Context<Self>) {
		use decodex_protocol::{McpLoginPhase, McpLoginRequest, McpLoginStatus};
		if self.mcp_login_task.is_some() {
			self.integration_feedback = "Another native MCP sign-in is still pending.".into();
			cx.notify();
			return;
		}
		let Some(profile) = self.profile.clone() else {
			self.integration_feedback = "No service profile is configured.".into();
			cx.notify();
			return;
		};
		if self.selected.as_deref() != Some(work) {
			return;
		}
		let (Ok(work_id), Ok(server_name), Ok(session_id)) = (
			EntityId::new(work.to_owned()),
			WireText::new(server.to_owned()),
			EntityId::new(unique_command()),
		) else {
			return;
		};
		let initial = McpLoginStatus {
			session_id: session_id.clone(),
			phase: McpLoginPhase::Starting,
			authorization_url: None,
			message: WireText::new("Starting native MCP sign-in…").unwrap(),
		};
		self.mcp_login = Some((work.into(), server.into(), initial));
		let work = work.to_owned();
		let server = server.to_owned();
		let generation = self.generation;
		self.mcp_login_task = Some(cx.spawn(async move |surface, cx| {
			let mut first = Some(McpLoginRequest::Start {
				session_id: session_id.clone(),
				work_id: work_id.clone(),
				server_name,
			});
			let started = std::time::Instant::now();
			loop {
				let request = first.take().unwrap_or_else(|| McpLoginRequest::Poll {
					session_id: session_id.clone(),
					work_id: work_id.clone(),
				});
				let profile = profile.clone();
				let received = cx
					.background_executor()
					.spawn(async move {
						let runtime = tokio::runtime::Builder::new_current_thread()
							.enable_all()
							.build()
							.ok()?;
						runtime.block_on(ChiefClient::new(profile).mcp_login(request)).ok()
					})
					.await;
				let mut status = received.unwrap_or_else(|| McpLoginStatus {
					session_id: session_id.clone(),
					phase: McpLoginPhase::Unknown,
					authorization_url: None,
					message: WireText::new(
						"Native sign-in is not confirmed. Checking without replaying the request…",
					)
					.unwrap(),
				});
				if started.elapsed() > std::time::Duration::from_secs(180)
					&& !matches!(
						status.phase,
						McpLoginPhase::NativeCompleted
							| McpLoginPhase::Failed
							| McpLoginPhase::Disconnected
					) {
					status.phase = McpLoginPhase::Expired;
					status.authorization_url = None;
					status.message = WireText::new(
						"Waiting expired. Refresh server status before starting another sign-in.",
					)
					.unwrap();
				}
				let completed = status.phase == McpLoginPhase::NativeCompleted;
				let terminal = matches!(
					status.phase,
					McpLoginPhase::NativeCompleted
						| McpLoginPhase::Failed
						| McpLoginPhase::Disconnected
						| McpLoginPhase::Expired
				);
				let keep = surface
					.update(cx, |s, cx| {
						if s.generation != generation
							|| !s.mcp_login.as_ref().is_some_and(|(owner, _, current)| {
								owner == &work && current.session_id == session_id
							}) {
							return false;
						}
						s.mcp_login = Some((work.clone(), server.clone(), status));
						if terminal {
							s.mcp_login_task = None;
						}
						if completed && s.selected.as_deref() == Some(work.as_str()) {
							s.load_integrations(&work, cx);
						}
						cx.notify();
						!terminal
					})
					.unwrap_or(false);
				if !keep {
					break;
				}
				cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
			}
		}));
		cx.notify();
	}

	fn refresh_native_integrations(&mut self, work: &str, cx: &mut Context<Self>) {
		if self.integration_refresh_task.is_some() || self.selected.as_deref() != Some(work) {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			self.integration_feedback = "No service profile is configured.".into();
			cx.notify();
			return;
		};
		let Ok(work_id) = EntityId::new(work.to_owned()) else {
			return;
		};
		self.integrations_task = None;
		let generation = self.generation;
		let work = work.to_owned();
		self.integration_feedback = "Synchronizing shared integrations…".into();
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let key = IdempotencyKey::new(unique_command()).ok()?;
			let result =
				runtime
					.block_on(client.execute(
						ChiefActionDto::RefreshIntegrations { work_id: work_id.clone() },
						key,
					))
					.ok();
			let status = runtime.block_on(client.integrations(work_id)).ok();
			Some((result, status))
		});
		self.integration_refresh_task = Some(cx.spawn(async move |surface, cx| {
			let (receipt, status) = request.await.unwrap_or((None, None));
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation || s.selected.as_deref() != Some(work.as_str()) {
					return;
				}
				s.integration_refresh_task = None;
				s.integration_feedback = integration_refresh_feedback(receipt);
				if s.integrations.as_ref().is_some_and(|(owner, _)| owner == &work) {
					s.integrations_task = None;
					s.integrations =
						Some((work, Some(status.unwrap_or(ChiefIntegrationsResult::Unavailable))));
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn load_integrations(&mut self, work: &str, cx: &mut Context<Self>) {
		self.integrations_task = None;
		self.integrations = Some((work.into(), None));
		let Some(profile) = self.profile.clone() else {
			self.integrations = Some((work.into(), Some(ChiefIntegrationsResult::Unavailable)));
			cx.notify();
			return;
		};
		let work = work.to_owned();
		let requested = work.clone();
		let generation = self.generation;
		let query = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime
				.block_on(ChiefClient::new(profile).integrations(EntityId::new(requested).ok()?))
				.ok()
		});
		self.integrations_task = Some(cx.spawn(async move |surface, cx| {
			let result = query.await.unwrap_or(ChiefIntegrationsResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.generation == generation
					&& s.selected.as_deref() == Some(work.as_str())
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

fn integration_refresh_feedback(receipt: Option<ChiefCommandResponse>) -> String {
	match receipt {
        Some(ChiefCommandResponse::Accepted {..})=>"Native configuration reload was acknowledged. Check the connection and discovery states below; acknowledgement does not mean every tool is ready.".into(),
        Some(ChiefCommandResponse::Rejected {error:decodex_protocol::CommandError::ApplicationUnavailable {message}})=>message.as_str().into(),
        Some(ChiefCommandResponse::Rejected {..})=>"Integration refresh was not accepted. Read current status before trying again.".into(),
        _=>"Integration refresh could not be confirmed and may have partly applied. Read current status before trying again.".into(),
    }
}

fn integration_text(result: &ChiefIntegrationsResult) -> String {
	let ChiefIntegrationsResult::Available { cwd, mcp, plugins } = result else {
		return match result {
			ChiefIntegrationsResult::CapacityExceeded =>
				"The complete integration inventory exceeds the display limit.",
			_ => "Integration status is unavailable for the current task and connection.",
		}
		.into();
	};
	let mut lines = vec![format!("Repository: {cwd}")];
	match mcp {
		ChiefMcpInventory::Available { servers } => {
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
		ChiefMcpInventory::Unsupported =>
			lines.push("This provider does not support MCP status discovery.".into()),
		ChiefMcpInventory::CapacityExceeded =>
			lines.push("MCP inventory exceeds the display limit.".into()),
		ChiefMcpInventory::Unavailable => lines.push("MCP status could not be read.".into()),
	}
	match plugins {
		ChiefPluginInventory::Available { plugins, errors } => {
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
		ChiefPluginInventory::Unsupported =>
			lines.push("This provider does not support installed-plugin discovery.".into()),
		ChiefPluginInventory::CapacityExceeded =>
			lines.push("Plugin inventory exceeds the display limit.".into()),
		ChiefPluginInventory::Unavailable =>
			lines.push("Plugin configuration could not be read.".into()),
	}
	lines.join("\n\n")
}

fn integration_button(
	id: impl Into<String>,
	label: impl Into<String>,
	cx: &mut Context<ChiefSurface>,
	action: impl Fn(&mut ChiefSurface, &mut Context<ChiefSurface>) + 'static,
) -> gpui::AnyElement {
	let id = id.into();
	let label = label.into();
	let action = std::rc::Rc::new(action);
	let click = action.clone();
	div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id)
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label.clone())
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
	#[gpui::test]
	fn mcp_login_opens_only_on_click_and_remains_scoped_to_its_task(cx: &mut gpui::TestAppContext) {
		use decodex_protocol::{McpAuthorizationUrl, McpLoginPhase, McpLoginStatus};
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
			s.integrations = Some(("root".into(), Some(ChiefIntegrationsResult::Unavailable)));
			s.mcp_login = Some((
				"root".into(),
				"server".into(),
				McpLoginStatus {
					session_id: EntityId::new("intent").unwrap(),
					phase: McpLoginPhase::AwaitingUser,
					authorization_url: Some(
						McpAuthorizationUrl::new("https://example.test/authorize".into()).unwrap(),
					),
					message: WireText::new("Continue in your browser").unwrap(),
				},
			));
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.), px(1400.)));
			window.draw(cx).clear();
		});
		assert!(visual.opened_url().is_none());
		let bounds =
			visual.debug_bounds("integration-open-signin").expect("explicit authorization control");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		assert_eq!(visual.opened_url().as_deref(), Some("https://example.test/authorize"));
		surface.update(visual, |s, cx| {
			assert_eq!(s.mcp_login.as_ref().unwrap().2.phase, McpLoginPhase::AwaitingUser);
			s.open_page("other", cx);
			assert_eq!(s.mcp_login.as_ref().unwrap().0, "root");
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("integration-open-signin").is_none());
		surface.update(visual, |s, cx| {
			s.open_page("root", cx);
			s.integrations = Some(("root".into(), Some(ChiefIntegrationsResult::Unavailable)));
			let status = &mut s.mcp_login.as_mut().unwrap().2;
			status.phase = McpLoginPhase::Expired;
			status.authorization_url = None;
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("integration-open-signin").is_none());
	}

	#[test]
	fn refresh_receipt_never_claims_runtime_readiness() {
		let accepted = integration_refresh_feedback(Some(ChiefCommandResponse::Accepted {
			work_id: EntityId::new("work").unwrap(),
		}));
		assert!(accepted.contains("does not mean every tool is ready"));
		assert!(integration_refresh_feedback(None).contains("may have partly applied"));
		let rejected = integration_refresh_feedback(Some(ChiefCommandResponse::Rejected {
			error: decodex_protocol::CommandError::ApplicationUnavailable {
				message: WireText::new("Some plugin updates failed").unwrap(),
			},
		}));
		assert_eq!(rejected, "Some plugin updates failed");
	}

	#[test]
	fn incomplete_plugin_discovery_and_mcp_failure_never_render_as_empty_success() {
		let text = integration_text(&ChiefIntegrationsResult::Available {
			cwd: "/repo".into(),
			mcp: ChiefMcpInventory::Available {
				servers: vec![decodex_protocol::ChiefMcpStatusDto {
					name: "test".into(),
					plugin_id: None,
					runtime_status: Some("authenticationRequired".into()),
					auth_status: "notLoggedIn".into(),
					tool_count: 0,
					tools_error: Some("Provider discovery failed".into()),
					resource_count: 0,
					template_count: 0,
					advertised_capabilities: None,
				}],
			},
			plugins: ChiefPluginInventory::Available {
				plugins: vec![],
				errors: vec!["Invalid repository configuration".into()],
			},
		});
		assert!(text.contains("Sign-in required"));
		assert!(text.contains("Tool discovery failed"));
		assert!(text.contains("Plugin discovery incomplete"));
		assert!(!text.contains("Reported tools: 0"));
		assert!(!text.contains("No installed plugins"));
	}
}
