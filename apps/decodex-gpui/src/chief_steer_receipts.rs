//! Resolve only an exact positive receipt; pending-list absence is not evidence.
use super::*;
#[cfg(test)] use decodex_protocol::ChiefSteerIdentity;
use decodex_protocol::ChiefSteerReceiptResult;

impl ChiefSurface {
	pub(super) fn refresh_steer_receipt(&mut self, cx: &mut Context<Self>) {
		if self.sending || !self.uncertain || self.submission.receipt_task.is_some() {
			return;
		}
		let Some(identity) =
			self.submission.pending.as_ref().and_then(|pending| pending.steer.clone())
		else {
			return;
		};
		let Some(profile) = self.profile.clone() else { return };
		let epoch = self.command_epoch;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).steer_receipt(identity)).ok()
		});
		self.submission.receipt_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				if surface.command_epoch != epoch {
					return;
				}
				surface.submission.receipt_task = None;
				if let Some(result) = result {
					surface.apply_steer_receipt(result, cx);
				}
			});
		}));
	}

	fn apply_steer_receipt(&mut self, result: ChiefSteerReceiptResult, cx: &mut Context<Self>) {
		let ChiefSteerReceiptResult::Confirmed { identity } = result else { return };
		if self.sending
			|| !self.uncertain
			|| self.submission.pending.as_ref().and_then(|pending| pending.steer.as_ref())
				!= Some(&identity)
		{
			return;
		}
		let Some(mut pending) = self.submission.pending.take() else { return };
		pending.epoch = self.command_epoch;
		self.uncertain =
			self.submission.unconfirmed.iter().any(|key| Some(key) != pending.key.as_ref());
		self.resolve_steer_draft_copies(&identity);
		self.finish_command(
			pending,
			Ok(ChiefCommandResponse::Accepted { work_id: identity.work_id }),
			cx,
		);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[gpui::test]
	fn exact_steer_receipt_preserves_later_edits_and_other_task_drafts(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		for mode in ["unchanged", "edited", "other-task"] {
			surface.update(visual, |s, cx| {
				let identity = ChiefSteerIdentity {
					work_id: EntityId::new("root").unwrap(),
					thread_id: WireText::new("thread").unwrap(),
					turn_id: WireText::new("turn").unwrap(),
					submission_id: IdempotencyKey::new("submission").unwrap(),
				};
				s.composer_manager =
					Some(if mode == "other-task" { "other" } else { "root" }.into());
				let text = if mode == "unchanged" { "original" } else { "later draft" };
				s.composer.update(cx, |input, cx| input.set_content(text, cx));
				s.draft_profiles.texts.insert("root".into(), "original".into());
				let file = |path| decodex_protocol::ChiefAttachmentDto {
					path: ConversationWorkingDirectory::new(path).unwrap(),
					image: true,
				};
				let sent = file("/tmp/sent.png");
				let later = file("/tmp/later.png");
				s.attachments = if mode == "other-task" {
					vec![later.clone()]
				} else {
					vec![sent.clone(), later.clone()]
				};
				s.draft_profiles.files.insert("root".into(), vec![sent.clone(), later.clone()]);
				s.uncertain = true;
				s.feedback = "Acceptance unknown".into();
				s.submission.pending = Some(PendingCommand {
					recovery: None,
					key: Some(identity.submission_id.clone()),
					steer: Some(identity.clone()),
					epoch: s.command_epoch,
					execution_intent: None,
					draft: Some("original".into()),
					owner: Some("root".into()),
					attachments: Some(vec![sent]),
					references: None,
				});
				for result in
					[ChiefSteerReceiptResult::Unconfirmed, ChiefSteerReceiptResult::Unavailable]
				{
					s.apply_steer_receipt(result, cx);
					assert!(s.uncertain);
					assert_eq!(s.composer.read(cx).content(), text);
				}
				let mut foreign = identity.clone();
				foreign.submission_id = IdempotencyKey::new("different").unwrap();
				s.apply_steer_receipt(ChiefSteerReceiptResult::Confirmed { identity: foreign }, cx);
				assert!(s.uncertain);
				s.apply_steer_receipt(ChiefSteerReceiptResult::Confirmed { identity }, cx);
				assert!(!s.uncertain && s.submission.pending.is_none());
				assert_eq!(s.attachments, vec![later.clone()]);
				assert_eq!(
					s.composer.read(cx).content(),
					if mode == "unchanged" { "" } else { text }
				);
				if mode == "other-task" {
					assert!(!s.draft_profiles.texts.contains_key("root"));
					assert_eq!(s.draft_profiles.files["root"], vec![later]);
				}
			});
		}
	}
}
