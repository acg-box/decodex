//! Explicit, source-bound desktop App UI loading.
use super::*;
use decodex_protocol::{ChiefAppUiRequest, ChiefAppUiResult};
#[path = "chief_app_ui_native.rs"] mod native;

#[derive(Default)]
pub(super) struct State {
	request: Option<ChiefAppUiRequest>,
	host: Option<native::AppHost>,
	task: Option<Task<()>>,
	notice: Option<&'static str>,
	serial: u64,
}
impl State {
	pub(super) fn clear(&mut self) {
		*self = Self { serial: self.serial.wrapping_add(1), ..Default::default() };
	}
}

impl ChiefSurface {
	pub(in super::super) fn poll_native_app_ui(&mut self) {
		let state = &mut self.native_history.app_ui;
		while let Some(event) = state.host.as_mut().and_then(native::AppHost::poll) {
			if matches!(event["type"].as_str(), Some("closed" | "unavailable")) {
				state.clear();
				break;
			}
		}
	}

	pub(super) fn native_app_ui_action(
		&self,
		work: &ChiefWorkItemDto,
		turn: &str,
		item: &str,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let Some(request) = request(work, turn, item) else {
			return muted("App unavailable").into_any_element();
		};
		let selected = self.native_history.app_ui.request.as_ref() == Some(&request);
		let notice = selected.then_some(self.native_history.app_ui.notice).flatten();
		let mut row = div().w_full().flex().flex_col().gap_1();
		row = row.child(
			div()
				.id(SharedString::from(format!("open-app-{turn}-{item}")))
				.debug_selector(|| "native-app-ui-open".into())
				.cursor_pointer()
				.p_2()
				.child("Open app")
				.on_click(cx.listener(move |surface, _, window, cx| {
					surface.load_native_app_ui(request.clone(), window, cx)
				})),
		);
		if let Some(notice) = notice {
			row = row.child(muted(notice));
		}
		row.into_any_element()
	}

	fn load_native_app_ui(
		&mut self,
		request: ChiefAppUiRequest,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let Some(profile) = self.profile.clone() else { return };
		let Some(binding) = self.native_history.binding.clone().filter(|binding| {
			binding.work == request.work_id.as_str() && binding.thread == request.thread_id.as_str()
		}) else {
			return;
		};
		if self.selected.as_deref() != Some(request.work_id.as_str()) {
			return;
		}
		self.native_history.app_ui.clear();
		let state = &mut self.native_history.app_ui;
		state.request = Some(request.clone());
		let Ok(host) = native::AppHost::new(window) else {
			state.notice = Some("App view is unavailable in this build.");
			cx.notify();
			return;
		};
		state.host = Some(host);
		state.notice = Some("Loading app…");
		let serial = state.serial;
		let epoch = self.native_history.epoch;
		let account = binding.account.clone();
		let read = cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "App could not be loaded.")?;
			runtime.block_on(async {
				tokio::time::timeout(
					std::time::Duration::from_secs(60),
					load(&ChiefClient::new(profile), request, &account),
				)
				.await
				.map_err(|_| "App read timed out.")?
			})
		});
		self.native_history.app_ui.task = Some(cx.spawn(async move |surface, cx| {
			let result = read.await;
			let _ = surface.update(cx, |surface, cx| {
				if surface.native_history.epoch != epoch
					|| surface.native_history.app_ui.serial != serial
					|| surface.native_history.binding.as_ref() != Some(&binding)
					|| surface.selected.as_deref() != Some(&binding.work)
				{
					return;
				}
				let state = &mut surface.native_history.app_ui;
				state.task = None;
				state.notice = Some(match result {
					Ok(document)
						if state.host.as_mut().is_some_and(|host| {
							host.command(
								serde_json::json!({"operation":"load","document":document}),
							)
						}) =>
						"App opened in a separate window.",
					Ok(_) => "This app document could not be displayed.",
					Err(message) => message,
				});
				cx.notify();
			});
		}));
		cx.notify();
	}
}

fn request(work: &ChiefWorkItemDto, turn: &str, item: &str) -> Option<ChiefAppUiRequest> {
	Some(ChiefAppUiRequest {
		work_id: EntityId::new(work.id.clone()).ok()?,
		thread_id: EntityId::new(work.codex_thread_id.clone()?).ok()?,
		turn_id: EntityId::new(turn).ok()?,
		item_id: EntityId::new(item).ok()?,
		offset: 0,
		fingerprint: None,
	})
}

async fn load(
	client: &ChiefClient,
	mut request: ChiefAppUiRequest,
	account: &str,
) -> Result<serde_json::Value, &'static str> {
	let mut document = Vec::new();
	let mut total = None;
	loop {
		let result =
			client.app_ui(request.clone()).await.map_err(|_| "App could not be loaded.")?;
		let ChiefAppUiResult::Available { account_id, fingerprint, total_bytes, bytes, .. } =
			result
		else {
			return Err(match result {
				ChiefAppUiResult::Unsupported =>
					"No supported app is available for this tool result.",
				ChiefAppUiResult::CapacityExceeded => "App document exceeds the size limit.",
				_ => "App source changed or is unavailable.",
			});
		};
		if account_id.as_str() != account || total.is_some_and(|expected| expected != total_bytes) {
			return Err("App source changed.");
		}
		total = Some(total_bytes);
		document.extend(bytes);
		if document.len() == total_bytes as usize {
			return serde_json::from_slice(&document).map_err(|_| "App document is invalid.");
		}
		request.offset = document.len() as u32;
		request.fingerprint = Some(fingerprint);
	}
}

#[cfg(test)]
#[path = "chief_app_ui_wire_tests.rs"]
mod wire_tests;
