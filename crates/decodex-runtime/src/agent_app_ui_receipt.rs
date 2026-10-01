//! Bounded durable readback. Reads never repeat a native tool call.
use sha2::{Digest, Sha256};

use decodex_database::SqliteStore;
use decodex_protocol::{
	AGENT_APP_UI_RECEIPT_CHUNK_BYTES, AgentAppUiReceiptRequest, AgentAppUiReceiptResult,
	AgentPendingAppUiCall, EntityId, MAX_AGENT_APP_UI_RECEIPT_BYTES,
};

pub(crate) async fn pending(store: &SqliteStore, work_id: &EntityId) -> AgentPendingAppUiCall {
	match store.pending_agent_app_ui_call(work_id.as_str().into()).await {
		Ok(receipt) => {
			let operation_id = match receipt {
				Some(receipt) => match EntityId::new(receipt.attempt.attempt_id) {
					Ok(id) => Some(id),
					Err(_) => return AgentPendingAppUiCall::Unavailable,
				},
				None => None,
			};

			AgentPendingAppUiCall::Available { work_id: work_id.clone(), operation_id }
		},
		Err(_) => AgentPendingAppUiCall::Unavailable,
	}
}

pub(crate) async fn read(
	store: &SqliteStore,
	request: &AgentAppUiReceiptRequest,
) -> AgentAppUiReceiptResult {
	if request.offset as usize >= MAX_AGENT_APP_UI_RECEIPT_BYTES
		|| (request.offset > 0 && request.fingerprint.is_none())
	{
		return AgentAppUiReceiptResult::Unavailable;
	}
	if store
		.recover_agent_app_ui_call(
			request.work_id.as_str().into(),
			request.operation_id.as_str().into(),
		)
		.await
		.is_err()
	{
		return AgentAppUiReceiptResult::Unavailable;
	}

	let Ok(Some(receipt)) = store
		.agent_app_ui_call_receipt(
			request.work_id.as_str().into(),
			request.operation_id.as_str().into(),
		)
		.await
	else {
		return AgentAppUiReceiptResult::Unavailable;
	};
	let a = receipt.attempt;
	let document = serde_json::json!({"reservationId":receipt.id,"operationId":a.attempt_id,"workId":a.owner.work,"threadId":a.owner.thread,"accountId":a.owner.account,"sourceFingerprint":a.source_fingerprint,"turnId":a.turn,"itemId":a.item,"server":a.server,"tool":a.tool,"arguments":a.arguments,"state":receipt.state,"uncertaintyAcknowledged":receipt.uncertainty_acknowledged,"result":receipt.result});

	chunk(request, serde_json::to_vec(&document).expect("saved JSON serializes"))
}

fn chunk(request: &AgentAppUiReceiptRequest, document: Vec<u8>) -> AgentAppUiReceiptResult {
	if document.len() > MAX_AGENT_APP_UI_RECEIPT_BYTES {
		return AgentAppUiReceiptResult::CapacityExceeded;
	}

	let fingerprint = EntityId::new(
		Sha256::digest(&document).iter().map(|b| format!("{b:02x}")).collect::<String>(),
	)
	.expect("digest identity");

	if request.fingerprint.as_ref().is_some_and(|expected| expected != &fingerprint)
		|| request.offset as usize >= document.len()
	{
		return AgentAppUiReceiptResult::Unavailable;
	}

	let start = request.offset as usize;
	let end = (start + AGENT_APP_UI_RECEIPT_CHUNK_BYTES).min(document.len());

	AgentAppUiReceiptResult::Available {
		request: Box::new(request.clone()),
		fingerprint,
		total_bytes: document.len() as u32,
		bytes: document[start..end].to_vec(),
	}
}

#[cfg(test)]
mod tests {
	use crate::agent_app_ui_receipt::{
		self, AGENT_APP_UI_RECEIPT_CHUNK_BYTES, AgentAppUiReceiptRequest, AgentAppUiReceiptResult,
		EntityId,
	};

	#[test]
	fn result_chunks_cannot_mix_saved_outcomes() {
		let mut request = AgentAppUiReceiptRequest {
			work_id: EntityId::new("work").unwrap(),
			operation_id: EntityId::new("operation").unwrap(),
			offset: 0,
			fingerprint: None,
		};
		let document = vec![b'a'; AGENT_APP_UI_RECEIPT_CHUNK_BYTES + 7];
		let AgentAppUiReceiptResult::Available { fingerprint, bytes, .. } =
			agent_app_ui_receipt::chunk(&request, document.clone())
		else {
			panic!("first chunk")
		};

		assert_eq!(bytes.len(), AGENT_APP_UI_RECEIPT_CHUNK_BYTES);

		request.offset = bytes.len() as u32;
		request.fingerprint = Some(fingerprint);

		assert!(
			matches!(agent_app_ui_receipt::chunk(&request,document.clone()),AgentAppUiReceiptResult::Available{bytes,..} if bytes.len()==7)
		);

		let mut changed = document;

		changed[0] = b'b';

		assert_eq!(
			agent_app_ui_receipt::chunk(&request, changed),
			AgentAppUiReceiptResult::Unavailable
		);
	}
}
