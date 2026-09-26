//! Native confirmation for untrusted widget requests. The service owns execution.
use super::*;
use decodex_protocol::{
	ChiefAppUiCall, ChiefAppUiCallReview, ChiefAppUiReceiptRequest, ChiefAppUiReceiptResult,
};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct State {
	review: Option<ChiefAppUiCallReview>,
	task: Option<Task<()>>,
	receipt_request: Option<ChiefAppUiReceiptRequest>,
	receipt: Option<Value>,
}

impl State {
	pub(super) fn close_view(&mut self) {
		self.review = None;
		// Submitted work has an independent durable outcome. Keep its readback alive.
		if self.receipt_request.is_none() {
			self.task = None;
		}
	}
}

impl ChiefSurface {
	pub(super) fn review_native_app_call(&mut self, event: Value, cx: &mut Context<Self>) {
		let state = &mut self.native_history.app_ui;
		if state.callback.task.is_some() || state.callback.review.is_some() {
			return;
		}
		let (Some(request), Some(source), Some(profile)) =
			(state.request.clone(), state.source.clone(), self.profile.clone())
		else {
			return;
		};
		let Some(call) = callback_request(&request, source, &event) else {
			return;
		};
		let serial = state.serial;
		let operation = call.operation_id.clone();
		state.notice = Some("Reviewing app request…");
		let read = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).review_app_ui_call(call)).ok()
		});
		state.callback.task = Some(cx.spawn(async move |surface, cx| {
            let review = read.await;
            let _ = surface.update(cx, |surface, cx| {
                let state = &mut surface.native_history.app_ui;
                if state.serial != serial || state.host.is_none() { return; }
                state.callback.task = None;
                match review {
                    Some(review @ ChiefAppUiCallReview::Available { .. }) => {
                        if let ChiefAppUiCallReview::Available { request, pending_operation: Some(operation), .. } = &review {
                            state.callback.receipt_request = Some(ChiefAppUiReceiptRequest { work_id:request.work_id.clone(), operation_id:operation.clone(), offset:0, fingerprint:None });
                            state.callback.receipt = None;
                        }
                        state.callback.review = Some(review);
                        state.notice = Some("Review the app request below before allowing it.");
                    },
                    _ => {
                        if let Some(host) = state.host.as_mut() { host.command(json!({"operation":"tool_result","operationId":operation,"error":"App request could not be reviewed"})); }
                        state.notice = Some("App request is unavailable or exceeds the supported size.");
                    }
                }
                cx.notify();
            });
        }));
		cx.notify();
	}

	pub(super) fn render_app_call(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let mut row = div().flex().flex_col().gap_2();
		if let Some(ChiefAppUiCallReview::Available {
			request,
			server,
			title,
			pending_operation,
			..
		}) = &self.native_history.app_ui.callback.review
		{
			row = row
				.child(format!(
					"{} · {} · {}",
					title.as_str(),
					server.as_str(),
					request.tool.as_str()
				))
				.child(serde_json::to_string_pretty(&request.arguments).unwrap_or_default());
			if pending_operation.is_some() {
				row =
					row.child("An earlier app call is unresolved. Review its saved outcome first.");
			} else {
				row = row.child(
					div()
						.id("app-call-confirm")
						.cursor_pointer()
						.p_2()
						.child("Allow this call")
						.on_click(
							cx.listener(|surface, _, _, cx| surface.confirm_native_app_call(cx)),
						),
				);
			}
			row = row.child(div().id("app-call-cancel").cursor_pointer().p_2().child("Cancel")
                .on_click(cx.listener(|surface, _, _, cx| {
                    let state = &mut surface.native_history.app_ui;
                    if let Some(ChiefAppUiCallReview::Available { request, .. }) = state.callback.review.take()
                        && let Some(host) = state.host.as_mut() {
                        host.command(json!({"operation":"tool_result","operationId":request.operation_id,"error":"User declined this app call"}));
                    }
                    state.notice = Some("App call cancelled.");
                    cx.notify();
                })));
		}
		let callback = &self.native_history.app_ui.callback;
		if callback.receipt_request.is_some() {
			if let Some(receipt) = &callback.receipt {
				row = row
					.child(format!(
						"Saved call: {} · {}",
						receipt["server"].as_str().unwrap_or(""),
						receipt["tool"].as_str().unwrap_or("")
					))
					.child(serde_json::to_string_pretty(&receipt["arguments"]).unwrap_or_default())
					.child(receipt_notice(Some(receipt)));
			}
			if callback.task.is_none() {
				row = row.child(
					div()
						.id("app-call-refresh")
						.cursor_pointer()
						.p_2()
						.child("Read saved outcome")
						.on_click(cx.listener(|surface, _, _, cx| {
							surface.refresh_native_app_receipt(false, cx)
						})),
				);
				if callback.receipt.as_ref().is_some_and(|r| {
					r["state"] == "unknown" && r["uncertaintyAcknowledged"] == false
				}) {
					row = row.child(
						div()
							.id("app-call-acknowledge")
							.cursor_pointer()
							.p_2()
							.child("I understand this call may have run")
							.on_click(cx.listener(|surface, _, _, cx| {
								surface.refresh_native_app_receipt(true, cx)
							})),
					);
				}
			}
		}
		row.into_any_element()
	}

	fn refresh_native_app_receipt(&mut self, acknowledge: bool, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let state = &mut self.native_history.app_ui;
		if state.callback.task.is_some() {
			return;
		}
		let Some(request) = state.callback.receipt_request.clone() else {
			return;
		};
		let action = if acknowledge {
			let Some(receipt) = state
				.callback
				.receipt
				.as_ref()
				.filter(|r| r["state"] == "unknown" && r["uncertaintyAcknowledged"] == false)
			else {
				return;
			};
			let Some(reservation_id) = receipt["reservationId"].as_i64() else {
				return;
			};
			Some(decodex_protocol::ChiefActionDto::AcknowledgeAppUiCall {
				work_id: request.work_id.clone(),
				operation_id: request.operation_id.clone(),
				reservation_id,
			})
		} else {
			None
		};
		let serial = state.serial;
		let operation = request.operation_id.clone();
		let read = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(async {
				let client = ChiefClient::new(profile);
				if let Some(action) = action {
					let _ = client
						.execute(
							action,
							IdempotencyKey::new(unique_command()).expect("command identity"),
						)
						.await;
				}
				read_receipt(&client, request).await
			})
		});
		state.callback.task = Some(cx.spawn(async move |surface, cx| {
            let receipt = read.await;
            let _ = surface.update(cx, |surface, cx| {
                let state = &mut surface.native_history.app_ui;
                if state.serial != serial { return; }
                state.callback.task = None;
                apply_receipt(state, &operation, receipt);
                // A prior review never becomes permission after acknowledgment. Require a
                // fresh browser request and fresh service review for any subsequent call.
                if acknowledge
                    && let Some(ChiefAppUiCallReview::Available { request, .. }) = state.callback.review.take()
                    && let Some(host) = state.host.as_mut() {
                    host.command(json!({"operation":"tool_result","operationId":request.operation_id,"error":"Request a fresh review after checking the saved outcome"}));
                }
                cx.notify();
            });
        }));
		cx.notify();
	}

	fn confirm_native_app_call(&mut self, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let state = &mut self.native_history.app_ui;
		if state.host.is_none() || state.callback.task.is_some() {
			return;
		}
		let Some(ChiefAppUiCallReview::Available { pending_operation: None, .. }) =
			state.callback.review.as_ref()
		else {
			return;
		};
		let Some(ChiefAppUiCallReview::Available { request, review_token, .. }) =
			state.callback.review.take()
		else {
			return;
		};
		let serial = state.serial;
		let operation = request.operation_id.clone();
		let receipt = ChiefAppUiReceiptRequest {
			work_id: request.work_id.clone(),
			operation_id: operation.clone(),
			offset: 0,
			fingerprint: None,
		};
		state.callback.receipt_request = Some(receipt.clone());
		state.callback.receipt = None;
		state.notice = Some("App call submitted. Waiting for its saved outcome…");
		let run = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(async {
				let client = ChiefClient::new(profile);
				// Even a lost local reply goes to durable readback, never another execution.
				let _ = client
					.execute(
						decodex_protocol::ChiefActionDto::ConfirmAppUiTool {
							request: *request,
							review_token,
						},
						IdempotencyKey::new(unique_command()).expect("command identity"),
					)
					.await;
				read_receipt(&client, receipt).await
			})
		});
		state.callback.task = Some(cx.spawn(async move |surface, cx| {
			let receipt = run.await;
			let _ = surface.update(cx, |surface, cx| {
				let state = &mut surface.native_history.app_ui;
				if state.serial != serial {
					return;
				}
				state.callback.task = None;
				apply_receipt(state, &operation, receipt);
				cx.notify();
			});
		}));
		cx.notify();
	}
}

