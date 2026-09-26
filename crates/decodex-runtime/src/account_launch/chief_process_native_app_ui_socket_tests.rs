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
	let work = EntityId::new("recap-root").expect("native App UI fixture");
	accepted(
		client,
		Action::Start(ChiefStartDto {
			root_id: work.clone(),
			prompt: HistoryText::new("Read the local counter fixture.")
				.expect("native App UI fixture"),
			model: ConversationModel::new("cold-native-model").expect("native App UI fixture"),
			effort: Some(
				ConversationReasoningEffort::new("provider-effort").expect("native App UI fixture"),
			),
			cwd: ConversationWorkingDirectory::new(home.to_str().expect("native App UI fixture"))
				.expect("native App UI fixture"),
			account_id: Some(EntityId::new(account.as_str()).expect("native App UI fixture")),
			sandbox: ChiefSandboxDto::ReadOnly,
		}),
		"app-ui-parent",
	)
	.await;
	let thread = settled(client).await;
	let native = runtime.chief_client().expect("native owner");
	let turn = native
		.thread_latest_turn_id(&thread)
		.await
		.expect("native App UI fixture")
		.expect("native App UI fixture");
	let items = native.thread_read_turn_items(&thread, &turn).await.expect("native App UI fixture");
	let item = items
		.as_array()
		.expect("native App UI fixture")
		.iter()
		.find(|item| item["type"] == "mcpToolCall")
		.expect("real MCP origin");
	let mut request = ChiefAppUiRequest {
		work_id: work.clone(),
		thread_id: EntityId::new(&thread).expect("native App UI fixture"),
		turn_id: EntityId::new(&turn).expect("native App UI fixture"),
		item_id: EntityId::new(item["id"].as_str().expect("native App UI fixture"))
			.expect("native App UI fixture"),
		offset: 0,
		fingerprint: None,
	};
	let (document, source) = read_document(client, &mut request, account).await;
	assert_eq!(document["item"], *item);
	assert!(document["resources"][0]["text"].as_str().expect("widget text").contains("Counter"));
	let mut desktop_request = request.clone();
	desktop_request.offset = 0;
	desktop_request.fingerprint = None;
	std::fs::write(
		home.join("app-ui-source.json"),
		serde_json::to_vec(&json!({"request":desktop_request,"account":account.as_str()}))
			.expect("native App UI fixture"),
	)
	.expect("native App UI fixture");
	let call = ChiefAppUiCall {
		work_id: work.clone(),
		thread_id: request.thread_id,
		turn_id: request.turn_id,
		item_id: request.item_id,
		source_fingerprint: source,
		operation_id: EntityId::new("confirmed-widget-call").expect("native App UI fixture"),
		tool: WireText::new("counter").expect("native App UI fixture"),
		arguments: json!({"value":42}),
	};
	let before = requests.load(Ordering::Acquire);
	let ChiefAppUiCallReview::Available {
		review_token, request: reviewed, pending_operation, ..
	} = client.review_app_ui_call(call.clone()).await.expect("native App UI fixture")
	else {
		panic!("public review")
	};
	assert_eq!(*reviewed, call);
	assert!(pending_operation.is_none());
	assert_eq!(
		std::fs::read_to_string(home.join("widget-calls.jsonl"))
			.expect("native App UI fixture")
			.lines()
			.count(),
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
	let saved = read_receipt(client, &mut read).await;
	let saved: Value = serde_json::from_slice(&saved).expect("native App UI fixture");
	assert_eq!(saved["state"], "completed");
	assert_eq!(saved["result"]["structuredContent"]["value"], 42);
	assert_eq!(requests.load(Ordering::Acquire), before, "callback cannot infer");
	assert_eq!(
		std::fs::read_to_string(home.join("widget-calls.jsonl"))
			.expect("native App UI fixture")
			.lines()
			.count(),
		2
	);
	std::fs::write(
		home.join("app-ui-evidence.json"),
		serde_json::to_vec_pretty(
			&json!({"document":document,"receipt":saved,"model_requests":before,"tool_calls":2}),
		)
		.expect("native App UI fixture"),
	)
	.expect("native App UI fixture");
	qualify_desktop(home, requests, before).await;
}

async fn qualify_desktop(
	home: &std::path::Path,
	requests: &std::sync::atomic::AtomicUsize,
	before: usize,
) {
	if let Some(binary) = std::env::var_os("DECODEX_TEST_APP_UI_GUI_BINARY") {
		assert!(std::path::Path::new(&binary).is_absolute());
		let log = home.join("app-ui-capture.log");
		let stdout = std::fs::File::create(&log).expect("native App UI fixture");
		let stderr = stdout.try_clone().expect("native App UI fixture");
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
			.expect("native App UI fixture");
		let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
			.await
			.expect("bounded desktop capture")
			.expect("native App UI fixture");
		assert!(status.success(), "desktop capture failed; inspect {}", log.display());
		let evidence: Value = serde_json::from_slice(
			&std::fs::read(output.with_extension("app-ui.json")).expect("native App UI fixture"),
		)
		.expect("native App UI fixture");
		assert_eq!(evidence["receipt"]["state"], "completed");
		assert_eq!(evidence["receipt"]["result"]["structuredContent"]["value"], 42);
		assert_eq!(evidence["browserPing"], "fixture-counter-42");
		assert_eq!(requests.load(Ordering::Acquire), before, "desktop callback cannot infer");
		assert_eq!(
			std::fs::read_to_string(home.join("widget-calls.jsonl"))
				.expect("native App UI fixture")
				.lines()
				.count(),
			3
		);
	}
}

async fn read_document(
	client: &ChiefClient,
	request: &mut ChiefAppUiRequest,
	account: &AccountId,
) -> (Value, EntityId) {
	let mut document = Vec::new();
	let source = loop {
		let result = client.app_ui(request.clone()).await.expect("native App UI fixture");
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
	let document: Value = serde_json::from_slice(&document).expect("native App UI fixture");
	(document, source)
}

async fn read_receipt(client: &ChiefClient, read: &mut ChiefAppUiReceiptRequest) -> Vec<u8> {
	let mut saved = Vec::new();
	loop {
		let ChiefAppUiReceiptResult::Available { fingerprint, total_bytes, bytes, .. } =
			client.app_ui_receipt(read.clone()).await.expect("native App UI fixture")
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
	saved
}
