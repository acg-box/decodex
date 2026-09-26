//! Durable input staging does not authorize native execution.
use super::{ChiefHost, ChiefHostError};
use decodex_database::ChiefPromptUpload;
use decodex_protocol::{
	ChiefActionDto as Action, PromptInputUpload, PromptInputUploadStatus as Status,
};

fn source(upload: &PromptInputUpload) -> Result<ChiefPromptUpload, ChiefHostError> {
	if !upload.is_valid() {
		return Err("Invalid prompt upload".into());
	}
	Ok(ChiefPromptUpload {
		upload_id: upload.upload_id.as_str().into(),
		work: upload.work_id.as_str().into(),
		thread: upload.thread_id.as_str().into(),
		edit_receipt_id: upload.edit_receipt_id,
		sha256: upload.sha256.as_str().into(),
		total_bytes: upload.total_bytes as i64,
	})
}

impl ChiefHost {
	pub(super) async fn send_prompt_input(
		&self,
		key: &str,
		action: Action,
		active: &mut Option<(
			String,
			super::ChiefCoordinator,
			tokio::sync::mpsc::Receiver<decodex_codex::app_server_client::ServerEvent>,
		)>,
	) -> Result<String, ChiefHostError> {
		let Action::SendPromptInput {
			work_id,
			thread_id,
			input_id,
			edit_receipt_id,
			sha256,
			execution,
		} = action
		else {
			return Err("Unsupported prompt input command".into());
		};
		let input = self
			.store
			.chief_prompt_input(input_id, work_id.as_str().into(), thread_id.as_str().into())
			.await
			.map_err(|_| "Prompt input is unavailable")?
			.ok_or("Prompt input is unavailable")?;
		if input.edit_receipt_id != edit_receipt_id || input.sha256 != sha256.as_str() {
			return Err("Prompt input source changed".into());
		}
		decodex_protocol::PromptDraft::new(input.content.clone())
			.map_err(|_| "Prompt input is invalid")?;
		let text: String = input
			.content
			.iter()
			.filter_map(|part| part.get("text").and_then(serde_json::Value::as_str))
			.flat_map(|text| text.chars().chain(std::iter::once('\n')))
			.take(2000)
			.collect();
		let preview = decodex_protocol::HistoryText::new(format!(
			"[Edited input preview; full content retained]\n{text}"
		))
		.map_err(|_| "Prompt preview is unavailable")?;
		let options = serde_json::json!({"execution":execution,"attachments":[],"taskReferences":[],
			"canonicalInput":{"id":input_id,"threadId":thread_id,"editReceiptId":edit_receipt_id,"sha256":sha256}});
		self.accept_message(&work_id, &preview, key, Some(&options), active).await
	}

	pub(super) async fn handle_prompt_upload(
		&self,
		action: Action,
	) -> Result<String, ChiefHostError> {
		match action {
			Action::UploadPromptInput { upload, offset, fragment } => {
				let source = source(&upload)?;
				let offset = i64::try_from(offset).map_err(|_| "Invalid prompt upload offset")?;
				self.store
					.append_chief_prompt_chunk(source, offset, fragment)
					.await
					.map_err(|_| "Prompt input chunk could not be saved")?;
				Ok(upload.work_id.as_str().into())
			},
			Action::CompletePromptInputUpload { upload } => {
				self.store
					.complete_chief_prompt_upload(source(&upload)?)
					.await
					.map_err(|_| "Prompt input could not be completed")?;
				Ok(upload.work_id.as_str().into())
			},
			_ => Err("Unsupported prompt upload command".into()),
		}
	}

	pub(crate) async fn prompt_upload_status(&self, upload: PromptInputUpload) -> Status {
		let Ok(source) = source(&upload) else {
			return Status::Unavailable { upload };
		};
		let bound = self
			.store
			.get_chief_work_item(source.work.clone())
			.await
			.is_ok_and(|work| work.codex_thread_id.as_deref() == Some(source.thread.as_str()));
		if !bound {
			return Status::Unavailable { upload };
		}
		// Check the current receipt before reporting reusable data for this source.
		let valid = self
			.store
			.chief_prompt_edit_receipt(source.work.clone(), source.thread.clone())
			.await
			.ok()
			.flatten()
			.is_some_and(|receipt| {
				receipt.id == source.edit_receipt_id
					&& matches!(receipt.state.as_str(), "applied" | "draft_restored")
			});
		if !valid {
			return Status::Unavailable { upload };
		}
		let received = match self.store.chief_prompt_upload_received(source.clone()).await {
			Ok(bytes) => bytes as u64,
			Err(_) => return Status::Unavailable { upload },
		};
		match self
			.store
			.chief_prompt_input_id(
				source.work,
				source.thread,
				source.edit_receipt_id,
				source.sha256,
				source.total_bytes,
			)
			.await
		{
			Ok(Some(input_id)) => Status::Ready { upload, input_id },
			Ok(None) => Status::Receiving { upload, received_bytes: received },
			Err(_) => Status::Unavailable { upload },
		}
	}
}
