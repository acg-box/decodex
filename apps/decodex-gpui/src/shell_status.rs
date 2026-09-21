//! One workspace notification center with dismissible current notices.
use super::*;
use crate::ui_motion::{SmoothControl, popover};

#[derive(Clone, Copy)]
enum Recovery {
	General,
	Accounts,
	None,
	RefreshChief,
	LoginItems,
}
struct Notice {
	title: &'static str,
	detail: String,
	recovery: Recovery,
	color: u32,
}
impl Notice {
	fn key(&self) -> (String, String) {
		(self.title.to_owned(), self.detail.clone())
	}

	fn new(title: &'static str, detail: impl Into<String>, recovery: Recovery) -> Self {
		Self { title, detail: detail.into(), recovery, color: ui_theme::AMBER }
	}

	fn info(mut self) -> Self {
		self.color = ui_theme::BLUE;
		self
	}
}
impl Shell {
	pub(super) fn render_status_center(
		&self,
		connection: &ConnectionPresentation,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let notices = self.notifications(connection, cx);
		let color = notices
			.iter()
			.find(|n| n.color == ui_theme::AMBER)
			.or(notices.first())
			.map_or(WB_TEXT_MUTED, |n| n.color);
		let label = if notices.is_empty() {
			"Status".to_owned()
		} else {
			format!("Notifications · {}", notices.len())
		};
		let open = self.status_open;
		let panel = self.render_status_panel(connection, cx);
		#[cfg(not(all(target_os = "macos", not(test))))]
		let native = false;
		#[cfg(all(target_os = "macos", not(test)))]
		let native = self.native_status.child.is_some();
		let popup_bounds =
			std::rc::Rc::new(std::cell::Cell::new(None::<gpui::Bounds<gpui::Pixels>>));
		let outside_bounds = popup_bounds.clone();
		div()
			.id("status-center")
			.w(px(304.))
			.absolute()
			.bottom(px(ui_theme::CONTROL_MARGIN))
			.right(px(ui_theme::CONTROL_MARGIN))
			.flex()
			.flex_col()
			.items_end()
			.gap_2()
			.on_mouse_down_out(cx.listener(move |s, event: &gpui::MouseDownEvent, _, cx| {
				if open
					&& !outside_bounds.get().is_some_and(|bounds| bounds.contains(&event.position))
				{
					s.status_open = false;
					cx.notify();
				}
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
				if open && event.keystroke.key == "escape" {
					s.status_open = false;
					cx.notify();
					cx.stop_propagation();
				}
			}))
			.child(
				div()
					.absolute()
					.bottom(px(ui_theme::CONTROL_GROUP_HEIGHT + ui_theme::CONTROL_MARGIN))
					.right_0()
					.w_full()
					.on_children_prepainted(move |bounds, _, _| {
						popup_bounds.set(bounds.first().copied())
					})
					.when(!native, |d| {
						d.child(popover("status-panel-motion", "status", open, panel))
					}),
			)
			.child(
				ui_theme::floating_group().child(
					div()
						.id("status-toggle")
						.role(Role::Button)
						.tab_index(0)
						.aria_label(label.clone())
						.aria_expanded(open)
						.size(px(ui_theme::CHROME_CONTROL_SIZE))
						.relative()
						.rounded(px(6.))
						.flex()
						.items_center()
						.justify_center()
						.text_size(px(11.))
						.text_color(rgb(WB_TEXT_MUTED))
						.cursor_pointer()
						.hover(|s| s.bg(rgba(0xffffff0c)))
						.on_click(cx.listener(move |s, _, _, cx| {
							s.status_open = !open;
							cx.notify();
						}))
						.child(workspace_symbols::icon(if notices.is_empty() {
							workspace_symbols::Symbol::Bell
						} else if color == ui_theme::AMBER {
							workspace_symbols::Symbol::BellAttention
						} else {
							workspace_symbols::Symbol::BellInfo
						}))
						.when(count_preference(None) && !notices.is_empty(), |d| {
							d.child(
								div()
									.absolute()
									.top(px(-4.))
									.right(px(-5.))
									.min_w(px(14.))
									.h(px(14.))
									.px(px(3.))
									.rounded_full()
									.bg(rgb(color))
									.text_color(rgb(0x17171a))
									.text_size(px(9.))
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
						.smooth(),
				),
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
		self.conversation_notifications(&mut notices);
		let chief = self.chief.read(cx);
		if let Some((title, detail, retry)) =
			chief.status_notice().filter(|(title, _, _)| *title != "Sending")
		{
			notices.push(Notice::new(
				title,
				detail,
				if retry { Recovery::RefreshChief } else { Recovery::None },
			));
		}
		notices.extend(
			chief
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
		let mut seen = std::collections::HashSet::new();
		notices.retain(|notice| seen.insert((notice.title, notice.detail.clone())));
		let current = notices.iter().map(Notice::key).collect::<std::collections::HashSet<_>>();
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
				account_rejection_label(rejection),
				Recovery::Accounts,
			));
		} else if matches!(
			snapshot.command,
			AccountCommandState::Refused | AccountCommandState::OutcomeUnknown
		) && let Some(detail) = account_command_label(snapshot.command)
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
				accounts_load_label(snapshot.load),
				Recovery::Accounts,
			));
		}
		if let Some(detail) = &self.account_login_error {
			if detail.as_ref() != "Cancelling account login…" {
				let notice = Notice::new("Account login", detail.to_string(), Recovery::Accounts);
				notices.push(if detail.as_ref() == "Login code copied." {
					notice.info()
				} else {
					notice
				});
			}
		} else if let Some(status) = &self.account_login_status {
			match status.state {
				AccountLoginState::Failed => notices.push(Notice::new(
					"Account login",
					account_login_status_label(status),
					Recovery::Accounts,
				)),
				AccountLoginState::Completed | AccountLoginState::Cancelled => notices.push(
					Notice::new(
						"Account login",
						account_login_status_label(status),
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
				account_profile_load_label(self.account_profile.load),
				Recovery::Accounts,
			));
		}
	}

