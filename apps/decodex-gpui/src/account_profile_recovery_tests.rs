//! Retained-session recovery isolation and delayed-response regression tests.

use super::*;

fn source() -> (AccountProfileController, ServerId, EntityId) {
	let controller = AccountProfileController::production();
	let server = ServerId::new("10000000-0000-4000-8000-000000000001").unwrap();
	let account = EntityId::new("20000000-0000-4000-8000-000000000001").unwrap();
	controller.bind_session(3, server.clone());
	controller.select_at_revision(account.clone(), EntityRevision(1));
	(controller, server, account)
}

fn reply(
	query: &QueryEnvelope,
	server: &ServerId,
	account: &EntityId,
	revision: u64,
) -> QueryResultEnvelope {
	QueryResultEnvelope {
		version: CURRENT_VERSION,
		server_id: server.clone(),
		query_id: query.query_id.clone(),
		payload: QueryResultPayload::AccountRecovery(AccountRecoveryResult {
			account_id: account.clone(),
			account_revision: EntityRevision(revision),
			observed_at_unix_micros: Some(100),
			state: AccountRecoveryState::Absent,
		}),
	}
}

#[tokio::test]
async fn recovery_and_profile_use_distinct_queries_and_reject_wrong_revision() {
	let (controller, server, account) = source();
	let query = controller.next_dispatch(3, &server).await;
	assert!(matches!(query.payload, QueryPayload::GetAccountRecovery { .. }));
	assert_eq!(
		controller.route_result(3, &server, &reply(&query, &server, &account, 2)),
		AccountProfileRouteOutcome::Refused
	);
	assert!(controller.snapshot().recovery.is_none());
	let profile = controller.next_dispatch(3, &server).await;
	assert!(matches!(
		profile.payload,
		QueryPayload::GetAccountProfile { include_email: false, .. }
	));
	assert_ne!(query.query_id, profile.query_id);
}

#[tokio::test]
async fn credential_revision_change_and_refresh_discard_delayed_recovery() {
	let (controller, server, account) = source();
	let old = controller.next_dispatch(3, &server).await;
	controller.select_at_revision(account.clone(), EntityRevision(2));
	let newer = controller.next_dispatch(3, &server).await;
	assert_eq!(
		controller.route_result(3, &server, &reply(&old, &server, &account, 1)),
		AccountProfileRouteOutcome::Unmatched
	);
	assert!(controller.refresh());
	assert_eq!(
		controller.route_result(3, &server, &reply(&newer, &server, &account, 2)),
		AccountProfileRouteOutcome::Unmatched
	);
	let current = controller.next_dispatch(3, &server).await;
	assert_eq!(
		controller.route_result(3, &server, &reply(&current, &server, &account, 2)),
		AccountProfileRouteOutcome::Fresh
	);
	assert_eq!(controller.snapshot().recovery.unwrap().account_revision, EntityRevision(2));
}

#[tokio::test]
async fn recovery_survives_unavailable_profile_but_not_foreign_account() {
	let (controller, server, account) = source();
	let query = controller.next_dispatch(3, &server).await;
	assert_eq!(
		controller.route_result(3, &server, &reply(&query, &server, &account, 1)),
		AccountProfileRouteOutcome::Fresh
	);
	let profile = controller.next_dispatch(3, &server).await;
	let result = QueryResultEnvelope {
		version: CURRENT_VERSION,
		server_id: server.clone(),
		query_id: profile.query_id,
		payload: QueryResultPayload::AccountProfile(AccountProfileResult::Unavailable {
			error: decodex_protocol::AccountProfileErrorDto::ProviderUnavailable,
			email: AccountProfileEmailDto::Redacted,
			plan_type: None,
		}),
	};
	assert_eq!(controller.route_result(3, &server, &result), AccountProfileRouteOutcome::Fresh);
	assert!(controller.snapshot().recovery.is_some());
	assert!(controller.refresh());
	let query = controller.next_dispatch(3, &server).await;
	let foreign = EntityId::new("20000000-0000-4000-8000-000000000002").unwrap();
	assert_eq!(
		controller.route_result(3, &server, &reply(&query, &server, &foreign, 1)),
		AccountProfileRouteOutcome::Refused
	);
}

