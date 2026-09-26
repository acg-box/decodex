//! Native media history and exact public byte reads through the production service.
use super::*;
use decodex_protocol::{ChiefMediaRequest, ChiefMediaResult};

pub(super) async fn check(
	client: &ChiefClient,
	runtime: &ConversationRuntime,
	home: &std::path::Path,
	account: &AccountId,
	requests: &std::sync::atomic::AtomicUsize,
) {
	let png = include_bytes!("../../../../assets/workspace-symbols/plus.png");
	std::fs::write(home.join("fixture.png"), png).expect("native media fixture");
	// A complete two-second PCM WAV forces several public chunks.
	let mut wav = b"RIFF".to_vec();
	wav.extend(64036_u32.to_le_bytes());
	wav.extend(b"WAVEfmt ");
	wav.extend(16_u32.to_le_bytes());
	wav.extend(1_u16.to_le_bytes());
	wav.extend(1_u16.to_le_bytes());
	wav.extend(16000_u32.to_le_bytes());
	wav.extend(32000_u32.to_le_bytes());
	wav.extend(2_u16.to_le_bytes());
	wav.extend(16_u16.to_le_bytes());
	wav.extend(b"data");
	wav.extend(64000_u32.to_le_bytes());
	wav.resize(64044, 0);
	std::fs::write(home.join("fixture.wav"), &wav).expect("native media fixture");
	let thread_directory = home.join("thread-directory");
	std::fs::create_dir(&thread_directory).expect("native media fixture");
	let work = EntityId::new("recap-root").expect("native media fixture");
	accepted(
		client,
		Action::Start(ChiefStartDto {
			root_id: work.clone(),
			prompt: HistoryText::new("Prepare the media fixture.").expect("native media fixture"),
			model: ConversationModel::new("cold-native-model").expect("native media fixture"),
			effort: Some(
				ConversationReasoningEffort::new("provider-effort").expect("native media fixture"),
			),
			cwd: ConversationWorkingDirectory::new(home.to_str().expect("native media fixture"))
				.expect("native media fixture"),
			account_id: Some(EntityId::new(account.as_str()).expect("native media fixture")),
			sandbox: ChiefSandboxDto::ReadOnly,
		}),
		"media-parent",
	)
	.await;
	let thread = settled(client).await;
	let native = runtime.chief_client().expect("native media fixture");
	let (generation, ..) = runtime.chief_usage_source().await.expect("native media fixture");
	assert_eq!(runtime.chief_input_directory(&generation).as_deref(), home.to_str());
	// Simulate native input from another client, preserving native relative paths.
	let started = native
		.turn_start(json!({"threadId":thread,"cwd":thread_directory,"input":[
			{"type":"text","text":"Inspect the fixture media."},
			{"type":"localImage","path":"fixture.png"},
			{"type":"localAudio","path":"fixture.wav"}
		]}))
		.await
		.expect("native media fixture");
	let turn = started["turn"]["id"].as_str().expect("native media fixture");
	loop {
		let history = native.thread_read_turn(&thread, turn).await.expect("native media fixture");
		if history["thread"]["turns"][0]["status"] == "completed" {
			break;
		}
		tokio::time::sleep(Duration::from_millis(20)).await;
	}
	assert_eq!(settled(client).await, thread);
	let items = native.thread_read_turn_items(&thread, turn).await.expect("native media fixture");
	let item = items
		.as_array()
		.expect("native media fixture")
		.iter()
		.find(|item| item["type"] == "userMessage")
		.expect("native media fixture");
	assert_eq!(item["content"][1]["path"], "fixture.png");
	assert_eq!(item["content"][2]["path"], "fixture.wav");
	let before = requests.load(Ordering::Acquire);
	let mut evidence = Vec::new();
	for (index, expected, mime) in
		[(1, png.as_slice(), "image/png"), (2, wav.as_slice(), "audio/wav")]
	{
		let mut request = ChiefMediaRequest {
			work_id: work.clone(),
			thread_id: EntityId::new(&thread).expect("native media fixture"),
			turn_id: EntityId::new(turn).expect("native media fixture"),
			item_id: EntityId::new(item["id"].as_str().expect("native media fixture"))
				.expect("native media fixture"),
			index,
			offset: 0,
			fingerprint: None,
		};
		let (bytes, chunks) = read_media(client, &mut request, account, mime).await;
		assert_eq!(bytes, expected);
		if index == 2 {
			assert!(chunks > 1);
		}
		evidence.push(json!({"index":index,"mime":mime,"bytes":bytes.len(),"chunks":chunks}));
		request.offset = 0;
		request.fingerprint = None;
		if index == 1 {
			std::fs::write(
				home.join("media-source.json"),
				serde_json::to_vec(&request).expect("native media fixture"),
			)
			.expect("native media fixture");
		}
		request.work_id = EntityId::new("foreign-work").expect("native media fixture");
		assert_eq!(
			client.media(request).await.expect("native media fixture"),
			ChiefMediaResult::Unavailable
		);
	}
	assert_eq!(requests.load(Ordering::Acquire), before, "media reads cannot infer");
	std::fs::write(
		home.join("media-evidence.json"),
		serde_json::to_vec_pretty(
			&json!({"thread":thread,"turn":turn,"item":item,"reads":evidence,"model_requests":before}),
		)
		.expect("native media fixture"),
	)
	.expect("native media fixture");
	qualify_desktop(home, &work, item, requests, before).await;
	qualify_resources(client, &work).await;
	assert_eq!(requests.load(Ordering::Acquire), before, "resource association cannot infer");
}