	fn profile_notifications(&self, notices: &mut Vec<Notice>) {
		let detail = match self.account_profile.result.as_ref() {
			Some(AccountProfileResult::Cached { refresh_error, .. }) => Some(format!(
				"Refresh failed: {refresh_error:?}. Cached profile remains available."
			)),
			Some(AccountProfileResult::Unavailable { error, .. }) =>
				Some(format!("Profile unavailable: {error:?}.")),
			_ => None,
		};
		if let Some(detail) = detail {
			notices.push(Notice::new("Account profile", detail, Recovery::Accounts));
		}
	}

	fn conversation_notifications(&self, notices: &mut Vec<Notice>) {
		if let Some(detail) = &self.input_status {
			notices.push(Notice::new("Message delivery", detail.to_string(), Recovery::None));
		}
		if matches!(
			self.quick.command,
			ConversationCommandState::Refused | ConversationCommandState::OutcomeUnknown
		) && let Some(detail) = command_status(self.quick.command)
		{
			notices.push(Notice::new("Conversation", detail, Recovery::None));
		}
		if matches!(
			self.quick.load,
			ConversationsLoadState::Offline
				| ConversationsLoadState::Unavailable
				| ConversationsLoadState::Refused
		) {
			notices.push(Notice::new(
				"Conversation",
				conversation_load_status(self.quick.load),
				Recovery::None,
			));
		}
	}

