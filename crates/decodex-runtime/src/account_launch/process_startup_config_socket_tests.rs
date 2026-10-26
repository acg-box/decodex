//! Configuration diagnostics remain available when the conversation runtime is unavailable.
use std::{
	fs::{self, Permissions},
	os::unix::fs::PermissionsExt as _,
	path::Path,
};

use crate::{
	ProtocolServer, ServerConfig,
	application::{ProductStore, ProductStoreUnavailableReason, ServiceApplication},
	conversation::ConversationCapability,
};
use decodex_core::{DecodexRoot, LocalTrustPolicy};
use decodex_protocol::{
	CURRENT_VERSION, ClientProfile, ConversationUnavailableReason, ConversationWorkingDirectory,
	DoctorCheck, DoctorClient, DoctorComponent, DoctorIssue, DoctorReport, DoctorStatus,
	LocalTransportAuthority, NativeStartupConfiguration, ServerId,
};

pub(super) async fn check(home: &Path, directory: &Path) -> NativeStartupConfiguration {
	let root = DecodexRoot::new(home.join("diagnostic-service")).expect("temporary service root");

	root.paths().ensure_layout().expect("private service layout");

	let server_id = ServerId::new("20000000-0000-4000-8000-000000000001").expect("server identity");
	let uid = unsafe { libc::geteuid() };
	let config = root.as_path().join("config.toml");

	fs::write(&config,format!("version=1\nactive_profile='local'\n[cache]\n[profiles.local]\nkind='local'\npolicy='same_uid'\nservice_owner_uid={uid}\nexpected_server_identity='{}'\n",server_id.as_str())).expect("local client profile");
	fs::set_permissions(config, Permissions::from_mode(0o600)).expect("private profile");

	let profile = ClientProfile::load(root.as_path(), None).expect("local profile");
	let doctor = DoctorReport::new(
		server_id.clone(),
		CURRENT_VERSION,
		DoctorComponent::ALL
			.into_iter()
			.map(|component| {
				DoctorCheck::new(component, DoctorStatus::Unavailable(DoctorIssue::NotProbed))
			})
			.collect(),
	)
	.expect("unprobed health");
	let app = ServiceApplication::new(
		ProductStore::Unavailable(ProductStoreUnavailableReason::Configuration),
		None,
		None,
		None,
		ConversationCapability::Unavailable(ConversationUnavailableReason::AppServerProfile),
		doctor,
	);
	let authority =
		LocalTransportAuthority::new(root.paths(), LocalTrustPolicy::SameUid, Some(uid))
			.expect("same-UID authority");
	let client = DoctorClient::new(profile);
	let server = ProtocolServer::new(server_id, app, ServerConfig::default())
		.bind(authority)
		.await
		.expect("local service");
	let before = client.query().await.expect("initial service health");
	let result = client
		.startup_configuration(
			ConversationWorkingDirectory::new(directory.to_str().expect("directory"))
				.expect("exact directory"),
		)
		.await;
	let after = client.query().await.expect("service health after inspection");
	let mut server = server;

	assert!(server.shutdown().await.expect("owned service shutdown").is_success());
	assert_eq!(before, after, "configuration success cannot promote service health");

	result.expect("native configuration query with unavailable conversation runtime")
}
