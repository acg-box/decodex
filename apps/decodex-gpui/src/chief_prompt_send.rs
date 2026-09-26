//! Explicit canonical send. Unknown results are reconciled by reads, never replayed.
use super::*;
#[derive(Clone, Copy)]
enum Outcome {
	Accepted,
	Rejected,
	Unknown,
}

impl ChiefSurface {
	pub(super) fn send_prompt_editor(
		&mut self,
		expected: DesktopPromptEditDraft,
		checking: bool,
		cx: &mut Context<Self>,
	) {
		if !self.prompt_send_eligible(&expected, checking) {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let source_profile = profile.clone();
		let execution = self.draft_profiles.execution.choice(expected.work_id.as_str());
		let expected_execution = execution.clone();
		let worker_draft = expected.clone();
		let panel_key = self.prompt_edit.key.clone();
		let command_key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let (staged, stage) = tokio::sync::oneshot::channel();
		let (permit, permitted) = tokio::sync::oneshot::channel();
		let (completed, completion) = tokio::sync::oneshot::channel();
		let started = std::thread::Builder::new().name("prompt-send-io".into()).spawn(move || {
			let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build()
			else {
				let _ = staged.send(Err("Could not start input submission"));
				return;
			};
			runtime.block_on(send_worker(
				ChiefClient::new(profile),
				worker_draft,
				execution,
				command_key,
				checking,
				(staged, permitted, completed),
			));
		});
		if started.is_err() {
			self.prompt_edit.feedback = "Could not start input submission".into();
			cx.notify();
			return;
		}
		self.prompt_edit.feedback = if checking {
			"Reading the original send receipt…"
		} else {
			"Preparing the complete edited input…"
		}
		.into();
		self.prompt_edit.task = Some(cx.spawn(async move |surface, cx| {
			let result =
				stage.await.unwrap_or(Err("Input preparation stopped. Nothing was submitted."));
			let pending = surface
				.update(cx, |s, cx| {
					if s.prompt_edit.key != panel_key || !s.prompt_editor_source_current() {
						return None;
					}
					let result = result.and_then(|pending| {
						if s.prompt_edit.draft.as_ref() != Some(&expected)
							|| (!checking
								&& s.draft_profiles.execution.choice(expected.work_id.as_str())
									!= expected_execution)
						{
							return Err("Draft or settings changed. Nothing was submitted.");
						}
						s.stage_prompt_editor(pending.clone(), cx)?;
						s.prompt_edit.draft = Some(pending.clone());
						if !checking {
							s.prompt_edit.prepared_send =
								Some((source_profile.clone(), pending.clone()));
						}
						Ok(pending)
					});
					match result {
						Ok(pending) => {
							s.prompt_edit.feedback = "Saving the exact send record…".into();
							cx.notify();
							Some(pending)
						},
						Err(message) => {
							s.prompt_edit.task = None;
							s.prompt_edit.feedback = message.into();
							cx.notify();
							None
						},
					}
				})
				.ok()
				.flatten();
			let Some(pending) = pending else {
				return;
			};
			let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
			loop {
				let ready = surface
					.update(cx, |s, cx| {
						s.prompt_send_ready(&panel_key, &pending, checking, deadline, cx)
					})
					.unwrap_or(Some(false));
				if let Some(ready) = ready {
					if !ready {
						return;
					}
					break;
				}
				cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
			}
			if permit.send(()).is_err() {
				let _ = surface.update(cx, |s, cx| {
					if s.prompt_edit.key == panel_key {
						s.finish_prompt_send(
							&pending,
							if checking { Outcome::Unknown } else { Outcome::Rejected },
							cx,
						);
					}
				});
				return;
			}
			let outcome = completion.await.unwrap_or(Outcome::Unknown);
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.key == panel_key && s.prompt_editor_source_current() {
					s.finish_prompt_send(&pending, outcome, cx);
				}
			});
		}));
		cx.notify();
	}

	fn prompt_send_eligible(&self, expected: &DesktopPromptEditDraft, checking: bool) -> bool {
		!(self.prompt_edit.task.is_some()
			|| !self.prompt_editor_source_current()
			|| self.prompt_edit.draft.as_ref() != Some(expected)
			|| expected.receipt_id.is_none()
			|| expected.handback_pending
			|| expected.pending_send.is_some() != checking
			|| !self.command_connection_ready())
	}

	fn prompt_send_ready(
		&mut self,
		panel_key: &str,
		pending: &DesktopPromptEditDraft,
		checking: bool,
		deadline: std::time::Instant,
		cx: &mut Context<Self>,
	) -> Option<bool> {
		if self.prompt_edit.key != panel_key || !self.prompt_editor_source_current() {
			return Some(false);
		}
		if self.prompt_edit.draft.as_ref() != Some(pending)
			|| !self.command_connection_ready()
			|| std::time::Instant::now() >= deadline
		{
			self.finish_prompt_send(
				pending,
				if checking { Outcome::Unknown } else { Outcome::Rejected },
				cx,
			);
			return Some(false);
		}
		if checking || self.prompt_editor_saved(pending) {
			// No UI yield occurs between removing this known-unsent marker and
			// granting the worker permit.
			self.prompt_edit.prepared_send = None;
			return Some(true);
		}
		None
	}

	fn finish_prompt_send(
		&mut self,
		pending: &DesktopPromptEditDraft,
		outcome: Outcome,
		cx: &mut Context<Self>,
	) {
		self.prompt_edit.task = None;
		self.prompt_edit.prepared_send = None;
		if self.prompt_edit.draft.as_ref() != Some(pending) {
			return;
		}
		if matches!(outcome, Outcome::Unknown) {
			self.prompt_edit.feedback = "Send result is unknown. Input retained; check its receipt. It will not be sent again automatically.".into();
			cx.notify();
			return;
		}
		match self.settle_prompt_send(pending, matches!(outcome, Outcome::Accepted), cx) {
			Ok(None) => {
				self.reset_prompt_edit();
				self.feedback =
					"Edited input accepted. A complete recovery copy was retained.".into();
				self.history_requested_for = None;
				self.load_history(cx);
			},
			Ok(Some(draft)) => {
				self.prompt_edit.draft = Some(draft);
				self.prompt_edit.feedback = "Input was not submitted. Draft retained.".into();
			},
			Err(message) => self.prompt_edit.feedback = message.into(),
		}
		cx.notify();
	}
}