fn receipt_notice(receipt: Option<&Value>) -> &'static str {
	match receipt.and_then(|r| r["state"].as_str()) {
		Some("completed") => "App response saved.",
		Some("unsent") => "This call was not sent.",
		Some("unknown") if receipt.is_some_and(|r| r["uncertaintyAcknowledged"] == true) =>
			"Uncertain outcome acknowledged. This call will not be repeated.",
		Some("unknown") => "This call may have run. Check the app before making another request.",
		Some("reserved") => "This call is still unresolved. Read its saved outcome again later.",
		_ => "App outcome is unavailable. Do not repeat the call.",
	}
}

fn apply_receipt(state: &mut super::State, operation: &EntityId, receipt: Option<Value>) {
	state.notice = Some(receipt_notice(receipt.as_ref()));
	if let Some(host) = state.host.as_mut() {
		match receipt.as_ref().and_then(|r| r["state"].as_str()) {
			Some("completed") => {
				if let Some(result) =
					receipt.as_ref().and_then(|r| r.get("result")).filter(|r| r.is_object())
				{
					host.command(
						json!({"operation":"tool_result","operationId":operation,"result":result}),
					);
				}
			},
			Some("unsent" | "unknown") => {
				host.command(json!({"operation":"tool_result","operationId":operation,"error":receipt_notice(receipt.as_ref())}));
			},
			_ => {},
		}
	}
	state.callback.receipt = receipt;
}