#[tokio::test]
async fn retained_notice_expires_on_refresh_disconnect_and_wall_clock() {
	let (controller, server, account) = source();
	let query = controller.next_dispatch(3, &server).await;
	let mut result = reply(&query, &server, &account, 1);
	let QueryResultPayload::AccountRecovery(recovery) = &mut result.payload else { unreachable!() };
	recovery.observed_at_unix_micros = Some(
		i64::try_from(
			std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros(),
		)
		.unwrap(),
	);
	recovery.state =
		AccountRecoveryState::Current(Box::new(decodex_protocol::AccountRecoveryBanner {
			banner_type: decodex_protocol::WireText::new("limit").unwrap(),
			title: decodex_protocol::WireText::new("Model limit reached").unwrap(),
			description: decodex_protocol::WireText::new("Wait for the limit to reset.").unwrap(),
			reset_at: None,
			model_slug: None,
			blocked_model_slug: None,
			fallback_model_slugs: Vec::new(),
			dismissible: true,
			actions: Vec::new(),
			request_url: None,
		}));
	assert_eq!(controller.route_result(3, &server, &result), AccountProfileRouteOutcome::Fresh);
	assert!(matches!(
		controller.snapshot().recovery.unwrap().state,
		AccountRecoveryState::Current(_)
	));
	controller.session_ended(3);
	assert!(matches!(
		controller.snapshot().recovery.unwrap().state,
		AccountRecoveryState::Stale(_)
	));
	controller.bind_session(4, server.clone());
	let query = controller.next_dispatch(4, &server).await;
	result.query_id = query.query_id;
	assert_eq!(controller.route_result(4, &server, &result), AccountProfileRouteOutcome::Fresh);
	assert!(controller.refresh());
	assert!(matches!(
		controller.snapshot().recovery.unwrap().state,
		AccountRecoveryState::Stale(_)
	));
	let query = controller.next_dispatch(4, &server).await;
	result.query_id = query.query_id;
	let QueryResultPayload::AccountRecovery(recovery) = &mut result.payload else { unreachable!() };
	recovery.observed_at_unix_micros = Some(100);
	assert_eq!(controller.route_result(4, &server, &result), AccountProfileRouteOutcome::Fresh);
	assert!(matches!(
		controller.snapshot().recovery.unwrap().state,
		AccountRecoveryState::Stale(_)
	));
}

fn complete_pair(controller: &AccountProfileController, server: &ServerId, account: &EntityId) {
	let recovery = controller.try_take_dispatch(3, server).unwrap();
	assert_eq!(
		controller.route_result(3, server, &reply(&recovery, server, account, 1)),
		AccountProfileRouteOutcome::Fresh
	);
	let profile = controller.try_take_dispatch(3, server).unwrap();
	assert!(matches!(profile.payload, QueryPayload::GetAccountProfile { .. }));
	let result = QueryResultEnvelope {
		version: CURRENT_VERSION,
		server_id: server.clone(),
		query_id: profile.query_id,
		payload: QueryResultPayload::AccountProfile(AccountProfileResult::Unavailable {
			error: decodex_protocol::AccountProfileErrorDto::ProviderUnavailable,
			email: AccountProfileEmailDto::Redacted,
			plan_type: None,
		}),
	};
	assert_eq!(controller.route_result(3, server, &result), AccountProfileRouteOutcome::Fresh);
}

#[test]
fn observation_wait_is_single_across_refresh_close_and_reopen() {
	let (controller, server, account) = source();
	complete_pair(&controller, &server, &account);
	let wait = controller.try_take_dispatch(3, &server).unwrap();
	assert!(matches!(
		wait.payload,
		QueryPayload::WaitForAccountObservation { after_generation: 0, .. }
	));
	assert!(controller.try_take_dispatch(3, &server).is_none());
	for _ in 0..4 {
		assert!(controller.refresh());
		complete_pair(&controller, &server, &account);
		assert!(controller.try_take_dispatch(3, &server).is_none());
	}
	controller.close();
	assert!(controller.try_take_dispatch(3, &server).is_none());
	controller.select_at_revision(account.clone(), EntityRevision(1));
	complete_pair(&controller, &server, &account);
	assert!(controller.try_take_dispatch(3, &server).is_none());
	let result = QueryResultEnvelope {
		version: CURRENT_VERSION,
		server_id: server.clone(),
		query_id: wait.query_id,
		payload: QueryResultPayload::AccountObservation(
			decodex_protocol::AccountObservationSignal::new(7),
		),
	};
	assert_eq!(controller.route_result(3, &server, &result), AccountProfileRouteOutcome::Fresh);
	complete_pair(&controller, &server, &account);
	let wait = controller.try_take_dispatch(3, &server).unwrap();
	assert!(matches!(
		wait.payload,
		QueryPayload::WaitForAccountObservation { after_generation: 7, .. }
	));
	controller.close();
	let result = QueryResultEnvelope { query_id: wait.query_id, ..result };
	assert_eq!(controller.route_result(3, &server, &result), AccountProfileRouteOutcome::Fresh);
	assert!(controller.try_take_dispatch(3, &server).is_none());
}