async fn send_worker(
	client: ChiefClient,
	worker_draft: DesktopPromptEditDraft,
	execution: decodex_protocol::ChiefExecutionOverrides,
	command_key: IdempotencyKey,
	checking: bool,
	channels: (
		tokio::sync::oneshot::Sender<Result<DesktopPromptEditDraft, &'static str>>,
		tokio::sync::oneshot::Receiver<()>,
		tokio::sync::oneshot::Sender<Outcome>,
	),
) {
	let (staged, permitted, completed) = channels;
	let result = async {
		if checking {
			return Ok(worker_draft);
		}
		super::confirmation::readable_local_media(&worker_draft.input)?;
		client
			.preflight_prompt_input(
				worker_draft.work_id.clone(),
				worker_draft.thread_id.clone(),
				&worker_draft.input,
				&execution,
			)
			.await
			.map_err(|_| "Edited input could not be qualified. Nothing was sent.")?;
		let receipt = worker_draft.receipt_id.ok_or("Missing edit receipt")?;
		let hash = worker_draft.input.fingerprint()?;
		let upload = decodex_protocol::PromptInputUpload {
			work_id: worker_draft.work_id.clone(),
			thread_id: worker_draft.thread_id.clone(),
			edit_receipt_id: receipt,
			upload_id: IdempotencyKey::new(format!("prompt-{receipt}-{}", hash.as_str()))
				.map_err(|_| "Invalid upload identity")?,
			sha256: hash,
			total_bytes: serde_json::to_vec(&worker_draft.input)
				.map_err(|_| "Input encoding failed")?
				.len() as u64,
		};
		let input_id = client
			.stage_prompt_input(upload, &worker_draft.input)
			.await
			.map_err(|_| "Input upload stopped. Draft retained; no model input was submitted.")?;
		worker_draft.begin_send(input_id, command_key, execution)
	}
	.await;
	let Ok(pending) = result else {
		let _ = staged.send(result);
		return;
	};
	if staged.send(Ok(pending.clone())).is_err() || permitted.await.is_err() {
		return;
	}
	let identity = pending.send_identity().expect("validated pending send");
	let outcome = if checking {
		Outcome::Unknown
	} else {
		match client
			.execute(
				ChiefActionDto::SendPromptInput {
					work_id: identity.work_id.clone(),
					thread_id: identity.thread_id.clone(),
					input_id: identity.send.input_id,
					edit_receipt_id: identity.edit_receipt_id,
					sha256: identity.send.sha256.clone(),
					execution: identity.send.execution.clone(),
				},
				identity.send.command_key.clone(),
			)
			.await
		{
			Ok(ChiefCommandResponse::Accepted { .. }) => Outcome::Accepted,
			Ok(ChiefCommandResponse::Rejected { .. }) | Err(_) => Outcome::Rejected,
			Ok(ChiefCommandResponse::PotentiallyDispatched { .. }) => Outcome::Unknown,
		}
	};
	let outcome = if matches!(outcome, Outcome::Unknown)
		&& client
			.prompt_input_send_status(identity)
			.await
			.is_ok_and(|status| status.accepted_event_id.is_some())
	{
		Outcome::Accepted
	} else {
		outcome
	};
	let _ = completed.send(outcome);
}
