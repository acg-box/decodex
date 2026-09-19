//! Display native associations without creating a second persistence owner.
use decodex_codex::app_server_client::{AppServerClient, ClientError, ThreadAttachment};
use decodex_protocol::{ChiefResourceDto, ChiefResourcesResult};

pub(crate) async fn read(client: &AppServerClient, thread: &str) -> ChiefResourcesResult {
	match client.thread_attachments(thread).await {
		Ok(resources) => project(resources),
		Err(ClientError::Remote(error)) if error.code == -32601 =>
			ChiefResourcesResult::Unsupported,
		Err(ClientError::CapacityExceeded) => ChiefResourcesResult::CapacityExceeded,
		Err(_) => ChiefResourcesResult::Unavailable,
	}
}

fn project(resources: Vec<ThreadAttachment>) -> ChiefResourcesResult {
	if resources.len() > 128 {
		return ChiefResourcesResult::CapacityExceeded;
	}
	let mut projected = Vec::new();
	for resource in resources {
		let payload = resource.payload.to_string();
		let omitted =
			payload.len() > 16 * 1024 || decodex_core::contains_credential_material(&payload);
		projected.push(ChiefResourceDto {
			id: resource.id,
			attachment_type: resource.attachment_type,
			identity_key: resource.identity_key,
			payload_json: if omitted { String::new() } else { payload },
			payload_omitted: omitted,
			created_at: resource.created_at,
		});
	}
	let result = ChiefResourcesResult::Available { resources: projected };
	if serde_json::to_vec(&result).map_or(true, |bytes| bytes.len() > 64 * 1024) {
		ChiefResourcesResult::CapacityExceeded
	} else {
		result
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	fn resource(payload: serde_json::Value) -> ThreadAttachment {
		ThreadAttachment {
			id: "native-resource".into(),
			attachment_type: "example.resource".into(),
			identity_key: "one".into(),
			payload,
			created_at: 1,
		}
	}
	#[test]
	fn complete_list_keeps_identity_and_marks_unavailable_payloads() {
		let empty = project(vec![]);
		assert!(matches!(empty,ChiefResourcesResult::Available{resources} if resources.is_empty()));
		let result = project(vec![
			resource(json!({"title":"Review"})),
			resource(json!({"text":"x".repeat(17000)})),
		]);
		let ChiefResourcesResult::Available { resources } = result else {
			panic!("complete list");
		};
		assert_eq!(resources.len(), 2);
		assert_eq!(resources[0].identity_key, "one");
		assert_eq!(resources[0].payload_json, "{\"title\":\"Review\"}");
		assert!(!resources[0].payload_omitted);
		assert!(resources[1].payload_omitted);
		assert!(resources[1].payload_json.is_empty());
	}
	#[test]
	fn oversized_list_is_not_reported_as_partial_or_empty() {
		assert_eq!(
			project((0..129).map(|_| resource(json!({}))).collect()),
			ChiefResourcesResult::CapacityExceeded
		);
		assert_eq!(
			project((0..8).map(|_| resource(json!({"text":"x".repeat(10000)}))).collect()),
			ChiefResourcesResult::CapacityExceeded
		);
	}
}

pub(crate) async fn add_link(
	client: &AppServerClient,
	thread: &str,
	title: &str,
	url: &str,
) -> Result<(), crate::chief::ChiefError> {
	use crate::chief::ChiefError;
	use sha2::{Digest, Sha256};
	let url = reqwest::Url::parse(url)
		.map_err(|_| ChiefError::Rejected("Invalid resource URL".into()))?;
	if !matches!(url.scheme(), "https" | "http")
		|| url.host_str().is_none()
		|| !url.username().is_empty()
		|| url.password().is_some()
		|| title.trim().is_empty()
	{
		return Err(ChiefError::Rejected(
			"Use a title and an HTTP or HTTPS link without embedded credentials".into(),
		));
	}
	let url = url.to_string();
	// This is a Decodex convention, not an upstream-reserved type. Native add keeps
	// the first title for an existing normalized URL rather than silently replacing it.
	let digest =
		Sha256::digest(url.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
	let key = format!("sha256:{digest}");
	client
		.add_thread_attachment(
			thread,
			"decodex.link",
			&key,
			serde_json::json!({"title":title,"url":url}),
		)
		.await?;
	Ok(())
}

#[cfg(test)]
mod link_tests {
	use super::*;
	use serde_json::{Value, json};
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[tokio::test]
	async fn links_use_stable_native_identity_without_replacing_existing_metadata() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let mut first = None;
			for index in 0..2 {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "thread/attachment/add");
				assert_eq!(request["params"]["threadId"], "thread-exact");
				assert_eq!(request["params"]["attachmentType"], "decodex.link");
				assert_eq!(request["params"]["payload"]["url"], "https://example.test/");
				let current = json!({"id":"resource","attachmentType":request["params"]["attachmentType"],"identityKey":request["params"]["identityKey"],"payload":request["params"]["payload"],"createdAt":1});
				let original = first.get_or_insert(current);
				assert_eq!(original["identityKey"], request["params"]["identityKey"]);
				let reply = json!({"id":request["id"],"result":{"outcome":if index==0 {"created"} else {"existing"},"attachment":original}});
				writer.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
			}
		});
		for url in ["file:///tmp/private", "https://user:password@example.test/", "not a URL"] {
			assert!(matches!(
				add_link(&client, "thread-exact", "Title", url).await,
				Err(crate::chief::ChiefError::Rejected(_))
			));
		}
		add_link(&client, "thread-exact", "Original", "https://EXAMPLE.test").await.unwrap();
		add_link(&client, "thread-exact", "Changed", "https://example.test/").await.unwrap();
		server.await.unwrap();
	}
}
