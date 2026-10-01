//! Short-lived transfer of one fully hydrated conversation, independent of history storage.
use std::{
	future::Future,
	sync::Arc,
	time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use tokio::{sync::Mutex, time};

use crate::agent_usage_estimate::{Source, SourceKey};
use decodex_codex::app_server_client::ClientError;
use decodex_protocol::{
	AgentTranscriptRequest, AgentTranscriptResult, EntityId, TRANSCRIPT_CHUNK_BYTES,
};

#[derive(Clone, Default)]
pub(crate) struct Transcripts(Arc<Mutex<Option<Document>>>);
impl Transcripts {
	pub(crate) async fn read<F, Fut>(
		&self,
		source: F,
		request: &AgentTranscriptRequest,
	) -> AgentTranscriptResult
	where
		F: Fn() -> Fut,
		Fut: Future<Output = Option<Source>>,
	{
		let Some(before) = source().await else { return AgentTranscriptResult::Unavailable };

		if before.key.work != request.work_id.as_str()
			|| before.key.thread != request.thread_id.as_str()
		{
			return AgentTranscriptResult::Unavailable;
		}
		if request.offset == 0 && request.token.is_none() {
			let text = match before.client.thread_markdown_transcript(&before.key.thread).await {
				Ok(text) => text,
				Err(ClientError::CapacityExceeded) =>
					return AgentTranscriptResult::CapacityExceeded,
				Err(_) => return AgentTranscriptResult::Unavailable,
			};

			if source().await.is_none_or(|after| after.key != before.key) {
				return AgentTranscriptResult::Unavailable;
			}

			let mut hash = Sha256::new();

			hash.update(format!("{:?}", before.key));
			hash.update(text.as_bytes());

			let token = EntityId::new(
				hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
			)
			.expect("SHA-256");
			let expires_token = token.clone();

			*self.0.lock().await = Some(Document {
				owner: before.key.clone(),
				token,
				created: Instant::now(),
				bytes: text.into_bytes(),
			});

			let cache = Arc::downgrade(&self.0);

			tokio::spawn(async move {
				time::sleep(Duration::from_secs(120)).await;

				if let Some(cache) = cache.upgrade() {
					let mut cache = cache.lock().await;

					if cache.as_ref().is_some_and(|doc| {
						doc.token == expires_token
							&& doc.created.elapsed() >= Duration::from_secs(120)
					}) {
						*cache = None;
					}
				}
			});
		}

		let mut cache = self.0.lock().await;
		let Some(document) = cache.as_ref() else { return AgentTranscriptResult::Unavailable };

		if document.created.elapsed() > Duration::from_secs(120) {
			*cache = None;

			return AgentTranscriptResult::Unavailable;
		}
		if document.owner != before.key
			|| request.token.as_ref().is_some_and(|token| token != &document.token)
			|| (request.offset > 0 && request.token.is_none())
		{
			return AgentTranscriptResult::Unavailable;
		}

		let start = request.offset as usize;

		if start >= document.bytes.len() {
			return AgentTranscriptResult::Unavailable;
		}

		let end = (start + TRANSCRIPT_CHUNK_BYTES).min(document.bytes.len());
		let Ok(account_id) = EntityId::new(document.owner.account.as_str()) else {
			return AgentTranscriptResult::Unavailable;
		};
		let result = AgentTranscriptResult::Available {
			request: request.clone(),
			account_id,
			token: document.token.clone(),
			total_bytes: document.bytes.len() as u32,
			bytes: document.bytes[start..end].to_vec(),
		};

		if end == document.bytes.len() {
			*cache = None;
		}

		result
	}
}

struct Document {
	owner: SourceKey,
	token: EntityId,
	created: Instant,
	bytes: Vec<u8>,
}
#[cfg(test)]
mod tests {
	use tokio::io;

	use crate::agent_transcript::{
		AgentTranscriptRequest, AgentTranscriptResult, Document, EntityId, Instant, Source,
		SourceKey, TRANSCRIPT_CHUNK_BYTES, Transcripts,
	};
	use decodex_core::{AccountId, ProcessGenerationId};

	#[tokio::test]
	async fn transcript_chunks_keep_owner_and_token_and_release_completed_document() {
		let key = SourceKey {
			generation: ProcessGenerationId::new("20000000-0000-4000-8000-000000000002").unwrap(),
			account: AccountId::new("30000000-0000-4000-8000-000000000003").unwrap(),
			revision: 1,
			history_revision: 1,
			thread: "thread".into(),
			work: "work".into(),
		};
		let token = EntityId::new("export-token").unwrap();
		let request = AgentTranscriptRequest {
			work_id: EntityId::new("work").unwrap(),
			thread_id: EntityId::new("thread").unwrap(),
			offset: 1,
			token: Some(token.clone()),
		};
		let cache = Transcripts::default();

		*cache.0.lock().await = Some(Document {
			owner: key.clone(),
			token,
			created: Instant::now(),
			bytes: vec![b'x'; TRANSCRIPT_CHUNK_BYTES + 3],
		});

		let (reader, writer) = io::duplex(1_024);
		let (client, _) =
			decodex_codex::app_server_client::AppServerClient::from_io(reader, writer);
		let source = || async { Some(Source { key: key.clone(), client: client.clone() }) };
		let mut stale = request.clone();

		stale.token = Some(EntityId::new("other-export").unwrap());

		assert_eq!(cache.read(source, &stale).await, AgentTranscriptResult::Unavailable);

		let mut foreign = key.clone();

		foreign.account = AccountId::new("40000000-0000-4000-8000-000000000004").unwrap();

		assert_eq!(
			cache
				.read(
					|| async { Some(Source { key: foreign.clone(), client: client.clone() }) },
					&request
				)
				.await,
			AgentTranscriptResult::Unavailable
		);

		let first = cache.read(source, &request).await;
		let AgentTranscriptResult::Available { bytes, total_bytes, .. } = &first else {
			panic!("{first:?}")
		};

		assert_eq!(bytes.len(), TRANSCRIPT_CHUNK_BYTES);
		assert_eq!(*total_bytes as usize, TRANSCRIPT_CHUNK_BYTES + 3);
		assert!(serde_json::to_vec(&first).unwrap().len() < 256 * 1_024);

		let mut last = request;

		last.offset += TRANSCRIPT_CHUNK_BYTES as u32;

		assert!(
			matches!(cache.read(source,&last).await,AgentTranscriptResult::Available{bytes,..} if bytes==vec![b'x';2])
		);
		assert!(cache.0.lock().await.is_none());
	}
}