#[test]
fn observation_during_profile_read_coalesces_one_followup_pair() {
	let (controller, server, account) = source();
	complete_pair(&controller, &server, &account);
	let wait = controller.try_take_dispatch(3, &server).unwrap();
	controller.refresh();
	let result = QueryResultEnvelope {
		version: CURRENT_VERSION,
		server_id: server.clone(),
		query_id: wait.query_id,
		payload: QueryResultPayload::AccountObservation(
			decodex_protocol::AccountObservationSignal::new(1),
		),
	};
	assert_eq!(controller.route_result(3, &server, &result), AccountProfileRouteOutcome::Fresh);
	complete_pair(&controller, &server, &account);
	complete_pair(&controller, &server, &account);
	assert!(matches!(
		controller.try_take_dispatch(3, &server).unwrap().payload,
		QueryPayload::WaitForAccountObservation { .. }
	));
	assert!(controller.try_take_dispatch(3, &server).is_none());
}

#[test]
fn recovery_time_copy_keeps_unavailable_timestamps_and_other_placeholders() {
	assert_eq!(
		recovery_copy("Reset at {time}; {model}", Some(0)),
		"Reset at 1970-01-01 00:00 UTC; {model}"
	);
	assert_eq!(recovery_copy("Reset at {time}", None), "Reset at {time}");
	assert_eq!(recovery_copy("Reset at {time}", Some(i64::MAX)), "Reset at {time}");
}

#[test]
fn dismissed_occurrence_stays_hidden_on_timestamp_refresh_but_changed_copy_returns() {
	let (controller, server, account) = source();
	let make = |title: &str| AccountRecoveryResult {
		account_id: account.clone(),
		account_revision: EntityRevision(1),
		observed_at_unix_micros: Some(
			i64::try_from(
				std::time::SystemTime::now()
					.duration_since(std::time::UNIX_EPOCH)
					.unwrap()
					.as_micros(),
			)
			.unwrap(),
		),
		state: AccountRecoveryState::Current(Box::new(decodex_protocol::AccountRecoveryBanner {
			banner_type: decodex_protocol::WireText::new("limit").unwrap(),
			title: decodex_protocol::WireText::new(title).unwrap(),
			description: decodex_protocol::WireText::new("Description").unwrap(),
			reset_at: None,
			model_slug: None,
			blocked_model_slug: None,
			fallback_model_slugs: Vec::new(),
			dismissible: true,
			actions: Vec::new(),
			request_url: None,
		})),
	};
	let accept = |value: AccountRecoveryResult| {
		let query = controller.try_take_dispatch(3, &server).unwrap();
		assert_eq!(
			controller.route_result(
				3,
				&server,
				&QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					query_id: query.query_id,
					payload: QueryResultPayload::AccountRecovery(value)
				}
			),
			AccountProfileRouteOutcome::Fresh
		);
	};
	let first = make("First");
	accept(first.clone());
	assert!(controller.dismiss_recovery(&first));
	assert!(controller.snapshot().recovery.is_none());
	controller.refresh();
	accept(make("First"));
	assert!(controller.snapshot().recovery.is_none());
	controller.refresh();
	accept(make("Second"));
	assert!(controller.snapshot().recovery.is_some());
	assert!(!controller.dismiss_recovery(&first));
	let current = controller.snapshot().recovery.unwrap();
	controller.session_ended(3);
	assert!(!controller.dismiss_recovery(&current));
}
