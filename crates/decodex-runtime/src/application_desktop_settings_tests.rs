//! Exercise the settings command owner without a native process or provider.
use super::*;
use decodex_protocol::{
	CURRENT_VERSION, ClientCommandId, CorrelationId, DoctorComponent, DoctorIssue, DoctorStatus,
	EntityRevision, IdempotencyKey,
};

#[tokio::test]
async fn automatic_recap_preference_uses_existing_command_and_readback_owner() {
	let directory = tempfile::tempdir().unwrap();
	let root = decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
	let store = SqliteStore::open(&root.paths()).unwrap();
	let doctor = DoctorReport::new(
		decodex_protocol::ServerId::new("settings-test").unwrap(),
		CURRENT_VERSION,
		DoctorComponent::ALL
			.into_iter()
			.map(|component| {
				DoctorCheck::new(component, DoctorStatus::Unavailable(DoctorIssue::NotProbed))
			})
			.collect(),
	)
	.unwrap();
	let app = ServiceApplication::new(
		ProductStore::Available(store),
		None,
		None,
		CodexAdapter::unavailable(),
		None,
		ConversationCapability::Unavailable(
			decodex_protocol::ConversationUnavailableReason::AppServerProfile,
		),
		doctor,
	);
	let mut command = CommandEnvelope {
		version: CURRENT_VERSION,
		client_command_id: ClientCommandId::new("recap-preference").unwrap(),
		idempotency_key: IdempotencyKey::new("recap-preference").unwrap(),
		expected_revision: Some(EntityRevision(1)),
		correlation_id: CorrelationId::new("recap-preference").unwrap(),
		causation_id: None,
		payload: CommandPayload::SetDesktopSettings {
			show_in_menu_bar: true,
			auto_activate_quota: None,
			auto_recap: Some(true),
		},
	};
	let published = app.execute_desktop_settings(&command).await.unwrap();
	let ResultPayload::DesktopSettingsChanged { settings } = published.result else {
		panic!("settings result")
	};
	assert!(settings.auto_recap && settings.auto_activate_quota);
	assert_eq!(app.desktop_settings().await, DesktopSettingsResult::Available(settings));
	assert!(
		matches!(published.event, EventPayload::DesktopSettingsChanged { settings: event } if event == settings)
	);
	assert!(app.execute_desktop_settings(&command).await.is_err(), "stale revision must fail");
	command.expected_revision = Some(settings.revision);
	command.payload = CommandPayload::SetDesktopSettings {
		show_in_menu_bar: false,
		auto_activate_quota: Some(false),
		auto_recap: None,
	};
	app.execute_desktop_settings(&command).await.unwrap();
	let DesktopSettingsResult::Available(retained) = app.desktop_settings().await else {
		panic!("settings readback")
	};
	assert!(retained.auto_recap && !retained.auto_activate_quota && !retained.show_in_menu_bar);
}
