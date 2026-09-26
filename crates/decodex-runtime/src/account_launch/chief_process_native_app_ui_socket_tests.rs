//! App UI acceptance through the existing isolated account/runtime/socket fixture.
use super::*;
use decodex_protocol::{
	ChiefAppUiCall, ChiefAppUiCallReview, ChiefAppUiReceiptRequest, ChiefAppUiReceiptResult,
	ChiefAppUiRequest, ChiefAppUiResult,
};

pub(super) fn configure(home: &std::path::Path) {
	let widget = home.join("widget.py");
	std::fs::write(&widget, include_str!("chief_process_native_app_ui_widget.py"))
		.expect("widget fixture");
	let mut config = std::fs::OpenOptions::new()
		.append(true)
		.open(home.join(".codex/config.toml"))
		.expect("native config");
	write!(
		config,
		"\n[mcp_servers.fixture]\ncommand=\"/usr/bin/python3\"\nargs=[{},{}]\nrequired=true\n",
		json!(widget),
		json!(home.join("widget-calls.jsonl"))
	)
	.expect("fixture server config");
}

pub(super) async fn check(
	client: &ChiefClient,
	runtime: &ConversationRuntime,
	home: &std::path::Path,
	account: &AccountId,
	requests: &std::sync::atomic::AtomicUsize,
) {
	let work = EntityId::new("recap-root").unwrap();
	accepted(
		client,
		Action::Start(ChiefStartDto {
			root_id: work.clone(),
			prompt: HistoryText::new("Read the local counter fixture.").unwrap(),
			model: ConversationModel::new("cold-native-model").unwrap(),
			effort: Some(ConversationReasoningEffort::new("provider-effort").unwrap()),
			cwd: ConversationWorkingDirectory::new(home.to_str().unwrap()).unwrap(),
			account_id: Some(EntityId::new(account.as_str()).unwrap()),
			sandbox: ChiefSandboxDto::ReadOnly,
		}),
		"app-ui-parent",
	)
	.await;
	let thread = settled(client).await;
	let native = runtime.chief_client().expect("native owner");
	let turn = native.thread_latest_turn_id(&thread).await.unwrap().unwrap();
	let items = native.thread_read_turn_items(&thread, &turn).await.unwrap();
	let item = items
		.as_array()
		.unwrap()
		.iter()
		.find(|item| item["type"] == "mcpToolCall")
		.expect("real MCP origin");
	let mut request = ChiefAppUiRequest {
		work_id: work.clone(),
		thread_id: EntityId::new(&thread).unwrap(),
		turn_id: EntityId::new(&turn).unwrap(),
		item_id: EntityId::new(item["id"].as_str().unwrap()).unwrap(),
		offset: 0,
		fingerprint: None,
	};
	let mut document = Vec::new();
	let source = loop {
		let result = client.app_ui(request.clone()).await.unwrap();
		let ChiefAppUiResult::Available {
			account_id,
			source_fingerprint,
			fingerprint,
			total_bytes,
			bytes,
			..
		} = result
		else {
			panic!("source-bound document: {result:?}")
		};
		assert_eq!(account_id.as_str(), account.as_str());
		document.extend(bytes);
		if document.len() == total_bytes as usize {
			break source_fingerprint;
		}
		request.offset = document.len() as u32;
		request.fingerprint = Some(fingerprint);
	};
	let document: Value = serde_json::from_slice(&document).unwrap();
	assert_eq!(document["item"], *item);
	assert!(document["resources"][0]["text"].as_str().unwrap().contains("Counter"));
	let mut desktop_request = request.clone();
	desktop_request.offset = 0;
	desktop_request.fingerprint = None;
	std::fs::write(
		home.join("app-ui-source.json"),
		serde_json::to_vec(&json!({"request":desktop_request,"account":account.as_str()})).unwrap(),
	)
	.unwrap();
	let call = ChiefAppUiCall {
		work_id: work.clone(),
		thread_id: request.thread_id,
		turn_id: request.turn_id,
		item_id: request.item_id,
		source_fingerprint: source,
		operation_id: EntityId::new("confirmed-widget-call").unwrap(),
		tool: WireText::new("counter").unwrap(),
		arguments: json!({"value":42}),
	};
	let before = requests.load(Ordering::Acquire);
	let ChiefAppUiCallReview::Available {
		review_token, request: reviewed, pending_operation, ..
	} = client.review_app_ui_call(call.clone()).await.unwrap()
	else {
		panic!("public review")
	};
	assert_eq!(*reviewed, call);
	assert!(pending_operation.is_none());
	assert_eq!(
		std::fs::read_to_string(home.join("widget-calls.jsonl")).unwrap().lines().count(),
		1,
		"review cannot execute"
	);
	accepted(
		client,
		Action::ConfirmAppUiTool { request: call.clone(), review_token },
		"confirm-widget",
	)
	.await;
	let mut read = ChiefAppUiReceiptRequest {
		work_id: work,
		operation_id: call.operation_id,
		offset: 0,
		fingerprint: None,
	};
	let mut saved = Vec::new();
	loop {
		let ChiefAppUiReceiptResult::Available { fingerprint, total_bytes, bytes, .. } =
			client.app_ui_receipt(read.clone()).await.unwrap()
		else {
			panic!("durable result")
		};
		saved.extend(bytes);
		if saved.len() == total_bytes as usize {
			break;
		}
		read.offset = saved.len() as u32;
		read.fingerprint = Some(fingerprint);
	}
	let saved: Value = serde_json::from_slice(&saved).unwrap();
	assert_eq!(saved["state"], "completed");
	assert_eq!(saved["result"]["structuredContent"]["value"], 42);
	assert_eq!(requests.load(Ordering::Acquire), before, "callback cannot infer");
	assert_eq!(
		std::fs::read_to_string(home.join("widget-calls.jsonl")).unwrap().lines().count(),
		2
	);
	std::fs::write(
		home.join("app-ui-evidence.json"),
		serde_json::to_vec_pretty(
			&json!({"document":document,"receipt":saved,"model_requests":before,"tool_calls":2}),
		)
		.unwrap(),
	)
	.unwrap();
	if let Some(binary) = std::env::var_os("DECODEX_TEST_APP_UI_GUI_BINARY") {
		assert!(std::path::Path::new(&binary).is_absolute());
		let log = home.join("app-ui-capture.log");
		let stdout = std::fs::File::create(&log).unwrap();
		let stderr = stdout.try_clone().unwrap();
		let output = home.join("app-ui-live.png");
		let mut child = tokio::process::Command::new(binary)
			.env("DECODEX_VISUAL_CHIEF_ROOT", home.join("product"))
			.env("DECODEX_VISUAL_CHIEF_WORK", "recap-root")
			.env("DECODEX_VISUAL_APP_UI_EXECUTE", "1")
			.env("DECODEX_VISUAL_OUTPUT", &output)
			.stdout(stdout)
			.stderr(stderr)
			.kill_on_drop(true)
			.spawn()
			.unwrap();
		let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
			.await
			.expect("bounded desktop capture")
			.unwrap();
		assert!(status.success(), "desktop capture failed; inspect {}", log.display());
		let evidence: Value =
			serde_json::from_slice(&std::fs::read(output.with_extension("app-ui.json")).unwrap())
				.unwrap();
		assert_eq!(evidence["receipt"]["state"], "completed");
		assert_eq!(evidence["receipt"]["result"]["structuredContent"]["value"], 42);
		assert_eq!(evidence["browserPing"], "fixture-counter-42");
		assert_eq!(requests.load(Ordering::Acquire), before, "desktop callback cannot infer");
		assert_eq!(
			std::fs::read_to_string(home.join("widget-calls.jsonl")).unwrap().lines().count(),
			3
		);
	}
}