	pub(super) fn render_status_panel(
		&self,
		connection: &ConnectionPresentation,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let notices = self.notifications(connection, cx);
		let mut panel = div().occlude().w(px(304.)).p_3().flex().flex_col().gap_3().child(
			div()
				.flex()
				.items_center()
				.justify_between()
				.child(div().text_size(px(12.)).text_color(rgb(WB_TEXT)).child("Notifications"))
				.when(!notices.is_empty(), |d| {
					d.child(
						div()
							.id("clear-notifications")
							.role(Role::Button)
							.aria_label("Clear all notifications")
							.text_size(px(11.))
							.text_color(rgb(WB_TEXT_MUTED))
							.cursor_pointer()
							.hover(|d| d.text_color(rgb(WB_TEXT)))
							.on_click(cx.listener(|s, _, _, cx| {
								let notices =
									s.notifications(&connection_presentation(s.connection), cx);
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
				div().text_size(px(11.)).text_color(rgb(WB_TEXT_MUTED)).child("No notifications."),
			);
		}
		panel
			.child(
				div()
					.id("notification-list")
					.max_h(px(340.))
					.overflow_y_scroll()
					.flex()
					.flex_col()
					.gap_3()
					.children(notices.into_iter().enumerate().map(|(index, notice)| {
						let key = notice.key();
						div()
							.flex()
							.flex_col()
							.gap_1()
							.child(
								div()
									.flex()
									.items_center()
									.justify_between()
									.child(
										div()
											.text_size(px(11.))
											.text_color(rgb(notice.color))
											.child(notice.title),
									)
									.child(
										div()
											.id(SharedString::from(format!(
												"dismiss-notice-{index}"
											)))
											.role(Role::Button)
											.aria_label(format!("Dismiss {}", notice.title))
											.size(px(22.))
											.flex()
											.items_center()
											.justify_center()
											.rounded(px(5.))
											.cursor_pointer()
											.hover(|d| d.bg(rgba(0xffffff0c)))
											.on_click(cx.listener(move |s, _, _, cx| {
												s.dismissed_notifications
													.borrow_mut()
													.insert(key.clone());
												cx.notify();
											}))
											.child(workspace_symbols::icon(
												workspace_symbols::Symbol::Close,
											)),
									),
							)
							.child(
								div()
									.text_size(px(11.))
									.line_height(px(17.))
									.text_color(rgb(WB_TEXT_MUTED))
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
			Recovery::RefreshChief => "Refresh",
			Recovery::LoginItems => "Open Login Items",
		};
		div()
			.id(SharedString::from(format!("notice-action-{index}")))
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label)
			.h(px(24.))
			.px_2()
			.rounded(px(5.))
			.flex()
			.items_center()
			.text_size(px(11.))
			.text_color(rgb(WB_BLUE))
			.cursor_pointer()
			.hover(|d| d.bg(rgba(0xffffff0c)))
			.on_click(cx.listener(move |s, event, window, cx| {
				s.status_open = false;
				match recovery {
					Recovery::RefreshChief => s.chief.update(cx, |chief, cx| chief.refresh(cx)),
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
	use std::sync::atomic::{AtomicU8, Ordering};
	static COUNT: AtomicU8 = AtomicU8::new(u8::MAX);
	if let Some(value) = value {
		stored_count_preference(Some(value));
		COUNT.store(u8::from(value), Ordering::Relaxed);
		return value;
	}
	let cached = COUNT.load(Ordering::Relaxed);
	if cached != u8::MAX {
		return cached != 0;
	}
	let value = stored_count_preference(None);
	COUNT.store(u8::from(value), Ordering::Relaxed);
	value
}

// Host-local appearance only; clearing notices never changes service state.
#[cfg(all(target_os = "macos", not(test)))]
fn stored_count_preference(value: Option<bool>) -> bool {
	use objc2::{
		msg_send,
		rc::Retained,
		runtime::{AnyClass, AnyObject},
	};
	unsafe {
		let defaults: Retained<AnyObject> =
			msg_send![AnyClass::get(c"NSUserDefaults").expect("Foundation"), standardUserDefaults];
		let key = objc2_foundation::NSString::from_str("DecodexNotificationCount");
		if let Some(value) = value {
			let _: () = msg_send![&*defaults, setBool: value, forKey: &*key];
		}
		msg_send![&*defaults, boolForKey: &*key]
	}
}
#[cfg(not(all(target_os = "macos", not(test))))]
fn stored_count_preference(value: Option<bool>) -> bool {
	value.unwrap_or(false)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn independent_failures_are_collected_and_clear_with_their_source(
		cx: &mut gpui::TestAppContext,
	) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
		shell.update(visual, |s, cx| {
			s.account_status = Some("Account change refused".into());
			s.input_status = Some("Message was not delivered".into());
			let connection = connection_presentation(s.connection);
			let notices = s.notifications(&connection, cx);
			assert!(
				notices
					.iter()
					.any(|n| n.title == "Accounts" && n.detail == "Account change refused")
			);
			assert!(
				notices
					.iter()
					.any(|n| n.title == "Message delivery"
						&& n.detail == "Message was not delivered")
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
	fn dismissed_notice_stays_hidden_until_source_recovers(cx: &mut gpui::TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
		shell.update(visual, |s, cx| {
			let connection = connection_presentation(s.connection);
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
}
