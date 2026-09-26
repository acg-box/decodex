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
	std::fs::write(home.join("fixture.png"), png).unwrap();
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
	std::fs::write(home.join("fixture.wav"), &wav).unwrap();
	let thread_directory = home.join("thread-directory");
	std::fs::create_dir(&thread_directory).unwrap();
	let work = EntityId::new("recap-root").unwrap();
	accepted(
		client,
		Action::Start(ChiefStartDto {
			root_id: work.clone(),
			prompt: HistoryText::new("Prepare the media fixture.").unwrap(),
			model: ConversationModel::new("cold-native-model").unwrap(),
			effort: Some(ConversationReasoningEffort::new("provider-effort").unwrap()),
			cwd: ConversationWorkingDirectory::new(home.to_str().unwrap()).unwrap(),
			account_id: Some(EntityId::new(account.as_str()).unwrap()),
			sandbox: ChiefSandboxDto::ReadOnly,
		}),
		"media-parent",
	)
	.await;
	let thread = settled(client).await;
	let native = runtime.chief_client().unwrap();
	let (generation, ..) = runtime.chief_usage_source().await.unwrap();
	assert_eq!(runtime.chief_input_directory(&generation).as_deref(), home.to_str());
	// Simulate native input from another client, preserving native relative paths.
	let started = native
		.turn_start(json!({"threadId":thread,"cwd":thread_directory,"input":[
			{"type":"text","text":"Inspect the fixture media."},
			{"type":"localImage","path":"fixture.png"},
			{"type":"localAudio","path":"fixture.wav"}
		]}))
		.await
		.unwrap();
	let turn = started["turn"]["id"].as_str().unwrap();
	loop {
		let history = native.thread_read_turn(&thread, turn).await.unwrap();
		if history["thread"]["turns"][0]["status"] == "completed" {
			break;
		}
		tokio::time::sleep(Duration::from_millis(20)).await;
	}
	assert_eq!(settled(client).await, thread);
	let items = native.thread_read_turn_items(&thread, turn).await.unwrap();
	let item = items.as_array().unwrap().iter().find(|item| item["type"] == "userMessage").unwrap();
	assert_eq!(item["content"][1]["path"], "fixture.png");
	assert_eq!(item["content"][2]["path"], "fixture.wav");
	let before = requests.load(Ordering::Acquire);
	let mut evidence = Vec::new();
	for (index, expected, mime) in
		[(1, png.as_slice(), "image/png"), (2, wav.as_slice(), "audio/wav")]
	{
		let mut request = ChiefMediaRequest {
			work_id: work.clone(),
			thread_id: EntityId::new(&thread).unwrap(),
			turn_id: EntityId::new(turn).unwrap(),
			item_id: EntityId::new(item["id"].as_str().unwrap()).unwrap(),
			index,
			offset: 0,
			fingerprint: None,
		};
		let mut bytes = Vec::new();
		let mut chunks = 0;
		loop {
			let result = client.media(request.clone()).await.unwrap();
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
		assert_eq!(bytes, expected);
		if index == 2 {
			assert!(chunks > 1);
		}
		evidence.push(json!({"index":index,"mime":mime,"bytes":bytes.len(),"chunks":chunks}));
		request.offset = 0;
		request.fingerprint = None;
		request.work_id = EntityId::new("foreign-work").unwrap();
		assert_eq!(client.media(request).await.unwrap(), ChiefMediaResult::Unavailable);
	}
	assert_eq!(requests.load(Ordering::Acquire), before, "media reads cannot infer");
	std::fs::write(
		home.join("media-evidence.json"),
		serde_json::to_vec_pretty(
			&json!({"thread":thread,"turn":turn,"item":item,"reads":evidence,"model_requests":before}),
		)
		.unwrap(),
	)
	.unwrap();
}
