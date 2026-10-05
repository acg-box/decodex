//! One workspace notification center with dismissible current notices.
use std::{cell::Cell, collections::HashSet, rc::Rc, sync::atomic::AtomicU8};

use gpui::{
	self, AnyElement, Bounds, Context, InteractiveElement as _, IntoElement as _, KeyDownEvent,
	MouseDownEvent, ParentElement as _, Pixels, Role, SharedString,
	StatefulInteractiveElement as _, Styled as _, prelude::FluentBuilder as _,
};

use crate::{
	account_profile::{self, AccountProfileLoadState},
	accounts::{AccountCommandState, AccountsLoadState},
	shell::{
		self, ConnectionPresentation, Destination, Shell, WB_BLUE, WB_TEXT, WB_TEXT_MUTED,
		workspace_symbols::{self, Symbol},
	},
	ui_motion::{self, SmoothControl},
	ui_preferences,
	ui_theme::{
		AMBER, BLUE, CHROME_CONTROL_SIZE, CONTROL_GROUP_HEIGHT, CONTROL_MARGIN, ERROR, HOVER_FILL,
	},
};
use decodex_protocol::{AccountLoginState, AccountProfileResult};

#[derive(Clone, Copy)]
enum Recovery {
	General,
	Accounts,
	None,
	LoginItems,
}
struct Notice {
	title: &'static str,
	detail: String,
	recovery: Recovery,
	color: u32,
	identity: Option<String>,
}
impl Notice {
	fn key(&self) -> (String, String) {
		(self.title.to_owned(), self.identity.as_ref().unwrap_or(&self.detail).clone())
	}

	fn new(title: &'static str, detail: impl Into<String>, recovery: Recovery) -> Self {
		Self { title, detail: detail.into(), recovery, color: AMBER, identity: None }
	}

	fn error(mut self) -> Self {
		self.color = ERROR;

		self
	}

	fn info(mut self) -> Self {
		self.color = BLUE;

		self
	}
}
impl Shell {
	pub(super) fn render_status_center(
		&self,
		connection: &ConnectionPresentation,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let open = self.status_open;
		let panel = self.render_status_panel(connection, cx);
		#[cfg(not(all(target_os = "macos", not(test))))]
		let native = false;
		#[cfg(all(target_os = "macos", not(test)))]
		let native = self.native_status.child.is_some();
		let popup_bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
		let outside_bounds = popup_bounds.clone();

		gpui::div()
			.id("status-center")
			.w(gpui::px(304.))
			.absolute()
			.top(gpui::px(CONTROL_MARGIN + CONTROL_GROUP_HEIGHT + CONTROL_MARGIN))
			.right(gpui::px(CONTROL_MARGIN))
			.flex()
			.flex_col()
			.items_end()
			.gap_2()
			.on_mouse_down_out(cx.listener(move |s, event: &MouseDownEvent, _, cx| {
				if open
					&& !outside_bounds.get().is_some_and(|bounds| bounds.contains(&event.position))
				{
					s.status_open = false;

					cx.notify();
				}
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if open && event.keystroke.key == "escape" {
					s.status_open = false;

					cx.notify();
					cx.stop_propagation();
				}
			}))
			.child(
				gpui::div()
					.absolute()
					.top_0()
					.right_0()
					.w_full()
					.on_children_prepainted(move |bounds, _, _| {
						popup_bounds.set(bounds.first().copied())
					})
					.when(!native, |d| d.child(ui_motion::popover(open, panel))),
			)
			.into_any_element()
	}

	fn notifications(
		&self,
		connection: &ConnectionPresentation,
		cx: &Context<Self>,
	) -> Vec<Notice> {
		let mut notices = self
			.settings
			.read(cx)
			.notifications()
			.into_iter()
			.map(|(title, detail)| {
				Notice::new(
					title,
					detail,
					if title == "Launch at login" {
						Recovery::LoginItems
					} else {
						Recovery::General
					},
				)
			})
			.collect::<Vec<_>>();

		self.account_notifications(&mut notices);
		self.profile_notifications(&mut notices);

		let agent = self.agent.read(cx);

		if let Some((identity, detail)) = agent.question_arrival_notice() {
			let mut notice = Notice::new("Question", detail, Recovery::None);

			notice.identity = Some(identity);

			notices.insert(0, notice);
		}

		notices.extend(
			agent
				.operation_notices()
				.into_iter()
				.map(|(title, detail)| Notice::new(title, detail, Recovery::None)),
		);

		if connection.label != "Online" {
			notices.push(Notice::new(
				connection.label,
				connection.detail.to_string(),
				Recovery::None,
			));
		}

		let mut seen = HashSet::new();

		notices.retain(|notice| seen.insert(notice.key()));

		let current = notices.iter().map(Notice::key).collect::<HashSet<_>>();
		let mut dismissed = self.dismissed_notifications.borrow_mut();

		dismissed.retain(|key| current.contains(key));
		notices.retain(|notice| !dismissed.contains(&notice.key()));

		notices
	}

