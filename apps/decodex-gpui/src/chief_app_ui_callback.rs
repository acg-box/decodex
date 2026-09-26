//! Native confirmation for untrusted widget requests. The service owns execution.
use super::*;
use decodex_protocol::{
	ChiefAppUiCall, ChiefAppUiCallReview, ChiefAppUiReceiptRequest, ChiefAppUiReceiptResult,
};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct State {
	work: Option<EntityId>,
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
	pub(in super::super) fn render_native_app_recovery(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let owner = work.id.clone();
		let mut row = div().flex().flex_col().gap_2().child(
			div()
				.id("app-call-discover")
				.debug_selector(|| "app-call-discover".into())
				.cursor_pointer()
				.p_2()
				.child("Check unresolved app calls")
				.on_click(cx.listener(move |surface, _, _, cx| {
					surface.discover_native_app_call(&owner, cx)
				})),
		);
		let state = &self.native_history.app_ui;
		if state.callback.work.as_ref().is_some_and(|id| id.as_str() == work.id) {
			if let Some(notice) = state.notice {
				row = row.child(notice);
			}
			row = row.child(self.render_app_call(cx));
		}
		row.into_any_element()
	}

	fn discover_native_app_call(&mut self, work: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work) {
			return;
		}
		let (Some(profile), Ok(work_id)) = (self.profile.clone(), EntityId::new(work)) else {
			return;
		};
		let state = &mut self.native_history.app_ui;
		if state.callback.task.is_some() || state.callback.review.is_some() {
			return;
		}
		let serial = state.serial;
		state.callback.work = Some(work_id.clone());
		state.notice = Some("Reading saved app calls…");
		let owner = work_id.clone();
		let read = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(async {
				let client = ChiefClient::new(profile);
				match client.pending_app_ui_call(work_id.clone()).await.ok()? {
					decodex_protocol::ChiefPendingAppUiCall::Available {
						operation_id: Some(operation_id),
						..
					} => {
						let request = ChiefAppUiReceiptRequest {
							work_id,
							operation_id,
							offset: 0,
							fingerprint: None,
						};
						let receipt = read_receipt(&client, request.clone()).await;
						Some(Some((request, receipt)))
					},
					decodex_protocol::ChiefPendingAppUiCall::Available {
						operation_id: None,
						..
					} => Some(None),
					decodex_protocol::ChiefPendingAppUiCall::Unavailable => None,
				}
			})
		});
		state.callback.task = Some(cx.spawn(async move |surface, cx| {
			let result = read.await;
			let _ = surface.update(cx, |surface, cx| {
				if surface.selected.as_deref() != Some(owner.as_str()) {
					return;
				}
				let state = &mut surface.native_history.app_ui;
				if state.serial != serial {
					return;
				}
				state.callback.task = None;
				match result {
					Some(Some((request, receipt))) => {
						let operation = request.operation_id.clone();
						state.callback.receipt_request = Some(request);
						apply_receipt(state, &operation, receipt);
					},
					Some(None) => state.notice = Some("No unresolved app calls were found."),
					None =>
						state.notice = Some(
							"Saved app calls are unavailable. This does not confirm their outcome.",
						),
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

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
		state.callback.work = Some(request.work_id.clone());
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
						.debug_selector(|| "app-call-confirm".into())
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
						.debug_selector(|| "app-call-refresh".into())
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
							.debug_selector(|| "app-call-acknowledge".into())
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
	#[tokio::test]
	async fn app_ui_pending_discovery_distinguishes_missing_data_and_foreign_owner() {
		for mode in ["pending-none", "pending-unavailable", "pending-foreign"] {
			let (_root, profile, server) = super::super::wire_tests::fixture(mode);
			let result =
				ChiefClient::new(profile).pending_app_ui_call(EntityId::new("work").unwrap()).await;
			assert!(server.join().unwrap().is_empty());
			match mode {
				"pending-none" => assert!(matches!(
					result,
					Ok(decodex_protocol::ChiefPendingAppUiCall::Available {
						operation_id: None,
						..
					})
				)),
				"pending-unavailable" => assert_eq!(
					result.unwrap(),
					decodex_protocol::ChiefPendingAppUiCall::Unavailable
				),
				_ => assert!(result.is_err()),
			}
		}
	}

	#[gpui::test]
	fn app_ui_cold_discovery_loads_unknown_receipt_without_a_widget(cx: &mut gpui::TestAppContext) {
		let (_root, profile, server) = super::super::wire_tests::fixture("pending-cold");
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, cx| {
			surface.selected = Some("work".into());
			surface.profile = Some(profile);
			assert!(surface.native_history.app_ui.host.is_none());
			surface.discover_native_app_call("work", cx);
			// The recovery task captured its real local client. Do not start the unrelated
			// OS-thread output subscription in GPUI's deterministic test scheduler.
			surface.profile = None;
		});
		visual.run_until_parked();
		assert!(server.join().unwrap().is_empty());
		surface.read_with(visual, |surface, _| {
			let state = &surface.native_history.app_ui;
			assert!(state.host.is_none());
			assert_eq!(
				state.callback.receipt_request.as_ref().unwrap().operation_id.as_str(),
				"saved-operation"
			);
			assert_eq!(state.callback.receipt.as_ref().unwrap()["state"], "unknown");
			assert_eq!(state.callback.receipt.as_ref().unwrap()["uncertaintyAcknowledged"], false);
		});
	}
	#[gpui::test]
	fn app_ui_recovery_controls_render_without_the_native_view(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(gpui::size(px(1400.), px(1400.)));
		surface.update(visual, |surface, cx| {
            surface.visual_workspace_fixture(cx);
            surface.graph_visible = false;
            let owner = EntityId::new(surface.selected.clone().unwrap()).unwrap();
            let state = &mut surface.native_history.app_ui;
            state.callback.work = Some(owner.clone());
            state.callback.receipt_request = Some(ChiefAppUiReceiptRequest {
                work_id:owner.clone(), operation_id:EntityId::new("saved-operation").unwrap(), offset:0, fingerprint:None,
            });
            apply_receipt(state, &EntityId::new("saved-operation").unwrap(), Some(json!({
                "workId":owner,"operationId":"saved-operation","reservationId":42,"server":"fixture","tool":"calculate",
                "arguments":{"value":7},"state":"unknown","uncertaintyAcknowledged":false,
            })));
            assert!(state.host.is_none());
            cx.notify();
        });
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("app-call-discover").is_some());
		assert!(visual.debug_bounds("app-call-refresh").is_some());
		assert!(visual.debug_bounds("app-call-acknowledge").is_some());
		surface.update(visual, |surface, cx| {
			surface.native_history.app_ui.callback.receipt.as_mut().unwrap()["uncertaintyAcknowledged"] =
				json!(true);
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("app-call-refresh").is_some());
		assert!(visual.debug_bounds("app-call-acknowledge").is_none());
	}
	#[gpui::test]
	fn app_ui_confirm_and_acknowledge_read_saved_outcomes_after_lost_replies(
		cx: &mut gpui::TestAppContext,
	) {
		for mode in ["call-lost", "ack-lost"] {
			let (_root, profile, server) = super::super::wire_tests::fixture(mode);
			let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
			surface.update(visual, |surface, cx| {
				surface.selected = Some("work".into());
				surface.profile = Some(profile);
				let state = &mut surface.native_history.app_ui;
				let owner = EntityId::new("work").unwrap();
				state.callback.work = Some(owner.clone());
				state.host = Some(super::super::native::AppHost);
				if mode == "call-lost" {
					state.callback.review = Some(ChiefAppUiCallReview::Available {
						request: Box::new(ChiefAppUiCall {
							work_id: owner,
							thread_id: EntityId::new("native-thread").unwrap(),
							turn_id: EntityId::new("turn").unwrap(),
							item_id: EntityId::new("widget").unwrap(),
							source_fingerprint: EntityId::new("c".repeat(64)).unwrap(),
							operation_id: EntityId::new("saved-operation").unwrap(),
							tool: decodex_protocol::WireText::new("calculate").unwrap(),
							arguments: json!({"value":7}),
						}),
						review_token: EntityId::new("a".repeat(64)).unwrap(),
						server: decodex_protocol::WireText::new("fixture").unwrap(),
						title: decodex_protocol::WireText::new("Calculate").unwrap(),
						pending_operation: None,
					});
					surface.confirm_native_app_call(cx);
					surface.confirm_native_app_call(cx);
				} else {
					state.callback.receipt_request = Some(ChiefAppUiReceiptRequest {
						work_id: owner,
						operation_id: EntityId::new("saved-operation").unwrap(),
						offset: 0,
						fingerprint: None,
					});
					state.callback.receipt = Some(
						json!({"state":"unknown","uncertaintyAcknowledged":false,"reservationId":42}),
					);
					surface.refresh_native_app_receipt(true, cx);
					surface.refresh_native_app_receipt(true, cx);
				}
				// Closing the widget does not cancel the independent saved-outcome read.
				surface.native_history.app_ui.host = None;
				surface.native_history.app_ui.callback.close_view();
				surface.profile = None;
			});
			visual.run_until_parked();
			assert!(server.join().unwrap().is_empty());
			surface.read_with(visual, |surface, _| {
				let callback = &surface.native_history.app_ui.callback;
				assert!(callback.review.is_none());
				assert!(callback.task.is_none());
				let receipt = callback.receipt.as_ref().unwrap();
				if mode == "call-lost" {
					assert_eq!(receipt["state"], "completed");
					assert_eq!(receipt["result"]["structuredContent"]["value"], 42);
				} else {
					assert_eq!(receipt["state"], "unknown");
					assert_eq!(receipt["uncertaintyAcknowledged"], true);
				}
			});
		}
	}
	#[gpui::test]
	fn app_ui_browser_request_only_prepares_confirmation(cx: &mut gpui::TestAppContext) {
		let (_root, profile, server) = super::super::wire_tests::fixture("review-valid");
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, cx| {
			surface.selected = Some("work".into());
			surface.profile = Some(profile);
			let state = &mut surface.native_history.app_ui;
			state.host = Some(super::super::native::AppHost);
			state.request = Some(ChiefAppUiRequest {
				work_id: EntityId::new("work").unwrap(),
				thread_id: EntityId::new("native-thread").unwrap(),
				turn_id: EntityId::new("turn").unwrap(),
				item_id: EntityId::new("widget").unwrap(),
				offset: 0,
				fingerprint: None,
			});
			state.source = Some(EntityId::new("c".repeat(64)).unwrap());
			surface.review_native_app_call(
				json!({"operationId":"saved-operation","tool":"calculate","arguments":{"value":7}}),
				cx,
			);
			surface.profile = None;
		});
		visual.run_until_parked();
		assert!(server.join().unwrap().is_empty());
		surface.read_with(visual, |surface, _| {
            let state = &surface.native_history.app_ui;
            assert!(state.callback.task.is_none());
            assert!(matches!(&state.callback.review, Some(ChiefAppUiCallReview::Available { request, .. }) if request.arguments == json!({"value":7})));
            assert!(state.callback.receipt_request.is_none(), "Review alone must not submit a call");
        });
	}
}
