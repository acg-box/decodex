//! Assemble selected request content without exposing incomplete actions to callers.
use super::{ChiefClient, ClientFailure, REQUEST_CLIENT_TIMEOUT, close_one_shot_socket};
use crate::{
	ChiefRequestResult as Request, ChiefRequestText, EntityId, QueryPayload, QueryResultPayload,
	WireText,
};
use tokio::time;

fn valid_owner(event_id: i64, expected: i64, work: &str, method: &str) -> bool {
	event_id == expected
		&& event_id > 0
		&& EntityId::new(work).is_ok()
		&& matches!(
			method,
			"item/commandExecution/requestApproval"
				| "item/fileChange/requestApproval"
				| "item/permissions/requestApproval"
				| "item/tool/requestUserInput"
				| "mcpServer/elicitation/request"
		)
}

pub(super) async fn collect(
	client: &ChiefClient,
	expected: i64,
	first: Request,
) -> Result<Request, ClientFailure> {
	let (owner, method, digest, total) = match &first {
		Request::Unavailable => return Ok(first),
		Request::Available { event_id, work_id, method, request_json } => {
			return if valid_owner(*event_id, expected, work_id, method)
				&& request_json.as_str().len() <= crate::MAX_HISTORY_INLINE_BYTES
			{
				Ok(first)
			} else {
				Err(ClientFailure::ProtocolMalformed)
			};
		},
		Request::Page { event_id, work_id, method, digest, total_bytes, .. } => {
			if !valid_owner(*event_id, expected, work_id, method)
				|| digest.len() != 64
				|| !digest.bytes().all(|b| b.is_ascii_hexdigit())
				|| *total_bytes <= crate::MAX_HISTORY_INLINE_BYTES
				|| *total_bytes > decodex_core::MAX_NATIVE_MESSAGE_BYTES
			{
				return Err(ClientFailure::ProtocolMalformed);
			}
			(work_id.clone(), method.clone(), digest.clone(), *total_bytes)
		},
	};
	let mut content = String::new();
	let mut page = first;
	loop {
		let Request::Page {
			event_id,
			work_id,
			method: returned_method,
			digest: returned_digest,
			offset,
			total_bytes,
			text,
			next_offset,
		} = page
		else {
			return if page == Request::Unavailable {
				Ok(page)
			} else {
				Err(ClientFailure::ProtocolMalformed)
			};
		};
		let end =
			offset.checked_add(text.as_str().len()).ok_or(ClientFailure::ProtocolMalformed)?;
		if event_id != expected
			|| work_id != owner
			|| returned_method != method
			|| returned_digest != digest
			|| total_bytes != total
			|| offset != content.len()
			|| text.as_str().is_empty()
			|| text.as_str().len() > 8192
			|| end > total
			|| next_offset != (end < total).then_some(end)
		{
			return Err(ClientFailure::ProtocolMalformed);
		}
		content.push_str(text.as_str());
		if next_offset.is_none() {
			let bytes = serde_json::to_vec(&(expected, &owner, &method, &content))
				.map_err(|_| ClientFailure::ProtocolMalformed)?;
			if decodex_core::BlobHash::digest(&bytes).to_hex() != digest {
				return Err(ClientFailure::ProtocolMalformed);
			}
			return Ok(Request::Available {
				event_id: expected,
				work_id: owner,
				method,
				request_json: ChiefRequestText::new(content)
					.map_err(|_| ClientFailure::ProtocolMalformed)?,
			});
		}
		let completed = time::timeout(
			REQUEST_CLIENT_TIMEOUT,
			client.transport.query_inner(
				"chief-request-page",
				QueryPayload::GetChiefRequestPage {
					event_id: expected,
					digest: WireText::new(digest.clone())
						.map_err(|_| ClientFailure::ProtocolMalformed)?,
					offset: end,
				},
			),
		)
		.await
		.map_err(|_| ClientFailure::ProtocolTimeout)??;
		close_one_shot_socket(completed.socket).await;
		let QueryResultPayload::ChiefRequest(next) = completed.value else {
			return Err(ClientFailure::ProtocolMalformed);
		};
		page = next;
	}
}
