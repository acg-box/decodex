//! Accounts Reset Card panel. The daemon owns recovery; the view stores no journal.
use super::{Shell, WB_TEXT, WB_TEXT_MUTED, account_row_action};
use decodex_protocol::{
	AccountResetCardOperationResult, ClientProfile, EntityId, EntityRevision, IdempotencyKey,
	ResetCardClient, ResetCardConsumeResponse, ResetCardDescriptorDto, ResetCardInventoryResult,
	ResetCardOperationResult, ResetCardOutcome,
};
use gpui::{AnyElement, Context, div, prelude::*, px, rgb};
use std::{
	sync::{
		atomic::{AtomicU64, Ordering},
		mpsc::{self, Receiver},
	},
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::quota_meter::ResetFill;

static NEXT_CONFIRMATION: AtomicU64 = AtomicU64::new(0);
#[derive(Default)]
pub(super) struct ResetCardsPanel {
	pub(super) profile: Option<ClientProfile>,
	selected: Option<(EntityId, String)>,
	inventory: Option<ResetCardInventoryResult>,
	confirmation: Option<(ResetCardDescriptorDto, EntityRevision)>,
	pending_key: Option<IdempotencyKey>,
	busy: bool,
	blocked: bool,
	message: String,
	updates: Option<Receiver<Update>>,
	pending_fill: Option<ResetFill>,
	fills: std::collections::HashMap<EntityId, ResetFill>,
}
struct Update {
	completed_reset: Option<(EntityId, IdempotencyKey)>,
	inventory: Option<ResetCardInventoryResult>,
	blocked: bool,
	pending_key: Option<IdempotencyKey>,
	message: String,
}
impl Shell {
	pub(super) fn reset_fill_for(
		&self,
		account: &decodex_protocol::AccountDto,
	) -> Option<ResetFill> {
		self.reset_cards
			.fills
			.get(&account.account_id)
			.filter(|fill| fill.revision == account.account_revision)
			.cloned()
	}

	pub(super) fn show_reset_cards(
		&mut self,
		account: EntityId,
		alias: String,
		cx: &mut Context<Self>,
	) {
		if self.reset_cards.busy {
			return;
		}
		if self.reset_cards.selected.as_ref().map(|value| &value.0) != Some(&account) {
			self.reset_cards.pending_key = None;
		}
		self.reset_cards.selected = Some((account, alias));
		self.reset_cards.confirmation = None;
		self.refresh_reset_cards(cx);
	}

	fn refresh_reset_cards(&mut self, cx: &mut Context<Self>) {
		if self.reset_cards.busy {
			return;
		}
		let (Some(profile), Some((account, _))) =
			(self.reset_cards.profile.clone(), self.reset_cards.selected.clone())
		else {
			return;
		};
		let key = self.reset_cards.pending_key.clone();
		self.start_reset_card_work(cx, async move {
			load(&ResetCardClient::new(profile), account, key).await
		});
	}

	fn confirm_reset_card(&mut self, cx: &mut Context<Self>) {
		if self.reset_cards.busy || self.reset_cards.blocked {
			return;
		}
		let (Some(profile), Some((account, _)), Some((descriptor, revision))) = (
			self.reset_cards.profile.clone(),
			self.reset_cards.selected.clone(),
			self.reset_cards.confirmation.take(),
		) else {
			return;
		};
		let Ok(timestamp) = SystemTime::now().duration_since(UNIX_EPOCH) else {
			return;
		};
		let Ok(key) = IdempotencyKey::new(format!(
			"decodex-reset-{}-{}-{}",
			std::process::id(),
			timestamp.as_nanos(),
			NEXT_CONFIRMATION.fetch_add(1, Ordering::Relaxed)
		)) else {
			return;
		};
		self.reset_cards.pending_fill = self
			.accounts
			.accounts
			.iter()
			.find(|row| row.account_id == account && row.account_revision == revision)
			.map(|row| ResetFill {
				revision,
				initial: [row.five_hour_quota, row.seven_day_quota],
				started: std::time::Instant::now(),
				confirmed_at_micros: 0,
			});
		self.reset_cards.pending_key = Some(key.clone());
		self.start_reset_card_work(cx, async move {
			let client = ResetCardClient::new(profile);
			match client.consume(account.clone(), descriptor, revision, key.clone()).await {
				Ok(ResetCardConsumeResponse::Rejected { error }) => Update {
					completed_reset: None,
					inventory: None,
					blocked: true,
					pending_key: None,
					message: format!(
						"Reset was not accepted ({error:?}). Refresh the account before trying again."
					),
				},
				Err(_) => Update {
					completed_reset: None,
					inventory: None,
					blocked: true,
					pending_key: None,
					message: "Could not send the request. Refresh to check account availability."
						.into(),
				},
				Ok(
					ResetCardConsumeResponse::Accepted { .. }
					| ResetCardConsumeResponse::PotentiallyDispatched { .. },
				) => {
					// Only read status after dispatch. Never call consume again from a poll/retry.
					for _ in 0..20 {
						tokio::time::sleep(Duration::from_millis(500)).await;
						if let Ok(state) = client.status(key.clone()).await {
							if terminal(state) {
								break;
							}
						} else {
							break;
						}
					}
					load(&client, account, Some(key)).await
				},
			}
		});
	}

	fn start_reset_card_work(
		&mut self,
		cx: &mut Context<Self>,
		work: impl std::future::Future<Output = Update> + Send + 'static,
	) {
		let (sender, receiver) = mpsc::channel();
		self.reset_cards.busy = true;
		self.reset_cards.blocked = true;
		self.reset_cards.confirmation = None;
		self.reset_cards.message = "Checking Reset Card status…".into();
		self.reset_cards.updates = Some(receiver);
		cx.background_executor()
			.spawn(async move {
				let update =
					match tokio::runtime::Builder::new_current_thread().enable_all().build() {
						Ok(runtime) => runtime.block_on(work),
						Err(_) => Update {
							completed_reset: None,
							inventory: None,
							blocked: true,
							pending_key: None,
							message: "Reset Card client unavailable. Refresh to recover status."
								.into(),
						},
					};
				let _ = sender.send(update);
			})
			.detach();
		cx.notify();
	}

	pub(super) fn poll_reset_cards(&mut self, cx: &mut Context<Self>) {
		let Some(update) =
			self.reset_cards.updates.as_ref().and_then(|receiver| receiver.try_recv().ok())
		else {
			return;
		};
		if let Some((account, key)) = &update.completed_reset
			&& self.reset_cards.pending_key.as_ref() == Some(key)
			&& let Some(mut fill) = self.reset_cards.pending_fill.take()
		{
			fill.started = std::time::Instant::now();
			fill.confirmed_at_micros = time::OffsetDateTime::now_utc()
				.unix_timestamp_nanos()
				.checked_div(1000)
				.and_then(|value| i64::try_from(value).ok())
				.unwrap_or(i64::MAX);
			self.reset_cards
				.fills
				.retain(|id, _| self.accounts.accounts.iter().any(|row| &row.account_id == id));
			self.reset_cards.fills.insert(account.clone(), fill);
		}
		if !update.blocked {
			self.reset_cards.pending_fill = None;
		}
		self.reset_cards.updates = None;
		self.reset_cards.busy = false;
		self.reset_cards.inventory = update.inventory;
		self.reset_cards.blocked = update.blocked;
		self.reset_cards.pending_key = update.pending_key;
		self.reset_cards.message = update.message;
		cx.notify();
	}
}
async fn load(
	client: &ResetCardClient,
	account: EntityId,
	known_key: Option<IdempotencyKey>,
) -> Update {
	let inventory = client.list(account.clone()).await.ok();
	let (state, key) = if let Some(key) = known_key {
		(client.status(key.clone()).await.ok(), Some(key))
	} else {
		match client.latest_operation(account.clone()).await {
			Ok(AccountResetCardOperationResult::NotFound) =>
				(Some(ResetCardOperationResult::NotFound), None),
			Ok(AccountResetCardOperationResult::Found(operation)) =>
				(Some(operation.state), Some(operation.idempotency_key)),
			_ => (None, None),
		}
	};
	let (blocked, message) = operation_presentation(state, key.is_some());
	let completed_reset = match (&state, &key) {
		(
			Some(ResetCardOperationResult::Completed { outcome: ResetCardOutcome::Reset }),
			Some(key),
		) => Some((account, key.clone())),
		_ => None,
	};
	let pending_key = if blocked { key } else { None };
	Update { inventory, blocked, pending_key, message: message.into(), completed_reset }
}
fn terminal(state: ResetCardOperationResult) -> bool {
	matches!(
		state,
		ResetCardOperationResult::Completed { .. }
			| ResetCardOperationResult::FailedBeforeEffect { .. }
	)
}
fn operation_presentation(
	state: Option<ResetCardOperationResult>,
	known_request: bool,
) -> (bool, &'static str) {
	match state {
		Some(ResetCardOperationResult::NotFound) if !known_request =>
			(false, "Choose a card, then confirm to use it."),
		Some(ResetCardOperationResult::Completed { outcome: ResetCardOutcome::Reset }) =>
			(false, "Account limits were reset. The card was used."),
		Some(ResetCardOperationResult::Completed {
			outcome: ResetCardOutcome::AlreadyRedeemed,
		}) => (false, "The selected card was already redeemed. No other card was selected."),
		Some(ResetCardOperationResult::Completed { outcome: ResetCardOutcome::NothingToReset }) =>
			(false, "There was nothing to reset. No card was used."),
		Some(ResetCardOperationResult::Completed { outcome: ResetCardOutcome::NoCredit }) =>
			(false, "The selected card is no longer available. No other card was selected."),
		Some(ResetCardOperationResult::FailedBeforeEffect { .. }) => (
			false,
			"The request stopped before redemption. Refresh and check this account before trying again.",
		),
		Some(ResetCardOperationResult::Prepared) =>
			(true, "Your confirmed request is queued. Refresh to check its result."),
		Some(ResetCardOperationResult::EffectAmbiguous) => (
			true,
			"The result is not confirmed. Do not use another card. Refresh only checks status.",
		),
		_ => (
			true,
			"The previous request could not be verified. Refresh only checks status; redemption stays blocked.",
		),
	}
}
pub(super) fn panel(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
	let state = &shell.reset_cards;
	let (_, alias) = state.selected.as_ref()?;
	let busy = state.busy;
	let mut content = div()
		.id("reset-card-panel")
		.max_h(px(320.0))
		.overflow_y_scroll()
		.w_full()
		.p_3()
		.flex()
		.flex_col()
		.gap_2()
		.text_size(px(12.0))
		.text_color(rgb(WB_TEXT));
	content = content.child(panel_header(alias, busy, cx));
	content = content.child(div().text_color(rgb(WB_TEXT_MUTED)).child(state.message.clone()));
	match &state.inventory {
		Some(ResetCardInventoryResult::Available {
			details_complete,
			cards,
			account_revision,
			reported_available_count,
			..
		}) => {
			content = content.child(format!(
				"Available: {}",
				reported_available_count
					.map_or_else(|| "unknown".into(), |count| count.to_string())
			));
			if !details_complete {
				content = content
					.child("Card details are still loading. Refresh before choosing a card.");
			}
			for (index, card) in cards.iter().enumerate() {
				let descriptor = card.descriptor;
				let revision = *account_revision;
				let can_use = !state.blocked
					&& !busy && *details_complete
					&& descriptor.expires_at_unix_seconds()
						> time::OffsetDateTime::now_utc().unix_timestamp();
				content = content.child(
					div()
						.flex()
						.items_center()
						.justify_between()
						.child(format!(
							"Granted {} · expires {}",
							date(descriptor.granted_at_unix_seconds()),
							date(descriptor.expires_at_unix_seconds())
						))
						.child(
							account_row_action(
								"reset-select",
								index,
								"Select this Reset Card",
								"Use card…",
								can_use,
							)
							.debug_selector(move || format!("reset-select-{index}"))
							.when(can_use, |button| {
								button.on_click(cx.listener(move |shell, _, _, cx| {
									shell.reset_cards.confirmation = Some((descriptor, revision));
									cx.notify();
								}))
							}),
						),
				);
			}
		},
		Some(
			ResetCardInventoryResult::ObservationFailed { .. }
			| ResetCardInventoryResult::Unavailable { .. },
		)
		| None => {
			content = content
				.child("Card inventory is unavailable. Refresh or check this account's login.");
		},
	}
	if let Some((descriptor, _)) = state.confirmation {
		content = content.child(
			div()
				.flex()
				.flex_col()
				.gap_2()
				.child(format!(
					"Use one Reset Card for {alias}, expiring {}? This cannot be undone.",
					date(descriptor.expires_at_unix_seconds())
				))
				.child(
					div()
						.flex()
						.gap_2()
						.child(
							account_row_action(
								"reset-confirm",
								0,
								"Confirm use of one Reset Card",
								"Confirm · use 1 card",
								true,
							)
							.on_click(cx.listener(|shell, _, _, cx| shell.confirm_reset_card(cx))),
						)
						.child(
							account_row_action(
								"reset-cancel",
								0,
								"Cancel Reset Card selection",
								"Cancel",
								true,
							)
							.debug_selector(|| "reset-cancel".into())
							.on_click(cx.listener(|shell, _, _, cx| {
								shell.reset_cards.confirmation = None;
								cx.notify();
							})),
						),
				),
		);
	}
	Some(content.into_any_element())
}
fn panel_header(alias: &str, busy: bool, cx: &mut Context<Shell>) -> AnyElement {
	div()
		.flex()
		.items_center()
		.justify_between()
		.child(format!("Reset Cards · {alias}"))
		.child(
			div()
				.flex()
				.gap_2()
				.child(
					account_row_action(
						"reset-refresh",
						0,
						"Refresh Reset Card status",
						"Refresh",
						!busy,
					)
					.when(!busy, |button| {
						button
							.on_click(cx.listener(|shell, _, _, cx| shell.refresh_reset_cards(cx)))
					}),
				)
				.child(
					account_row_action("reset-close", 0, "Close Reset Cards", "Close", true)
						.on_click(cx.listener(|shell, _, _, cx| {
							shell.reset_cards.selected = None;
							shell.reset_cards.confirmation = None;
							cx.notify();
						})),
				),
		)
		.into_any_element()
}

fn date(seconds: i64) -> String {
	time::OffsetDateTime::from_unix_timestamp(seconds)
		.map_or_else(|_| "unknown".into(), |value| value.date().to_string())
}
#[cfg(test)]
mod tests {
	use super::{operation_presentation, terminal};
	use decodex_protocol::{ResetCardOperationResult, ResetCardOutcome};
	#[test]
	fn missing_or_ambiguous_receipts_block_new_redemption() {
		assert!(operation_presentation(None, false).0);
		assert!(operation_presentation(Some(ResetCardOperationResult::EffectAmbiguous), true).0);
		assert!(operation_presentation(Some(ResetCardOperationResult::NotFound), true).0);
		assert!(!operation_presentation(Some(ResetCardOperationResult::NotFound), false).0);
	}
	#[test]
	fn completed_receipt_reports_the_exact_outcome() {
		let state =
			ResetCardOperationResult::Completed { outcome: ResetCardOutcome::NothingToReset };
		assert!(terminal(state));
		let (blocked, message) = operation_presentation(Some(state), true);
		assert!(!blocked);
		assert!(message.contains("No card was used"));
	}
}

#[cfg(test)]
mod render_tests {
	use super::{super::Shell, ResetCardsPanel, panel};
	use crate::client_lifecycle::ConnectionView;
	use decodex_protocol::{
		AccountQuotaStateDto, AccountQuotaWindowDto, EntityId, EntityRevision,
		ResetCardDescriptorDto, ResetCardInventoryResult, ResetCardObservationDto,
	};
	use gpui::{
		AppContext as _, Context, Entity, IntoElement as _, Modifiers, Render, TestAppContext,
		Window, px, size,
	};
	struct Harness {
		shell: Entity<Shell>,
	}
	impl Render for Harness {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
			self.shell.update(cx, |shell, cx| {
				panel(shell, cx).unwrap_or_else(|| gpui::div().into_any_element())
			})
		}
	}
	#[gpui::test]
	fn select_and_cancel_never_dispatch_and_uncertain_state_disables_selection(
		cx: &mut TestAppContext,
	) {
		let (harness, visual) = cx.add_window_view(|window, cx| Harness {
			shell: cx.new(|cx| Shell::new(window, cx, ConnectionView::Stopped)),
		});
		let shell = harness.read_with(visual, |harness, _| harness.shell.clone());
		shell.update(visual, |shell, cx| {
			let account =
				EntityId::new("21000000-0000-4000-8000-000000000099").expect("fake account");
			let descriptor = ResetCardDescriptorDto::new(1, 4102444800).expect("fake descriptor");
			let quota = |duration_minutes| AccountQuotaWindowDto {
				duration_minutes,
				observed_at_unix_micros: None,
				result: AccountQuotaStateDto::Unknown,
			};
			shell.reset_cards = ResetCardsPanel {
				selected: Some((account.clone(), "Fake account".into())),
				inventory: Some(ResetCardInventoryResult::Available {
					account_id: account,
					account_revision: EntityRevision(1),
					reported_available_count: Some(1),
					details_complete: true,
					cards: vec![ResetCardObservationDto { descriptor }],
					five_hour_quota: quota(300),
					seven_day_quota: quota(10080),
				}),
				..ResetCardsPanel::default()
			};
			// No ClientProfile: this rendered fixture cannot connect to any service.
			cx.notify();
		});
		visual.update(|window, cx| {
			window.resize(size(px(800.), px(600.)));
			window.draw(cx).clear();
		});
		let select = visual.debug_bounds("reset-select-0").expect("visible select button");
		visual.simulate_click(select.center(), Modifiers::default());
		shell.read_with(visual, |shell, _| {
			assert!(shell.reset_cards.confirmation.is_some());
			assert!(!shell.reset_cards.busy);
			assert!(shell.reset_cards.pending_key.is_none());
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let cancel = visual.debug_bounds("reset-cancel").expect("visible cancellation");
		visual.simulate_click(cancel.center(), Modifiers::default());
		shell.update(visual, |shell, cx| {
			assert!(shell.reset_cards.confirmation.is_none());
			assert!(shell.reset_cards.updates.is_none());
			shell.reset_cards.blocked = true;
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let select =
			visual.debug_bounds("reset-select-0").expect("disabled selection remains visible");
		visual.simulate_click(select.center(), Modifiers::default());
		shell.read_with(visual, |shell, _| assert!(shell.reset_cards.confirmation.is_none()));
	}
}