	fn account_notifications(&self, notices: &mut Vec<Notice>) {
		let snapshot = &self.accounts;

		if snapshot.route_reopen_notice {
			notices.push(
				Notice::new(
					"Account routing",
					"Route succeeded. You can reopen ChatGPT or Codex now.",
					Recovery::Accounts,
				)
				.info(),
			);
		}

		if let Some(detail) = &self.account_status {
			notices.push(Notice::new("Accounts", detail.to_string(), Recovery::Accounts));
		}
		if let Some(rejection) = snapshot.rejection {
			notices.push(Notice::new(
				"Accounts",
				shell::account_rejection_label(rejection),
				Recovery::Accounts,
			));
		} else if matches!(
			snapshot.command,
			AccountCommandState::Refused | AccountCommandState::OutcomeUnknown
		) && let Some(detail) = shell::account_command_label(snapshot.command)
		{
			notices.push(Notice::new("Accounts", detail, Recovery::Accounts));
		}

		if matches!(
			snapshot.load,
			AccountsLoadState::Offline
				| AccountsLoadState::Stale
				| AccountsLoadState::Unavailable
				| AccountsLoadState::Refused
		) {
			notices.push(Notice::new(
				"Accounts",
				shell::accounts_load_label(snapshot.load),
				Recovery::Accounts,
			));
		}

		if let Some(detail) = &self.account_login_error {
			if detail.as_ref() != "Cancelling sign-in…" {
				let notice = Notice::new("Account sign-in", detail.to_string(), Recovery::Accounts);

				notices.push(if detail.as_ref() == "Sign-in code copied." {
					notice.info()
				} else {
					notice
				});
			}
		} else if let Some(status) = &self.account_login_status {
			match status.state {
				AccountLoginState::Failed => notices.push(
					Notice::new(
						"Account sign-in",
						shell::account_login_status_label(status),
						Recovery::Accounts,
					)
					.error(),
				),
				AccountLoginState::Completed | AccountLoginState::Cancelled => notices.push(
					Notice::new(
						"Account sign-in",
						shell::account_login_status_label(status),
						Recovery::Accounts,
					)
					.info(),
				),
				_ => {},
			}
		}

		if matches!(
			self.account_profile.load,
			AccountProfileLoadState::Offline | AccountProfileLoadState::Refused
		) {
			notices.push(Notice::new(
				"Account profile",
				if self.account_profile.load == AccountProfileLoadState::Offline {
					"Connect to the service to update your activity."
				} else {
					"Your activity couldn’t be loaded. Try again later."
				},
				Recovery::Accounts,
			));
		}
	}

	fn profile_notifications(&self, notices: &mut Vec<Notice>) {
		let detail = match self.account_profile.result.as_ref() {
			Some(AccountProfileResult::Cached { refresh_error, .. }) =>
				Some(if account_profile::requires_login(*refresh_error) {
					"Sign in again to update your activity."
				} else {
					"Your activity couldn’t be updated. Try again later."
				}),
			Some(AccountProfileResult::Unavailable { error, .. }) =>
				Some(if account_profile::requires_login(*error) {
					"Sign in again to view your activity."
				} else {
					"Your activity couldn’t be loaded. Try again later."
				}),
			_ => None,
		};

		if let Some(detail) = detail {
			let mut notice = Notice::new("Account profile", detail, Recovery::Accounts);

			notice.identity = self.account_profile.selected.as_ref().map(|account| {
				format!(
					"{}/{:?}/{detail}",
					account.as_str(),
					self.account_profile.selected_revision
				)
			});

			let requires_login = match self.account_profile.result.as_ref() {
				Some(AccountProfileResult::Cached { refresh_error, .. }) =>
					account_profile::requires_login(*refresh_error),
				Some(AccountProfileResult::Unavailable { error, .. }) =>
					account_profile::requires_login(*error),
				_ => false,
			};

			notices.push(if requires_login { notice.error() } else { notice });
		}
	}

