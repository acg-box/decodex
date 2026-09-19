//! Native resource associations. These are not model input uploads.
use super::{AppServerClient, ClientError, MAX_FRAME_BYTES};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

/// A resource owned by the native thread store, with an opaque application payload.
#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachment {
	/// Native stable attachment identity.
	pub id: String,
	/// Application-defined resource type.
	pub attachment_type: String,
	/// Exact identity within the resource type and owning thread.
	pub identity_key: String,
	/// Application-defined metadata, never interpreted as model input.
	pub payload: Value,
	/// Native creation timestamp in seconds.
	pub created_at: i64,
}

/// Whether this call created a resource or located its existing association.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadAttachmentAddOutcome {
	/// A new association was persisted.
	Created,
	/// The identity already existed; its original payload is returned unchanged.
	Existing,
}

/// Native add receipt. A receipt is not implied by a submitted request.
#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachmentAddResult {
	/// Native mutation outcome.
	pub outcome: ThreadAttachmentAddOutcome,
	/// Confirmed persisted association.
	pub attachment: ThreadAttachment,
}

fn identity(kind: &str, key: &str) -> Result<(), ClientError> {
	if kind.trim().is_empty() || key.trim().is_empty() || kind.len() > 256 || key.len() > 256 {
		return Err(ClientError::InvalidFrame);
	}
	Ok(())
}

fn thread_id(thread: &str) -> Result<(), ClientError> {
	if thread.is_empty() || thread.len() > 4096 {
		return Err(ClientError::InvalidFrame);
	}
	Ok(())
}

fn validate(attachment: &ThreadAttachment) -> Result<(), ClientError> {
	identity(&attachment.attachment_type, &attachment.identity_key)?;
	if attachment.id.is_empty() || attachment.id.len() > 4096 {
		return Err(ClientError::InvalidFrame);
	}
	if attachment.payload.to_string().len() > 64 * 1024 {
		return Err(ClientError::CapacityExceeded);
	}
	Ok(())
}

