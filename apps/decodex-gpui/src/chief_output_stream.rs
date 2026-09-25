//! One cancellable observation connection for the visible managed conversation.
use super::*;
use decodex_protocol::{ChiefLiveMessageDto, ChiefOutputResult};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct OutputStream {
	owner: Option<String>,
	task: Option<Task<()>>,
	ready: bool,
	messages: Vec<ChiefLiveMessageDto>,
	retry_after: Option<Instant>,
}

impl ChiefSurface {
	pub(super) fn observe_visible_output(&mut self, cx: &mut Context<Self>) {
		let Some(owner) = self.selected.clone().filter(|_| self.native_agents.selected.is_none())
		else {
			self.output_stream = OutputStream::default();
			return;
		};
		if self.output_stream.owner.as_ref() != Some(&owner) {
			self.output_stream = OutputStream { owner: Some(owner.clone()), ..Default::default() };
		}
		if self.output_stream.task.is_some()
			|| self.output_stream.retry_after.is_some_and(|t| t > Instant::now())
		{
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Ok(work) = EntityId::new(owner.clone()) else {
			return;
		};
		let (sender, mut receiver) = tokio::sync::watch::channel(None);
		let background = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			Some(runtime.block_on(ChiefClient::new(profile).observe_output(work, sender)))
		});
		self.output_stream.task = Some(cx.spawn(async move |surface, cx| {
			while receiver.changed().await.is_ok() {
				// The latest-value channel coalesces bursts without an unbounded delta queue.
				cx.background_executor().timer(Duration::from_millis(8)).await;
				let update = receiver.borrow_and_update().clone();
				let Some(ChiefOutputResult::Available { work_id, messages, .. }) = update else {
					continue;
				};
				let _ = surface.update(cx, |s, cx| {
					if s.selected.as_deref() != Some(work_id.as_str())
						|| s.output_stream.owner.as_ref() != Some(&owner)
					{
						return;
					}
					let changed = !s.output_stream.ready || s.output_stream.messages != messages;
					s.output_stream.ready = true;
					if changed {
						let needs_state = messages.first().is_some_and(|message| {
							s.snapshot
								.as_ref()
								.and_then(|snapshot| {
									snapshot.work_items.iter().find(|work| work.id == owner)
								})
								.and_then(|work| work.active_turn_id.as_ref())
								!= Some(&message.turn_id)
						});
						s.output_stream.messages = messages;
						if needs_state {
							s.refresh(cx);
						}
						cx.notify();
					}
				});
			}
			let _ = background.await;
			let _ = surface.update(cx, |s, cx| {
				if s.output_stream.owner.as_ref() == Some(&owner) {
					s.output_stream.task = None;
					s.output_stream.ready = false;
					s.output_stream.retry_after = Some(Instant::now() + Duration::from_secs(2));
					cx.notify();
				}
			});
		}));
	}

	pub(super) fn streamed_output<'a>(
		&'a self,
		work: &ChiefWorkItemDto,
	) -> Option<&'a [ChiefLiveMessageDto]> {
		(self.output_stream.ready
			&& self.output_stream.owner.as_ref() == Some(&work.id)
			&& work
				.active_turn_id
				.as_ref()
				.is_some_and(|turn| self.output_stream.messages.iter().any(|m| &m.turn_id == turn)))
		.then_some(self.output_stream.messages.as_slice())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn output_is_visible_only_for_its_owner_and_current_turn(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let mut work = s
				.snapshot
				.as_ref()
				.unwrap()
				.work_items
				.iter()
				.find(|w| w.id == "chief")
				.unwrap()
				.clone();
			work.active_turn_id = Some("current".into());
			s.output_stream = OutputStream {
				owner: Some(work.id.clone()),
				ready: true,
				messages: vec![ChiefLiveMessageDto {
					kind: Default::default(),
					turn_id: "old".into(),
					item_id: "answer".into(),
					text: "old output".into(),
					truncated: false,
				}],
				..Default::default()
			};
			assert!(s.streamed_output(&work).is_none());
			s.output_stream.messages[0].turn_id = "current".into();
			assert_eq!(s.streamed_output(&work).unwrap().len(), 1);
			s.output_stream.owner = Some("other-agent".into());
			assert!(s.streamed_output(&work).is_none());
			s.output_stream.owner = Some(work.id.clone());
			work.active_turn_id = None;
			assert!(
				s.streamed_output(&work).is_none(),
				"completed output must come from saved history"
			);
		});
	}
}
