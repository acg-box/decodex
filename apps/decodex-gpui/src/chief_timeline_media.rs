//! One explicit native image preview, bounded and tied to the selected source.
use super::*;
use decodex_protocol::{ChiefMediaRequest, ChiefMediaResult};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Preview {
	request: Option<ChiefMediaRequest>,
	image: Option<Arc<gpui::Image>>,
	notice: Option<&'static str>,
	task: Option<Task<()>>,
	serial: u64,
}

impl Preview {
	pub(super) fn clear(&mut self) {
		*self = Self { serial: self.serial.wrapping_add(1), ..Default::default() };
	}
}

impl ChiefSurface {
	pub(super) fn native_attachment(
		&self,
		work: &ChiefWorkItemDto,
		turn: &str,
		item: &str,
		attachment: &decodex_protocol::ChiefTimelineAttachment,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let caption = render::attachment_caption(attachment);
		let Some(request) = media_request(work, turn, item, attachment.index) else {
			return muted(caption).into_any_element();
		};
		let preview = &self.native_history.preview;
		let selected = preview.request.as_ref() == Some(&request);
		let can_preview = attachment.source
			!= decodex_protocol::ChiefTimelineAttachmentSource::Unknown
			&& matches!(
				attachment.kind.as_str(),
				"image" | "localImage" | "imageView" | "imageGeneration" | "inputImage"
			);
		let mut row = div().flex().flex_col().gap_1().min_w_0();
		if can_preview {
			row = row.debug_selector(|| "native-media-action".into()).child(self.workspace_action(
				format!("native-media-{}-{}-{}", turn, item, attachment.index),
				format!("Preview {caption}"),
				move |surface, cx| surface.load_native_media(request.clone(), cx),
				cx,
			));
		} else {
			row = row.child(muted(caption));
		}
		if selected {
			if let Some(image) = &preview.image {
				row = row.child(
					gpui::img(image.clone())
						.debug_selector(|| "native-media-preview".into())
						.max_w_full()
						.max_h(gpui::px(420.0))
						.object_fit(gpui::ObjectFit::Contain),
				);
			}
			if let Some(notice) = preview.notice {
				row = row.child(muted(notice));
			}
		}
		row.into_any_element()
	}

	fn load_native_media(&mut self, request: ChiefMediaRequest, cx: &mut Context<Self>) {
		if self.native_history.preview.task.is_some() {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Some(binding) = self.native_history.binding.clone().filter(|binding| {
			binding.work == request.work_id.as_str() && binding.thread == request.thread_id.as_str()
		}) else {
			return;
		};
		if self.selected.as_deref() != Some(request.work_id.as_str()) {
			return;
		}
		let serial = self.native_history.preview.serial.wrapping_add(1);
		self.native_history.preview = Preview {
			request: Some(request.clone()),
			notice: Some("Loading image…"),
			serial,
			..Default::default()
		};
		let epoch = self.native_history.epoch;
		let account = binding.account.clone();
		let read = cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Image could not be loaded.")?;
			runtime.block_on(async {
				tokio::time::timeout(
					std::time::Duration::from_secs(60),
					load(&ChiefClient::new(profile), request, &account),
				)
				.await
				.map_err(|_| "Image read timed out.")?
			})
		});
		self.native_history.preview.task = Some(cx.spawn(async move |surface, cx| {
			let result = read.await;
			let _ = surface.update(cx, |surface, cx| {
				surface.finish_native_media(epoch, serial, &binding, result, cx);
			});
		}));
		cx.notify();
	}

	fn finish_native_media(
		&mut self,
		epoch: u64,
		serial: u64,
		binding: &Binding,
		result: Result<Arc<gpui::Image>, &'static str>,
		cx: &mut Context<Self>,
	) {
		if self.native_history.epoch != epoch
			|| self.native_history.preview.serial != serial
			|| self.native_history.binding.as_ref() != Some(binding)
			|| self.selected.as_deref() != Some(&binding.work)
		{
			return;
		}
		let preview = &mut self.native_history.preview;
		preview.task = None;
		match result {
			Ok(image) => {
				preview.image = Some(image);
				preview.notice = None;
			},
			Err(notice) => preview.notice = Some(notice),
		}
		cx.notify();
	}
}