	pub(super) fn render_status_toggle(
		&self,
		connection: &ConnectionPresentation,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let notices = self.notifications(connection, cx);
		let color = notice_color(&notices);
		let label = if notices.is_empty() {
			"Status".to_owned()
		} else {
			format!("Notifications · {}", notices.len())
		};
		let open = self.status_open;
		gpui::div()
			.id("status-toggle")
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label.clone())
			.aria_expanded(open)
			.size(gpui::px(CHROME_CONTROL_SIZE))
			.relative()
			.rounded(gpui::px(6.))
			.flex()
			.items_center()
			.justify_center()
			.text_size(gpui::px(11.))
			.text_color(gpui::rgb(WB_TEXT_MUTED))
			.cursor_pointer()
			.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.status_open = !open;

				cx.notify();
			}))
			.child(workspace_symbols::icon(if notices.is_empty() {
				Symbol::Bell
			} else if color == ERROR {
				Symbol::BellError
			} else if color == AMBER {
				Symbol::BellAttention
			} else {
				Symbol::BellInfo
			}))
			.when(count_preference(None) && !notices.is_empty(), |d| {
				d.child(
					gpui::div()
						.absolute()
						.top(gpui::px(-4.))
						.right(gpui::px(-5.))
						.min_w(gpui::px(14.))
						.h(gpui::px(14.))
						.px(gpui::px(3.))
						.rounded_full()
						.bg(gpui::rgb(color))
						.text_color(gpui::rgb(0x17171a))
						.text_size(gpui::px(9.))
						.flex()
						.items_center()
						.justify_center()
						.child(if notices.len() > 99 {
							"99+".into()
						} else {
							notices.len().to_string()
						}),
				)
			})
			.smooth()
			.into_any_element()
	}

	pub(super) fn render_status_panel(
		&self,
		connection: &ConnectionPresentation,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let notices = self.notifications(connection, cx);
		let mut panel =
			gpui::div().occlude().w(gpui::px(304.)).p_3().flex().flex_col().gap_3().child(
				gpui::div()
					.flex()
					.items_center()
					.justify_between()
					.child(
						gpui::div()
							.text_size(gpui::px(12.))
							.text_color(gpui::rgb(WB_TEXT))
							.child("Notifications"),
					)
					.when(!notices.is_empty(), |d| {
						d.child(
							gpui::div()
								.id("clear-notifications")
								.role(Role::Button)
								.aria_label("Clear all notifications")
								.tab_index(0)
								.text_size(gpui::px(11.))
								.text_color(gpui::rgb(WB_TEXT_MUTED))
								.cursor_pointer()
								.hover(|d| d.text_color(gpui::rgb(WB_TEXT)))
								.on_click(cx.listener(|s, _, _, cx| {
									let notices = s.notifications(
										&shell::connection_presentation(s.connection),
										cx,
									);

									s.dismissed_notifications
										.borrow_mut()
										.extend(notices.iter().map(Notice::key));
									cx.notify();
								}))
								.child("Clear all"),
						)
					}),
			);

		if notices.is_empty() {
			panel = panel.child(
				gpui::div()
					.text_size(gpui::px(11.))
					.text_color(gpui::rgb(WB_TEXT_MUTED))
					.child("No notifications."),
			);
		}

		panel
			.child(
				gpui::div()
					.id("notification-list")
					.max_h(gpui::px(340.))
					.overflow_y_scroll()
					.flex()
					.flex_col()
					.gap_3()
					.children(notices.into_iter().enumerate().map(|(index, notice)| {
						let key = notice.key();

						gpui::div()
							.flex()
							.flex_col()
							.gap_1()
							.child(
								gpui::div()
									.flex()
									.items_center()
									.justify_between()
									.child(
										gpui::div()
											.text_size(gpui::px(11.))
											.text_color(gpui::rgb(notice.color))
											.child(notice.title),
									)
									.child(
										gpui::div()
											.id(SharedString::from(format!(
												"dismiss-notice-{index}"
											)))
											.role(Role::Button)
											.aria_label(format!("Dismiss {}", notice.title))
											.tab_index(0)
											.size(gpui::px(22.))
											.flex()
											.items_center()
											.justify_center()
											.rounded(gpui::px(5.))
											.cursor_pointer()
											.hover(|d| d.bg(gpui::rgba(HOVER_FILL)))
											.on_click(cx.listener(move |s, _, _, cx| {
												s.dismissed_notifications
													.borrow_mut()
													.insert(key.clone());
												cx.notify();
											}))
											.child(workspace_symbols::icon(Symbol::Close)),
									),
							)
							.child(
								gpui::div()
									.text_size(gpui::px(11.))
									.line_height(gpui::px(17.))
									.text_color(gpui::rgb(notice.color))
									.child(notice.detail),
							)
							.when(!matches!(notice.recovery, Recovery::None), |d| {
								d.child(self.notification_action(index, notice.recovery, cx))
							})
					})),
			)
			.into_any_element()
	}

	fn notification_action(
		&self,
		index: usize,
		recovery: Recovery,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let label = match recovery {
			Recovery::General => "Settings",
			Recovery::Accounts => "Accounts",
			Recovery::None => unreachable!(),
			Recovery::LoginItems => "Open Login Items",
		};

		gpui::div()
			.id(SharedString::from(format!("notice-action-{index}")))
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label)
			.h(gpui::px(24.))
			.px_2()
			.rounded(gpui::px(5.))
			.flex()
			.items_center()
			.text_size(gpui::px(11.))
			.text_color(gpui::rgb(WB_BLUE))
			.cursor_pointer()
			.hover(|d| d.bg(gpui::rgba(HOVER_FILL)))
			.on_click(cx.listener(move |s, event, window, cx| {
				s.status_open = false;

				match recovery {
					Recovery::LoginItems => s.settings.update(cx, |settings, cx| {
						settings.open_login_items_settings(event, window, cx)
					}),
					Recovery::General => {
						s.select_settings_section(Destination::Settings, cx);
						s.open_settings_window(Destination::Settings, cx);
					},
					Recovery::Accounts => s.open_settings_window(Destination::Accounts, cx),
					Recovery::None => {},
				}

				cx.notify();
			}))
			.child(label)
			.smooth()
			.into_any_element()
	}
}