impl AppServerClient {
	/// Read all native attachment pages or return an error; never return a partial list.
	/// Unsupported stores remain remote errors, distinct from a confirmed empty list.
	pub async fn thread_attachments(
		&self,
		thread: &str,
	) -> Result<Vec<ThreadAttachment>, ClientError> {
		thread_id(thread)?;
		tokio::time::timeout(std::time::Duration::from_secs(30), async {
			let mut attachments = Vec::new();
			let mut cursor: Option<String> = None;
			let mut cursors = HashSet::new();
			let mut ids = HashSet::new();
			let mut identities = HashSet::new();
			let mut budget = MAX_FRAME_BYTES;
			for _ in 0..128 {
				let page = self
					.request(
						"thread/attachment/list",
						json!({"threadId":thread,"cursor":cursor,"limit":100}),
					)
					.await?;
				budget = budget
					.checked_sub(page.to_string().len())
					.ok_or(ClientError::CapacityExceeded)?;
				let data = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;
				if data.len() > 100 {
					return Err(ClientError::InvalidFrame);
				}
				for value in data {
					let attachment: ThreadAttachment = serde_json::from_value(value.clone())
						.map_err(|_| ClientError::InvalidFrame)?;
					validate(&attachment)?;
					if !ids.insert(attachment.id.clone())
						|| !identities.insert((
							attachment.attachment_type.clone(),
							attachment.identity_key.clone(),
						)) {
						return Err(ClientError::InvalidFrame);
					}
					attachments.push(attachment);
				}
				match page.get("nextCursor") {
					Some(Value::Null) => return Ok(attachments),
					Some(Value::String(next))
						if !next.is_empty()
							&& next.len() <= 4096 && cursors.insert(next.clone()) =>
						cursor = Some(next.clone()),
					_ => return Err(ClientError::InvalidFrame),
				}
			}
			Err(ClientError::CapacityExceeded)
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Submit one add. Never retry an uncertain response or overwrite existing metadata.
	pub async fn add_thread_attachment(
		&self,
		thread: &str,
		kind: &str,
		key: &str,
		payload: Value,
	) -> Result<ThreadAttachmentAddResult, ClientError> {
		thread_id(thread)?;
		identity(kind, key)?;
		if payload.to_string().len() > 64 * 1024 {
			return Err(ClientError::CapacityExceeded);
		}
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(30),
			self.request(
				"thread/attachment/add",
				json!({"threadId":thread,"attachmentType":kind,"identityKey":key,"payload":payload}),
			),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let receipt: ThreadAttachmentAddResult =
			serde_json::from_value(response).map_err(|_| ClientError::InvalidFrame)?;
		validate(&receipt.attachment)?;
		if receipt.attachment.attachment_type != kind
			|| receipt.attachment.identity_key != key
			|| (receipt.outcome == ThreadAttachmentAddOutcome::Created
				&& receipt.attachment.payload != payload)
		{
			return Err(ClientError::InvalidFrame);
		}
		Ok(receipt)
	}

	/// Submit one removal. Transport uncertainty requires readback, not automatic replay.
	pub async fn remove_thread_attachment(
		&self,
		thread: &str,
		kind: &str,
		key: &str,
	) -> Result<(), ClientError> {
		thread_id(thread)?;
		identity(kind, key)?;
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(30),
			self.request(
				"thread/attachment/remove",
				json!({"threadId":thread,"attachmentType":kind,"identityKey":key}),
			),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if response != json!({}) {
			return Err(ClientError::InvalidFrame);
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	fn attachment(id: &str, key: &str) -> Value {
		json!({"id":id,"attachmentType":"example.resource","identityKey":key,"payload":{"title":"Original"},"createdAt":1})
	}

	fn fixture(replies: Vec<Value>) -> (AppServerClient, tokio::task::JoinHandle<Vec<Value>>) {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let mut requests = Vec::new();
			for mut reply in replies {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				reply["id"] = request["id"].clone();
				writer.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
				requests.push(request);
			}
			requests
		});
		(client, server)
	}

	#[tokio::test]
	async fn list_and_mutations_keep_exact_identity_and_existing_payload() {
		let (client, server) = fixture(vec![
			json!({"result":{"data":[attachment("a","one")],"nextCursor":"opaque/next"}}),
			json!({"result":{"data":[attachment("b","two")],"nextCursor":null}}),
			json!({"result":{"outcome":"existing","attachment":attachment("a","one")}}),
			json!({"result":{}}),
		]);
		let rows = client.thread_attachments("thread/exact").await.unwrap();
		assert_eq!(rows.len(), 2);
		assert_eq!(rows[1].id, "b");
		let receipt = client
			.add_thread_attachment(
				"thread/exact",
				"example.resource",
				"one",
				json!({"title":"Replacement"}),
			)
			.await
			.unwrap();
		assert_eq!(receipt.outcome, ThreadAttachmentAddOutcome::Existing);
		assert_eq!(receipt.attachment.payload, json!({"title":"Original"}));
		client.remove_thread_attachment("thread/exact", "example.resource", "one").await.unwrap();
		let requests = server.await.unwrap();
		assert_eq!(requests.len(), 4);
		assert_eq!(requests[0]["method"], "thread/attachment/list");
		assert_eq!(requests[1]["params"]["cursor"], "opaque/next");
		assert_eq!(requests[2]["method"], "thread/attachment/add");
		assert_eq!(requests[3]["method"], "thread/attachment/remove");
		for request in requests {
			assert_eq!(request["params"]["threadId"], "thread/exact");
		}
	}

	#[tokio::test]
	async fn incomplete_or_duplicate_pages_are_not_partial_success() {
		for second in [
			json!({"data":[attachment("a","one")],"nextCursor":null}),
			json!({"data":[],"nextCursor":"again"}),
			json!({"data":[]}),
		] {
			let (client, server) = fixture(vec![
				json!({"result":{"data":[attachment("a","one")],"nextCursor":"again"}}),
				json!({"result":second}),
			]);
			assert!(matches!(
				client.thread_attachments("thread").await,
				Err(ClientError::InvalidFrame)
			));
			assert_eq!(server.await.unwrap().len(), 2);
		}
	}

	#[tokio::test]
	async fn unsupported_store_is_not_empty_and_mutations_do_not_retry() {
		let (client, server) =
			fixture(vec![json!({"error":{"code":-32601,"message":"unsupported store"}})]);
		assert!(
			matches!(client.thread_attachments("thread").await,Err(ClientError::Remote(error)) if error.code == -32601)
		);
		assert_eq!(server.await.unwrap().len(), 1);
		let (client, server) = fixture(vec![
			json!({"result":{"outcome":"created","attachment":attachment("a","wrong-key")}}),
		]);
		assert!(matches!(
			client.add_thread_attachment("thread", "example.resource", "one", json!({})).await,
			Err(ClientError::InvalidFrame)
		));
		assert_eq!(server.await.unwrap().len(), 1);
	}
}