fn callback_request(
	request: &ChiefAppUiRequest,
	source: EntityId,
	event: &Value,
) -> Option<ChiefAppUiCall> {
	let arguments = event.get("arguments")?.clone();
	if !arguments.is_object() {
		return None;
	}
	Some(ChiefAppUiCall {
		work_id: request.work_id.clone(),
		thread_id: request.thread_id.clone(),
		turn_id: request.turn_id.clone(),
		item_id: request.item_id.clone(),
		source_fingerprint: source,
		operation_id: EntityId::new(event["operationId"].as_str()?).ok()?,
		tool: decodex_protocol::WireText::new(event["tool"].as_str()?).ok()?,
		arguments,
	})
}

async fn read_receipt(
	client: &ChiefClient,
	mut request: ChiefAppUiReceiptRequest,
) -> Option<Value> {
	let mut document = Vec::new();
	let mut total = None;
	loop {
		let ChiefAppUiReceiptResult::Available { fingerprint, total_bytes, bytes, .. } =
			client.app_ui_receipt(request.clone()).await.ok()?
		else {
			return None;
		};
		if total.is_some_and(|n| n != total_bytes) {
			return None;
		}
		total = Some(total_bytes);
		document.extend(bytes);
		if document.len() == total_bytes as usize {
			let receipt: Value = serde_json::from_slice(&document).ok()?;
			if receipt["workId"].as_str() != Some(request.work_id.as_str())
				|| receipt["operationId"].as_str() != Some(request.operation_id.as_str())
			{
				return None;
			}
			return Some(receipt);
		}
		request.offset = document.len() as u32;
		request.fingerprint = Some(fingerprint);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn app_ui_callback_cannot_replace_the_displayed_native_source() {
		let request = ChiefAppUiRequest {
			work_id: EntityId::new("owned-work").unwrap(),
			thread_id: EntityId::new("owned-thread").unwrap(),
			turn_id: EntityId::new("owned-turn").unwrap(),
			item_id: EntityId::new("owned-item").unwrap(),
			offset: 0,
			fingerprint: None,
		};
		let source = EntityId::new("a".repeat(64)).unwrap();
		let event = json!({"operationId":"host-operation","tool":"calculate","arguments":{"value":7},
            "work_id":"foreign-work","thread_id":"foreign-thread","source_fingerprint":"foreign-source","review_token":"forged"});
		let call = callback_request(&request, source.clone(), &event).unwrap();
		assert_eq!(call.work_id, request.work_id);
		assert_eq!(call.thread_id, request.thread_id);
		assert_eq!(call.turn_id, request.turn_id);
		assert_eq!(call.item_id, request.item_id);
		assert_eq!(call.source_fingerprint, source);
		assert_eq!(call.arguments, json!({"value":7}));
		let mut malformed = event;
		malformed["arguments"] = json!(["not an argument object"]);
		assert!(callback_request(&request, call.source_fingerprint, &malformed).is_none());
	}
	#[tokio::test]
	async fn app_ui_saved_outcome_read_is_query_only_and_rejects_foreign_owner() {
		for mode in ["receipt-valid", "receipt-foreign"] {
			let (_root, profile, server) = super::super::wire_tests::fixture(mode);
			let receipt = read_receipt(
				&ChiefClient::new(profile),
				ChiefAppUiReceiptRequest {
					work_id: EntityId::new("work").unwrap(),
					operation_id: EntityId::new("saved-operation").unwrap(),
					offset: 0,
					fingerprint: None,
				},
			)
			.await;
			assert!(
				server.join().unwrap().is_empty(),
				"Receipt read must not request native resources"
			);
			if mode == "receipt-valid" {
				let mut state = super::super::State::default();
				state.callback.receipt_request = Some(ChiefAppUiReceiptRequest {
					work_id: EntityId::new("work").unwrap(),
					operation_id: EntityId::new("saved-operation").unwrap(),
					offset: 0,
					fingerprint: None,
				});
				apply_receipt(&mut state, &EntityId::new("saved-operation").unwrap(), receipt);
				state.callback.close_view();
				assert!(state.callback.receipt_request.is_some());
				assert_eq!(state.callback.receipt.as_ref().unwrap()["state"], "unknown");
				assert_eq!(
					state.callback.receipt.as_ref().unwrap()["uncertaintyAcknowledged"],
					false
				);
				assert_eq!(
					state.notice,
					Some("This call may have run. Check the app before making another request.")
				);
			} else {
				assert!(receipt.is_none());
			}
		}
	}
}