pub(crate) fn count_preference(value: Option<bool>) -> bool {
	static VALUE: AtomicU8 = AtomicU8::new(u8::MAX);

	ui_preferences::boolean("DecodexNotificationCount", &VALUE, value, false)
}

pub(crate) fn question_notice_preference(value: Option<bool>) -> bool {
	static VALUE: AtomicU8 = AtomicU8::new(u8::MAX);

	ui_preferences::boolean("DecodexQuestionNotices", &VALUE, value, true)
}

fn notice_color(notices: &[Notice]) -> u32 {
	notices
		.iter()
		.find(|n| n.color == ERROR)
		.or_else(|| notices.iter().find(|n| n.color == AMBER))
		.or(notices.first())
		.map_or(WB_TEXT_MUTED, |n| n.color)
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use gpui::{
		self, AppContext as _, Context, Entity, IntoElement, KeyDownEvent, KeyUpEvent, Keystroke,
		PlatformInput, Render, Subscription, TestAppContext, Window,
	};

	use crate::{
		client_lifecycle::ConnectionView,
		shell::{
			self, Shell,
			status::{Notice, Recovery},
		},
	};
	use decodex_protocol::{
		AccountProfileEmailDto, AccountProfileErrorDto, AccountProfileResult, EntityId,
		EntityRevision,
	};
	struct StatusFixture {
		shell: Entity<Shell>,
		_subscription: Subscription,
	}
	impl Render for StatusFixture {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			self.shell.update(cx, |s, cx| {
				s.render_status_panel(&shell::connection_presentation(s.connection), cx)
			})
		}
	}

	fn check_keyboard_dismissal(cx: &mut TestAppContext, tabs: usize, clear_all: bool) {
		let (fixture, visual) = cx.add_window_view(|window, cx| {
			let shell = cx.new(|cx| Shell::new(window, cx, ConnectionView::Stopped));

			shell.update(cx, |s, _| s.account_status = Some("Fixture account warning".into()));

			let subscription = cx.observe(&shell, |_, _, cx| cx.notify());

			StatusFixture { shell, _subscription: subscription }
		});
		let shell = fixture.read_with(visual, |f, _| f.shell.clone());
		let initial = shell.update(visual, |s, cx| {
			s.notifications(&shell::connection_presentation(s.connection), cx).len()
		});

		assert!(initial >= 2);

		visual.update(|w, cx| {
			w.draw(cx).clear();
			w.blur();
		});

		for _ in 0..tabs {
			visual.update(|w, cx| {
				w.focus_next(cx);
			});
		}

		visual.update(|w, cx| {
			w.draw(cx).clear();

			let keystroke = Keystroke::parse("enter").expect("activation key");

			w.dispatch_event(
				PlatformInput::KeyDown(KeyDownEvent {
					keystroke: keystroke.clone(),
					is_held: false,
					prefer_character_input: false,
				}),
				cx,
			);
			w.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
		});

		shell.update(visual, |s, cx| {
			assert_eq!(
				s.notifications(&shell::connection_presentation(s.connection), cx).len(),
				if clear_all { 0 } else { initial - 1 }
			);
			assert_eq!(s.account_status.as_deref(), Some("Fixture account warning"));
		});
	}

	#[gpui::test]
	fn clear_notifications_is_keyboard_reachable(cx: &mut TestAppContext) {
		check_keyboard_dismissal(cx, 1, true);
	}

	#[gpui::test]
	fn dismiss_one_notification_is_keyboard_reachable(cx: &mut TestAppContext) {
		check_keyboard_dismissal(cx, 2, false);
	}

	#[test]
	fn question_dismissal_uses_identity_instead_of_repeated_title() {
		let mut first = Notice::new("Question", "Choose a format", Recovery::None);

		first.identity = Some("first-question".into());

		let mut second = Notice::new("Question", "Choose a format", Recovery::None);

		second.identity = Some("second-question".into());

		assert_ne!(first.key(), second.key());

		let dismissed = HashSet::from([first.key()]);

		assert!(!dismissed.contains(&second.key()));
	}

	#[gpui::test]
	fn independent_failures_are_collected_and_clear_with_their_source(cx: &mut TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

		shell.update(visual, |s, cx| {
			s.account_status = Some("Account change refused".into());
			s.input_status = Some("Message was not delivered".into());

			let connection = shell::connection_presentation(s.connection);
			let notices = s.notifications(&connection, cx);

			assert!(
				notices
					.iter()
					.any(|n| n.title == "Accounts" && n.detail == "Account change refused")
			);
			assert!(
				!notices.iter().any(|n| n.detail == "Message was not delivered"),
				"thread delivery errors belong beside the conversation"
			);

			s.account_status = None;
			s.input_status = None;

			assert!(
				!s.notifications(&connection, cx)
					.iter()
					.any(|n| n.detail == "Account change refused"
						|| n.detail == "Message was not delivered")
			);
		});
	}
	#[gpui::test]
	fn dismissed_notice_stays_hidden_until_source_recovers(cx: &mut TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

		shell.update(visual, |s, cx| {
			let connection = shell::connection_presentation(s.connection);

			s.account_status = Some("Failed".into());

			let notices = s.notifications(&connection, cx);
			let key = notices.iter().find(|n| n.title == "Accounts").unwrap().key();

			s.dismissed_notifications.borrow_mut().insert(key);

			assert!(!s.notifications(&connection, cx).iter().any(|n| n.title == "Accounts"));

			s.account_status = Some("Different failure".into());

			assert!(
				s.notifications(&connection, cx).iter().any(|n| n.detail == "Different failure")
			);

			s.account_status = None;

			s.notifications(&connection, cx);

			s.account_status = Some("Failed".into());

			assert!(s.notifications(&connection, cx).iter().any(|n| n.detail == "Failed"));
		});
	}
	#[gpui::test]
	fn profile_dismissal_is_scoped_to_account_and_revision(cx: &mut TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|w, cx| Shell::new(w, cx, ConnectionView::Stopped));

		shell.update(visual, |s, cx| {
			s.account_profile.result = Some(AccountProfileResult::Unavailable {
				error: AccountProfileErrorDto::Unauthorized,
				email: AccountProfileEmailDto::Redacted,
				plan_type: None,
			});

			let connection = shell::connection_presentation(s.connection);

			for (id, revision) in [
				("20000000-0000-4000-8000-000000000001", 1),
				("20000000-0000-4000-8000-000000000002", 1),
				("20000000-0000-4000-8000-000000000002", 2),
			] {
				s.account_profile.selected = Some(EntityId::new(id).unwrap());
				s.account_profile.selected_revision = Some(EntityRevision(revision));

				let notices = s.notifications(&connection, cx);
				let profile = notices
					.iter()
					.find(|n| n.title == "Account profile")
					.expect("a new account or revision must have its own warning");

				s.dismissed_notifications.borrow_mut().insert(profile.key());

				assert!(
					!s.notifications(&connection, cx).iter().any(|n| n.title == "Account profile")
				);
			}
		});
	}
}
