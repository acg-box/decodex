//! Bounded durable readback. Reads never repeat a native tool call.
use decodex_database::SqliteStore;
use decodex_protocol::{
	CHIEF_APP_UI_RECEIPT_CHUNK_BYTES, ChiefAppUiReceiptRequest, ChiefAppUiReceiptResult as Result,
	EntityId, MAX_CHIEF_APP_UI_RECEIPT_BYTES,
};
use serde_json::json;
use sha2::{Digest, Sha256};

pub(crate) async fn pending(
	store: &SqliteStore,
	work_id: &EntityId,
) -> decodex_protocol::ChiefPendingAppUiCall {
	use decodex_protocol::ChiefPendingAppUiCall;
	match store.pending_chief_app_ui_call(work_id.as_str().into()).await {
		Ok(receipt) => {
			let operation_id = match receipt {
				Some(receipt) => match EntityId::new(receipt.attempt.attempt_id) {
					Ok(id) => Some(id),
					Err(_) => return ChiefPendingAppUiCall::Unavailable,
				},
				None => None,
			};
			ChiefPendingAppUiCall::Available { work_id: work_id.clone(), operation_id }
		},
		Err(_) => ChiefPendingAppUiCall::Unavailable,
	}
}

pub(crate) async fn read(store: &SqliteStore, request: &ChiefAppUiReceiptRequest) -> Result {
	if request.offset as usize >= MAX_CHIEF_APP_UI_RECEIPT_BYTES
		|| (request.offset > 0 && request.fingerprint.is_none())
	{
		return Result::Unavailable;
	}
	if store
		.recover_chief_app_ui_call(
			request.work_id.as_str().into(),
			request.operation_id.as_str().into(),
		)
		.await
		.is_err()
	{
		return Result::Unavailable;
	}
	let Ok(Some(receipt)) = store
		.chief_app_ui_call_receipt(
			request.work_id.as_str().into(),
			request.operation_id.as_str().into(),
		)
		.await
	else {
		return Result::Unavailable;
	};
	let a = receipt.attempt;
	let document = json!({"reservationId":receipt.id,"operationId":a.attempt_id,"workId":a.owner.work,"threadId":a.owner.thread,"accountId":a.owner.account,"sourceFingerprint":a.source_fingerprint,"turnId":a.turn,"itemId":a.item,"server":a.server,"tool":a.tool,"arguments":a.arguments,"state":receipt.state,"uncertaintyAcknowledged":receipt.uncertainty_acknowledged,"result":receipt.result});
	chunk(request, serde_json::to_vec(&document).expect("saved JSON serializes"))
}

fn chunk(request: &ChiefAppUiReceiptRequest, document: Vec<u8>) -> Result {
	if document.len() > MAX_CHIEF_APP_UI_RECEIPT_BYTES {
		return Result::CapacityExceeded;
	}
	let fingerprint = EntityId::new(
		Sha256::digest(&document).iter().map(|b| format!("{b:02x}")).collect::<String>(),
	)
	.expect("digest identity");
	if request.fingerprint.as_ref().is_some_and(|expected| expected != &fingerprint)
		|| request.offset as usize >= document.len()
	{
		return Result::Unavailable;
	}
	let start = request.offset as usize;
	let end = (start + CHIEF_APP_UI_RECEIPT_CHUNK_BYTES).min(document.len());
	Result::Available {
		request: Box::new(request.clone()),
		fingerprint,
		total_bytes: document.len() as u32,
		bytes: document[start..end].to_vec(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn result_chunks_cannot_mix_saved_outcomes() {
		let mut request = ChiefAppUiReceiptRequest {
			work_id: EntityId::new("work").unwrap(),
			operation_id: EntityId::new("operation").unwrap(),
			offset: 0,
			fingerprint: None,
		};
		let document = vec![b'a'; CHIEF_APP_UI_RECEIPT_CHUNK_BYTES + 7];
		let Result::Available { fingerprint, bytes, .. } = chunk(&request, document.clone()) else {
			panic!("first chunk")
		};
		assert_eq!(bytes.len(), CHIEF_APP_UI_RECEIPT_CHUNK_BYTES);
		request.offset = bytes.len() as u32;
		request.fingerprint = Some(fingerprint);
		assert!(
			matches!(chunk(&request,document.clone()),Result::Available{bytes,..} if bytes.len()==7)
		);
		let mut changed = document;
		changed[0] = b'b';
		assert_eq!(chunk(&request, changed), Result::Unavailable);
	}
}