async fn qualify_resources(client: &ChiefClient, work: &EntityId) {
	use decodex_protocol::ChiefResourcesResult;
	let empty = ChiefResourcesResult::Available { resources: vec![] };
	assert_eq!(client.resources(work.clone()).await.expect("native media fixture"), empty);
	let add = Action::AddResourceLink {
		work_id: work.clone(),
		title: WireText::new("Local fixture link").expect("native media fixture"),
		url: WireText::new("https://example.invalid/media-fixture").expect("native media fixture"),
	};
	accepted(client, add.clone(), "resource-add").await;
	let first = client.resources(work.clone()).await.expect("native media fixture");
	let ChiefResourcesResult::Available { resources } = &first else {
		panic!("native resources: {first:?}");
	};
	assert_eq!(resources.len(), 1);
	let resource = &resources[0];
	assert!(!resource.payload_omitted);
	assert!(resource.payload_json.contains("https://example.invalid/media-fixture"));
	accepted(client, add, "resource-add-again").await;
	assert_eq!(client.resources(work.clone()).await.expect("native media fixture"), first);
	accepted(
		client,
		Action::RemoveResource {
			work_id: work.clone(),
			attachment_type: WireText::new(&resource.attachment_type)
				.expect("native media fixture"),
			identity_key: WireText::new(&resource.identity_key).expect("native media fixture"),
		},
		"resource-remove",
	)
	.await;
	assert_eq!(client.resources(work.clone()).await.expect("native media fixture"), empty);
}

async fn qualify_desktop(
	home: &std::path::Path,
	work: &EntityId,
	item: &Value,
	requests: &std::sync::atomic::AtomicUsize,
	before: usize,
) {
	if let Some(binary) = std::env::var_os("DECODEX_TEST_MEDIA_GUI_BINARY") {
		assert!(std::path::Path::new(&binary).is_absolute());
		let log = home.join("media-capture.log");
		let stdout = std::fs::File::create(&log).expect("native media fixture");
		let stderr = stdout.try_clone().expect("native media fixture");
		let output = home.join("media-live.png");
		let mut child = tokio::process::Command::new(binary)
			.env("DECODEX_VISUAL_CHIEF_ROOT", home.join("product"))
			.env("DECODEX_VISUAL_CHIEF_WORK", work.as_str())
			.env("DECODEX_VISUAL_MEDIA", "1")
			.env("DECODEX_VISUAL_OUTPUT", &output)
			.stdout(stdout)
			.stderr(stderr)
			.kill_on_drop(true)
			.spawn()
			.expect("native media fixture");
		let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
			.await
			.expect("bounded desktop capture")
			.expect("native media fixture");
		assert!(status.success(), "media capture failed; inspect {}", log.display());
		let saved: Value = serde_json::from_slice(
			&std::fs::read(output.with_extension("media.json")).expect("native media fixture"),
		)
		.expect("native media fixture");
		assert_eq!(saved["imageLoaded"], true);
		assert!(saved["notice"].is_null());
		assert_eq!(saved["request"]["item_id"], item["id"]);
		assert_eq!(requests.load(Ordering::Acquire), before, "desktop preview cannot infer");
	}
}

async fn read_media(
	client: &ChiefClient,
	request: &mut ChiefMediaRequest,
	account: &AccountId,
	mime: &str,
) -> (Vec<u8>, usize) {
	let mut bytes = Vec::new();
	let mut chunks = 0;
	loop {
		let result = client.media(request.clone()).await.expect("native media fixture");
		let ChiefMediaResult::Available {
			account_id,
			fingerprint,
			total_bytes,
			bytes: chunk,
			mime_type,
			..
		} = result
		else {
			panic!("native media: {result:?}");
		};
		assert_eq!(account_id.as_str(), account.as_str());
		assert_eq!(mime_type, mime);
		bytes.extend(chunk);
		chunks += 1;
		if bytes.len() == total_bytes as usize {
			break;
		}
		request.offset = bytes.len() as u32;
		request.fingerprint = Some(fingerprint);
	}
	(bytes, chunks)
}
