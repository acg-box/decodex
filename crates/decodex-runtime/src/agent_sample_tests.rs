//! Source ownership, late-result and duplicate-generation boundaries for native samples.
use super::{Samples, Source};
use crate::agent_usage_estimate::SourceKey;
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{AccountId, ProcessGenerationId};
use decodex_protocol::{AgentSampleAnswer, AgentSamplePhase, EntityId, WireText};

fn answer() -> AgentSampleAnswer {
	AgentSampleAnswer {
		source_turn_id: WireText::new("turn").unwrap(),
		response_id: WireText::new("response").unwrap(),
		text: Some("Another approach".into()),
		input_tokens: Some(12),
		cached_input_tokens: Some(4),
		output_tokens: Some(3),
	}
}

#[tokio::test]
async fn samples_hide_old_owners_and_do_not_allow_duplicate_pending_generation() {
	let (local, mut remote) = tokio::io::duplex(4096);
	let (read, write) = tokio::io::split(local);
	let (client, mut events) = AppServerClient::from_io(read, write);
	let key = SourceKey {
		generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001").unwrap(),
		account: AccountId::new("20000000-0000-4000-8000-000000000002").unwrap(),
		revision: 1,
		history_revision: 0,
		work: "work".into(),
		thread: "parent".into(),
	};
	let make = || Source { client: client.clone(), key: key.clone() };
	let samples = Samples::default();
	samples.start(make(), "first", "Question", Default::default()).unwrap();
	assert!(samples.start(make(), "duplicate", "Question", Default::default()).is_err());
	samples.finish(&make(), "other", Ok(Some(answer())));
	assert_eq!(
		samples
			.status(EntityId::new("work").unwrap(), Some(&make()), Some(&Default::default()))
			.phase,
		AgentSamplePhase::Pending
	);
	samples.finish(&make(), "first", Ok(Some(answer())));
	let current =
		samples.status(EntityId::new("work").unwrap(), Some(&make()), Some(&Default::default()));
	assert_eq!(current.phase, AgentSamplePhase::Ready);
	assert!(current.is_valid());
	for change in ["account", "revision", "process", "thread"] {
		let mut source = make();
		match change {
			"account" =>
				source.key.account = AccountId::new("30000000-0000-4000-8000-000000000003").unwrap(),
			"revision" => source.key.revision += 1,
			"process" =>
				source.key.generation =
					ProcessGenerationId::new("40000000-0000-4000-8000-000000000004").unwrap(),
			_ => source.key.thread = "other".into(),
		}
		let status = samples.status(
			EntityId::new("work").unwrap(),
			Some(&source),
			Some(&Default::default()),
		);
		assert_eq!(status.phase, AgentSamplePhase::Obsolete, "{change}");
		assert!(status.answer.is_none());
		assert!(status.is_valid());
	}
	let changed_voice =
		decodex_database::AgentVoiceHistoryRevision { calls: 1, ..Default::default() };
	assert_eq!(
		samples.status(EntityId::new("work").unwrap(), Some(&make()), Some(&changed_voice)).phase,
		AgentSamplePhase::Obsolete
	);
	use tokio::io::AsyncWriteExt as _;
	remote.write_all(b"{\"method\":\"thread/settings/updated\",\"params\":{\"threadId\":\"parent\",\"threadSettings\":{\"model\":\"new-choice\"}}}\n").await.unwrap();
	events.recv().await.unwrap();
	assert_eq!(
		samples
			.status(EntityId::new("work").unwrap(), Some(&make()), Some(&Default::default()))
			.phase,
		AgentSamplePhase::Obsolete
	);

	samples.start(make(), "next", "New question", Default::default()).unwrap();
	samples.finish(&make(), "first", Ok(Some(answer())));
	assert_eq!(
		samples
			.status(EntityId::new("work").unwrap(), Some(&make()), Some(&Default::default()))
			.request_id
			.unwrap()
			.as_str(),
		"next"
	);
	client.close();
	let status =
		samples.status(EntityId::new("work").unwrap(), Some(&make()), Some(&Default::default()));
	assert_eq!(status.phase, AgentSamplePhase::Obsolete);
	assert!(status.answer.is_none());
}

#[tokio::test]
async fn sample_cancellation_keeps_exact_request_ownership_until_cleanup() {
	let (local, _remote) = tokio::io::duplex(4096);
	let (read, write) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(read, write);
	let source = Source {
		client,
		key: SourceKey {
			generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			account: AccountId::new("20000000-0000-4000-8000-000000000002").unwrap(),
			revision: 1,
			history_revision: 0,
			work: "work".into(),
			thread: "parent".into(),
		},
	};
	let make = || Source { client: source.client.clone(), key: source.key.clone() };
	let samples = Samples::default();
	let cancellation = samples.start(make(), "first", "Question", Default::default()).unwrap();
	assert!(samples.cancel_request("work", "foreign").is_err());
	assert!(samples.cancel_request("foreign", "first").is_err());
	samples.cancel_request("work", "first").unwrap();
	cancellation.await.unwrap();
	samples.cancel_request("work", "first").unwrap();
	assert!(samples.start(make(), "second", "Question", Default::default()).is_err());
	let status =
		samples.status(EntityId::new("work").unwrap(), Some(&source), Some(&Default::default()));
	assert_eq!(status.phase, AgentSamplePhase::Cancelling);
	assert!(status.is_valid());
	samples.finish(&source, "first", Ok(None));
	let status =
		samples.status(EntityId::new("work").unwrap(), Some(&source), Some(&Default::default()));
	assert_eq!(status.phase, AgentSamplePhase::Cancelled);
	assert!(status.is_valid());
	let _next = samples.start(make(), "second", "Question", Default::default()).unwrap();
	assert!(samples.cancel_request("work", "first").is_err());
	samples.finish(&source, "first", Ok(None));
	samples.finish(&source, "second", Ok(Some(answer())));
	samples.cancel_request("work", "second").unwrap();
	let status =
		samples.status(EntityId::new("work").unwrap(), Some(&source), Some(&Default::default()));
	assert_eq!(status.phase, AgentSamplePhase::Ready, "a late cancel must preserve the answer");
	assert!(status.answer.is_some());
}
