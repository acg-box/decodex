//! One native resource-owner page without scanning conversation history.
use serde::{Deserialize, Serialize};

use super::{AppServerClient, ClientError};

/// Native conversation that currently has the exact resource association.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachmentOwner {
	/// Native conversation identity.
	pub thread_id: String,
	/// Native archive membership.
	pub archived: bool,
}

/// One native reverse-lookup page.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachmentOwnerPage {
	/// Matching registered associations.
	pub data: Vec<ThreadAttachmentOwner>,
	/// Native continuation for the same resource and filter.
	pub next_cursor: Option<String>,
}

impl AppServerClient {
	/// Query an existing attachment identity in this server's store. Do not upload or index files.
	pub async fn thread_attachment_owners(
		&self,
		kind: &str,
		key: &str,
		archived: Option<bool>,
		cursor: Option<&str>,
	) -> Result<ThreadAttachmentOwnerPage, ClientError> {
		if [Some(kind), Some(key), cursor]
			.into_iter()
			.flatten()
			.any(|v| v.is_empty() || v.len() > 4096 || v.chars().any(char::is_control))
		{
			return Err(ClientError::InvalidFrame);
		}
		let value = self
			.request(
				"thread/attachmentOwner/list",
				serde_json::json!({
					"attachmentType":kind,"identityKey":key,"archived":archived,"cursor":cursor,"limit":50
				}),
			)
			.await?;
		let page: ThreadAttachmentOwnerPage =
			serde_json::from_value(value).map_err(|_| ClientError::InvalidFrame)?;
		let mut ids = std::collections::HashSet::new();
		if page.data.len() > 50
			|| page.data.iter().any(|owner| {
				owner.thread_id.is_empty()
					|| owner.thread_id.len() > 512
					|| owner.thread_id.chars().any(char::is_control)
					|| !ids.insert(&owner.thread_id)
					|| archived.is_some_and(|filter| filter != owner.archived)
			})
			|| page
				.next_cursor
				.as_deref()
				.is_some_and(|next| next.is_empty() || next.len() > 4096 || Some(next) == cursor)
		{
			return Err(ClientError::InvalidFrame);
		}
		Ok(page)
	}
}

#[cfg(test)]
mod tests {
	use super::AppServerClient;
	use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	#[tokio::test]
	async fn reverse_lookup_preserves_identity_archive_filter_and_opaque_page() {
		for invalid in [false, true] {
			let (local, remote) = tokio::io::duplex(4096);
			let (read, write) = tokio::io::split(local);
			let (client, _) = AppServerClient::from_io(read, write);
			let server = tokio::spawn(async move {
				let (read, mut write) = tokio::io::split(remote);
				let request: serde_json::Value = serde_json::from_str(
					&BufReader::new(read).lines().next_line().await.unwrap().unwrap(),
				)
				.unwrap();
				assert_eq!(request["method"], "thread/attachmentOwner/list");
				assert_eq!(
					request["params"],
					serde_json::json!({"attachmentType":"decodex.link","identityKey":"same-key","archived":true,"cursor":"opaque-page","limit":50})
				);
				let response = serde_json::json!({"id":request["id"],"result":{"data":[{"threadId":"archived-owner","archived":!invalid}],"nextCursor":"next-page"}});
				write.write_all(format!("{response}\n").as_bytes()).await.unwrap();
			});
			let result = client
				.thread_attachment_owners(
					"decodex.link",
					"same-key",
					Some(true),
					Some("opaque-page"),
				)
				.await;
			assert_eq!(result.is_err(), invalid);
			if let Ok(page) = result {
				assert_eq!(page.data[0].thread_id, "archived-owner");
				assert_eq!(page.next_cursor.as_deref(), Some("next-page"));
			}
			client.close();
			server.await.unwrap();
		}
	}
}
