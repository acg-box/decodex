use super::*;
use std::sync::{
	Arc,
	atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// Run with an isolated CODEX_HOME, registered fixture marketplace, and local Responses backend.
#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_PLUGIN_HOME and an isolated native fixture backend"]
async fn native_plugin_suggestion_runs_through_the_coordinator()
-> Result<(), Box<dyn std::error::Error>> {
	let home = std::env::var("DECODEX_NATIVE_PLUGIN_HOME")?;
	let token = std::env::var("DECODEX_NATIVE_PLUGIN_FIXTURE_TOKEN")?;
	let executable = std::env::var("DECODEX_NATIVE_PLUGIN_EXECUTABLE")?;
	let cwd = std::path::Path::new(&home).join("repo");
	let (mut chief, _sent, _directory) = fixture().await;
	let mut command = tokio::process::Command::new(executable);
	command.arg("app-server").current_dir(&cwd).env("CODEX_HOME", &home);
	let (client, mut events, mut process) = AppServerClient::spawn(&mut command)?;
	let outcome: Result<(), Box<dyn std::error::Error>> = async {
		client.initialize(json!({"clientInfo":{"name":"decodex_install_fixture","version":"0.1"},"capabilities":{"experimentalApi":true}})).await?;
		client.request("account/login/start",json!({"type":"chatgptAuthTokens","accessToken":token,"chatgptAccountId":"fixture-account","chatgptPlanType":"plus"})).await?;
		chief.client = client;
		chief.config = ChiefConfig::new("gpt-5.5".into(), "medium".into(), cwd.display().to_string());
		chief.start_chief("chief", "Use the sample plugin for this isolated acceptance fixture.").await?;
		let event = tokio::time::timeout(std::time::Duration::from_secs(30), async {
			while let Some(event) = events.recv().await {
				let request = match &event {
					ServerEvent::Request { id, method, .. } if method == "mcpServer/elicitation/request" => Some(id.clone()),
					_ => None,
				};
				chief.handle_event(event).await?;
				if let Some(id) = request {
					return chief.pending_requests.get(&id).copied().ok_or_else(|| ChiefError::Invalid("native suggestion not stored".into()));
				}
			}
			Err(ChiefError::Invalid("native event stream ended".into()))
		}).await??;
		let inspected = crate::chief_install::inspect(&chief.store, &chief.client, "chief", event).await.ok_or("native inspection unavailable")?;
		let decodex_protocol::ChiefInstallState::Available { can_install:true, can_continue:false, review_token, .. } = inspected.state else {
			return Err("unexpected native pre-install state".into());
		};
		if chief.respond_pending_event(event,json!({"action":"accept","content":{},"_meta":null})).await.is_ok() {
			return Err("native suggestion accepted without installation".into());
		}
		chief.install_suggested_plugin("chief",event,&review_token,"native-fixture-attempt").await?;
		if chief.install_suggested_plugin("chief",event,&review_token,"native-fixture-replay").await.is_ok() {
			return Err("native installation replay accepted".into());
		}
		let requirements = chief.store.chief_install_requirements(event).await?.ok_or("native receipt not saved")?;
		if requirements.auth_policy != "ON_INSTALL" || !requirements.connector_ids.is_empty() {
			return Err("unexpected native receipt requirements".into());
		}
		chief.respond_pending_event(event,json!({"action":"accept","content":{},"_meta":null})).await?;
		tokio::time::timeout(std::time::Duration::from_secs(30), async {
			while let Some(event) = events.recv().await {
				let done = matches!(&event,ServerEvent::Notification{method,..} if method=="turn/completed");
				chief.handle_event(event).await?;
				if done { return Ok::<(),ChiefError>(()); }
			}
			Err(ChiefError::Invalid("native completion missing".into()))
		}).await??;
		if chief.store.get_chief_inbox_event(event).await?.disposition.is_none() {
			return Err("native suggestion disposition missing".into());
		}
		Ok(())
	}.await;
	process.shutdown().await?;
	outcome
}

#[tokio::test]
async fn plugin_installation_requires_real_installation_and_connector_access_before_reply() {
	exercise_installation(true, false).await;
}

#[tokio::test]
async fn remote_install_without_confirmed_receipt_keeps_authorization_unknown() {
	exercise_installation(false, false).await;
}

#[tokio::test]
async fn native_child_installation_preserves_root_receipt_and_child_request() {
	exercise_installation(true, true).await;
}

struct InstallationFixtureState {
	installed: Arc<AtomicBool>,
	enabled: Arc<AtomicBool>,
	accessible: Arc<AtomicBool>,
	receipt_only_accessible: Arc<AtomicBool>,
	installs: Arc<AtomicUsize>,
	replies: Arc<AtomicUsize>,
}
async fn serve_installation_fixture(
	remote: tokio::io::DuplexStream,
	params: Value,
	receipt_confirmed: bool,
	state: InstallationFixtureState,
) {
	let InstallationFixtureState {
		installed: i,
		enabled: plugin_enabled,
		accessible: a,
		receipt_only_accessible: extra,
		installs: n,
		replies: r,
	} = state;
	let (reader, mut writer) = tokio::io::split(remote);
	writer
		.write_all(
			format!(
				"{}\n",
				json!({"id":"suggestion-1","method":"mcpServer/elicitation/request","params":params})
			)
			.as_bytes(),
		)
		.await
		.unwrap();
	let mut lines = BufReader::new(reader).lines();
	while let Some(line) = lines.next_line().await.unwrap() {
		let request: Value = serde_json::from_str(&line).unwrap();
		if request.get("method").is_none() {
			assert_eq!(request["id"], "suggestion-1");
			assert!(i.load(Ordering::SeqCst) && a.load(Ordering::SeqCst));
			r.fetch_add(1, Ordering::SeqCst);
			continue;
		}
		let summary = json!({"id":"sample@market","name":"sample","remotePluginId":"plugins~sample","installed":i.load(Ordering::SeqCst),"enabled":plugin_enabled.load(Ordering::SeqCst),"availability":"AVAILABLE","installPolicy":"AVAILABLE","authPolicy":"ON_INSTALL","source":{"type":"git","url":"https://example.com/plugin"}});
		let result = match request["method"].as_str().unwrap() {
			"thread/read" => {
				let thread = &request["params"]["threadId"];
				if thread == "native-child" {
					json!({"thread":{"id":thread,"cwd":"/tmp","parentThreadId":"opaque thread/1","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"opaque thread/1"}}}}})
				} else {
					json!({"thread":{"id":thread,"cwd":"/tmp"}})
				}
			},
			"plugin/list" =>
				json!({"marketplaces":[{"name":"market","path":null,"plugins":[summary]}],"marketplaceLoadErrors":[]}),
			"plugin/read" =>
				json!({"plugin":{"summary":summary,"description":"Sample integration","apps":[{"id":"connector","name":"Calendar","installUrl":"https://chatgpt.com/apps/calendar"}],"skills":[],"mcpServers":[],"hooks":[]}}),
			"app/list" =>
				json!({"data":[{"id":"connector","name":"Calendar","isAccessible":a.load(Ordering::SeqCst),"isEnabled":true,"installUrl":"https://chatgpt.com/apps/calendar"},{"id":"receipt-only","name":"Extra connector","isAccessible":extra.load(Ordering::SeqCst),"isEnabled":true}],"nextCursor":null}),
			"plugin/install" => {
				assert_eq!(request["params"]["pluginName"], "plugins~sample");
				assert_eq!(n.fetch_add(1, Ordering::SeqCst), 0);
				i.store(true, Ordering::SeqCst);
				json!({"authPolicy":"ON_INSTALL","appsNeedingAuth":[{"id":"connector","name":"Calendar"},{"id":"receipt-only","name":"Extra connector","installUrl":"https://chatgpt.com/apps/extra/receipt-only"}]})
			},
			other => panic!("unexpected {other}"),
		};
		let response = if request["method"] == "plugin/install" && !receipt_confirmed {
			json!({"id":request["id"],"error":{"code":-32603,"message":"Installation outcome is uncertain"}})
		} else {
			json!({"id":request["id"],"result":result})
		};
		writer.write_all(format!("{response}\n").as_bytes()).await.unwrap();
	}
}

async fn review_before_installation(
	chief: &mut ChiefCoordinator,
	event: i64,
	response: &Value,
	installs: &AtomicUsize,
) -> String {
	assert!(chief.respond_pending_event(event, response.clone()).await.is_err());
	assert_eq!(installs.load(Ordering::SeqCst), 0);
	let inspection =
		crate::chief_install::inspect(&chief.store, &chief.client, "chief", event).await.unwrap();
	let decodex_protocol::ChiefInstallState::Available {
		can_install,
		can_continue,
		review_token,
		apps,
		..
	} = inspection.state
	else {
		panic!("inspection");
	};
	assert!(can_install);
	assert!(!can_continue);
	assert_eq!(apps.len(), 1, "native details add connectors omitted by stale suggestion");
	assert!(
		chief
			.install_suggested_plugin("chief", event, "stale-details", "attempt-stale")
			.await
			.is_err()
	);
	assert_eq!(installs.load(Ordering::SeqCst), 0);
	review_token
}

async fn exercise_installation(receipt_confirmed: bool, native_child: bool) {
	let (mut chief, _sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let (local, remote) = tokio::io::duplex(65536);
	let (reader, writer) = tokio::io::split(local);
	let (client, mut events) = AppServerClient::from_io(reader, writer);
	chief.client = client;
	let installed = Arc::new(AtomicBool::new(false));
	let enabled = Arc::new(AtomicBool::new(true));
	let accessible = Arc::new(AtomicBool::new(false));
	let receipt_only_accessible = Arc::new(AtomicBool::new(false));
	let installs = Arc::new(AtomicUsize::new(0));
	let replies = Arc::new(AtomicUsize::new(0));
	let params = json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","serverName":"codex_apps","mode":"form","message":"Install Sample","requestedSchema":{"type":"object","properties":{}},"_meta":{"codex_approval_kind":"tool_suggestion","suggest_type":"install","tool_type":"plugin","tool_id":"sample@market","tool_name":"Sample","suggestion_id":"request_plugin_install_call-2","remote_plugin_id":"plugins~sample","app_connector_ids":[]}});
	let mut params = params;
	if native_child {
		params["threadId"] = json!("native-child");
		params["turnId"] = json!("native-child-turn");
	}
	let state = InstallationFixtureState {
		installed: installed.clone(),
		enabled: enabled.clone(),
		accessible: accessible.clone(),
		receipt_only_accessible: receipt_only_accessible.clone(),
		installs: installs.clone(),
		replies: replies.clone(),
	};
	let server = tokio::spawn(serve_installation_fixture(remote, params, receipt_confirmed, state));
	chief.handle_event(events.recv().await.unwrap()).await.unwrap();
	let event = *chief.pending_requests.get(&RequestId::String("suggestion-1".into())).unwrap();
	let response = json!({"action":"accept","content":{},"_meta":null});
	let review_token = review_before_installation(&mut chief, event, &response, &installs).await;
	chief.install_suggested_plugin("chief", event, &review_token, "attempt-1").await.unwrap();
	assert_eq!(installs.load(Ordering::SeqCst), 1);
	assert!(chief.pending_requests.values().any(|id| *id == event));
	assert!(chief.respond_pending_event(event, response.clone()).await.is_err());
	assert!(
		chief
			.install_suggested_plugin("chief", event, &review_token, "another-client")
			.await
			.is_err()
	);
	accessible.store(true, Ordering::SeqCst);
	if !receipt_confirmed {
		assert!(chief.respond_pending_event(event, response).await.is_err());
		let current = crate::chief_install::inspect(&chief.store, &chief.client, "chief", event)
			.await
			.unwrap();
		assert!(matches!(
			current.state,
			decodex_protocol::ChiefInstallState::Available {
				authorization_requirements_known: false,
				can_continue: false,
				..
			}
		));
		server.abort();
		let _ = server.await;
		return;
	}
	assert!(
		chief.respond_pending_event(event, response.clone()).await.is_err(),
		"receipt-only connector must block continuation"
	);
	let saved = chief.store.chief_install_requirements(event).await.unwrap().unwrap();
	assert!(saved.connector_ids.contains(&"receipt-only".into()));
	let current =
		crate::chief_install::inspect(&chief.store, &chief.client, "chief", event).await.unwrap();
	let decodex_protocol::ChiefInstallState::Available { apps, .. } = current.state else {
		panic!("state");
	};
	assert_eq!(
		apps.iter()
			.find(|a| a.id == "receipt-only")
			.unwrap()
			.install_url
			.as_ref()
			.unwrap()
			.as_str(),
		"https://chatgpt.com/apps/extra/receipt-only"
	);
	receipt_only_accessible.store(true, Ordering::SeqCst);
	enabled.store(false, Ordering::SeqCst);
	assert!(chief.respond_pending_event(event, response.clone()).await.is_err());
	let current =
		crate::chief_install::inspect(&chief.store, &chief.client, "chief", event).await.unwrap();
	let decodex_protocol::ChiefInstallState::Available { can_continue, review_details, .. } =
		current.state
	else {
		panic!("disabled state");
	};
	assert!(!can_continue, "installed and authorized does not override disabled configuration");
	assert!(review_details.contains("Installed but disabled in Codex configuration"));
	assert!(review_details.contains("Repository: https://example.com/plugin"));
	enabled.store(true, Ordering::SeqCst);

	chief.respond_pending_event(event, response).await.unwrap();
	assert!(!chief.pending_requests.values().any(|id| *id == event));
	for _ in 0..50 {
		if replies.load(Ordering::SeqCst) == 1 {
			break;
		}
		tokio::task::yield_now().await;
	}
	assert_eq!(replies.load(Ordering::SeqCst), 1);
	assert!(chief.store.get_chief_inbox_event(event).await.unwrap().disposition.is_some());
	assert_eq!(
		chief.store.chief_install_attempt_id(event).await.unwrap().as_deref(),
		Some("attempt-1")
	);
	server.abort();
	let _ = server.await;
}