fn media_request(
	work: &ChiefWorkItemDto,
	turn: &str,
	item: &str,
	index: u32,
) -> Option<ChiefMediaRequest> {
	Some(ChiefMediaRequest {
		work_id: EntityId::new(work.id.clone()).ok()?,
		thread_id: EntityId::new(work.codex_thread_id.clone()?).ok()?,
		turn_id: EntityId::new(turn).ok()?,
		item_id: EntityId::new(item).ok()?,
		index,
		offset: 0,
		fingerprint: None,
	})
}

async fn load(
	client: &ChiefClient,
	mut request: ChiefMediaRequest,
	account: &str,
) -> Result<Arc<gpui::Image>, &'static str> {
	let mut all = Vec::new();
	let mut expected = None;
	loop {
		let result =
			client.media(request.clone()).await.map_err(|_| "Image could not be loaded.")?;
		let ChiefMediaResult::Available {
			account_id,
			fingerprint,
			mime_type,
			total_bytes,
			bytes,
			..
		} = result
		else {
			return Err(match result {
				ChiefMediaResult::Unsupported => "Preview is not available for this image source.",
				ChiefMediaResult::CapacityExceeded => "Image exceeds the preview size limit.",
				_ => "Image changed or is no longer available. Select Preview to retry.",
			});
		};
		if account_id.as_str() != account
			|| expected.as_ref().is_some_and(|old| old != &(mime_type.clone(), total_bytes))
		{
			return Err("Image source changed. Refresh the conversation.");
		}
		expected = Some((mime_type.clone(), total_bytes));
		all.extend(bytes);
		if all.len() == total_bytes as usize {
			let format = match mime_type.as_str() {
				"image/png" => gpui::ImageFormat::Png,
				"image/jpeg" => gpui::ImageFormat::Jpeg,
				"image/webp" => gpui::ImageFormat::Webp,
				"image/gif" => gpui::ImageFormat::Gif,
				_ => return Err("This content is not a supported image."),
			};
			return Ok(Arc::new(gpui::Image::from_bytes(format, all)));
		}
		request.offset = all.len() as u32;
		request.fingerprint = Some(fingerprint);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::{
		ChiefTimelineAttachment, ChiefTimelineAttachmentSource, ChiefTimelinePage,
	};
	use gpui::{px, size};

	#[gpui::test]
	fn runtime_source_change_clears_history_and_rejects_same_account_late_preview(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, cx| {
			surface.visual_workspace_fixture(cx);
			let mut snapshot = surface.snapshot.clone().unwrap();
			snapshot.runtime_source = Some(EntityId::new("first-process").unwrap());
			surface.apply_result(Ok(ChiefSnapshotResult::Available(snapshot.clone())));
			let binding = Binding {
				work: surface.selected.clone().unwrap(),
				thread: "same-thread".into(),
				account: "same-account".into(),
			};
			let page = ChiefTimelinePage {
				thread_id: binding.thread.clone(),
				entries: vec![],
				next_cursor: None,
				active_realtime_session_at_page_start: None,
			};
			assert!(surface.native_history.replace(binding.clone(), page.clone()));
			let image = Arc::new(gpui::Image::from_bytes(
				gpui::ImageFormat::Png,
				include_bytes!("../../../assets/workspace-symbols/plus.png").to_vec(),
			));
			surface.native_history.preview.image = Some(image.clone());
			let epoch = surface.native_history.epoch;
			let serial = surface.native_history.preview.serial;
			surface.apply_result(Ok(ChiefSnapshotResult::Available(snapshot.clone())));
			assert_eq!(surface.native_history.epoch, epoch);
			assert!(surface.native_history.preview.image.is_some());
			snapshot.runtime_source = Some(EntityId::new("second-process").unwrap());
			surface.apply_result(Ok(ChiefSnapshotResult::Available(snapshot.clone())));
			assert!(surface.native_history.binding.is_none());
			assert!(surface.native_history.preview.image.is_none());
			assert_ne!(surface.native_history.epoch, epoch);
			// Reusing the same task, thread and account must not restore an old callback.
			assert!(surface.native_history.replace(binding.clone(), page));
			surface.finish_native_media(epoch, serial, &binding, Ok(image), cx);
			assert!(surface.native_history.preview.image.is_none());
			snapshot.runtime_source = None;
			surface.apply_result(Ok(ChiefSnapshotResult::Available(snapshot)));
			assert!(surface.native_history.binding.is_none());
		});
	}

	#[gpui::test]
	fn loaded_image_preview_is_removed_when_native_account_changes(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.update(|window, _| window.resize(size(px(1000.), px(700.))));
		let (binding, page, serial) = surface.update(visual, |surface, cx| {
			surface.visual_workspace_fixture(cx);
			surface.graph_visible = false;
			let work = surface
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == surface.selected.as_ref())
				.unwrap();
			work.codex_thread_id = Some("native-thread".into());
			let request = media_request(work, "turn", "image", 0).unwrap();
			let binding = Binding {
				work: work.id.clone(),
				thread: "native-thread".into(),
				account: "first-account".into(),
			};
			let page = ChiefTimelinePage {
				thread_id: "native-thread".into(),
				entries: vec![ChiefTimelineEntry {
					position: 1,
					content: Content::Item {
						app_ui: false,
						turn_id: "turn".into(),
						item_id: "image".into(),
						kind: "userMessage".into(),
						text: "Image question".into(),
						truncated: false,
						activity: None,
						attachments: vec![ChiefTimelineAttachment {
							index: 0,
							kind: "localImage".into(),
							label: "photo.png".into(),
							source: ChiefTimelineAttachmentSource::Local,
						}],
					},
				}],
				next_cursor: None,
				active_realtime_session_at_page_start: None,
			};
			assert!(surface.native_history.replace(binding.clone(), page.clone()));
			surface.native_history.preview = Preview {
				request: Some(request),
				image: Some(Arc::new(gpui::Image::from_bytes(
					gpui::ImageFormat::Png,
					include_bytes!("../../../assets/workspace-symbols/plus.png").to_vec(),
				))),
				serial: 8,
				..Default::default()
			};
			cx.notify();
			(binding, page, 8)
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("native-media-preview").is_some());
		surface.update(visual, |surface, cx| {
			let mut other = binding.clone();
			other.account = "second-account".into();
			assert!(surface.native_history.replace(other, page.clone()));
			assert!(surface.native_history.preview.image.is_none());
			assert_ne!(surface.native_history.preview.serial, serial);
			// Returning to the original account must not revive an earlier callback token.
			assert!(surface.native_history.replace(binding, page));
			assert_ne!(surface.native_history.preview.serial, serial);
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("native-media-preview").is_none());
	}
	#[gpui::test]
	fn delayed_preview_cannot_replace_a_new_view_after_leaving_and_returning(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		let (send, receive) = tokio::sync::oneshot::channel();
		let (task, original, other, binding, page) = surface.update(visual, |surface, cx| {
			surface.visual_workspace_fixture(cx);
			let original = surface.selected.clone().unwrap();
			let snapshot = surface.snapshot.as_mut().unwrap();
			let other =
				snapshot.work_items.iter().find(|work| work.id != original).unwrap().id.clone();
			let work = snapshot.work_items.iter_mut().find(|work| work.id == original).unwrap();
			work.codex_thread_id = Some("native-thread".into());
			let binding = Binding {
				work: original.clone(),
				thread: "native-thread".into(),
				account: "account".into(),
			};
			let page = ChiefTimelinePage {
				thread_id: "native-thread".into(),
				entries: vec![],
				next_cursor: None,
				active_realtime_session_at_page_start: None,
			};
			assert!(surface.native_history.replace(binding.clone(), page.clone()));
			let epoch = surface.native_history.epoch;
			let serial = surface.native_history.preview.serial;
			let captured = binding.clone();
			let task = cx.spawn(async move |surface, cx| {
				let result = receive.await.unwrap();
				surface
					.update(cx, |surface, cx| {
						surface.finish_native_media(epoch, serial, &captured, result, cx)
					})
					.unwrap();
			});
			(task, original, other, binding, page)
		});
		visual.run_until_parked();
		surface.update(visual, |surface, cx| {
			surface.open_page(&other, cx);
			surface.open_page(&original, cx);
			assert!(surface.native_history.replace(binding, page));
			surface.native_history.preview.notice = Some("New preview pending");
		});
		send.send(Ok(Arc::new(gpui::Image::from_bytes(
			gpui::ImageFormat::Png,
			include_bytes!("../../../assets/workspace-symbols/plus.png").to_vec(),
		))))
		.unwrap();
		visual.run_until_parked();
		surface.read_with(visual, |surface, _| {
			assert!(surface.native_history.preview.image.is_none());
			assert_eq!(surface.native_history.preview.notice, Some("New preview pending"));
		});
		drop(task);
	}
}

#[cfg(test)]
#[path = "chief_timeline_media_wire_tests.rs"]
mod wire_tests;
