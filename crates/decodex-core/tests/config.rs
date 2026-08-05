//! XY-1306 configuration, profile, redaction, and identity adversarial coverage.

#[path = "support/test_root.rs"] mod support;

use std::{fs, sync::Arc, thread};

use getrandom as _;
#[cfg(unix)] use libc as _;
use regex as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use tempfile::NamedTempFile;
use toml as _;

use decodex_core::{
	ConfigError, DecodexClientConfig, DecodexConfig, LocalTrustPolicy, MAX_CONFIG_BYTES, PathError,
	ServerIdentity, ServerProfile,
};
use support::{SERVER_ID, TestRoot};

#[test]
fn checked_in_example_matches_the_bounded_vnext_schema() {
	let example = include_bytes!("../../../decodex.example.toml");
	let config = DecodexConfig::parse(example).expect("checked-in example parses");

	assert_eq!(config.version(), 1);
	assert!(matches!(config.active_profile(), ServerProfile::Local(_)));
	assert!(config.profiles().values().any(|profile| matches!(profile, ServerProfile::Remote(_))));
}

#[test]
fn valid_configuration_keeps_profiles_and_runtime_postgres_explicit() {
	let config =
		DecodexConfig::parse(support::valid_config().as_bytes()).expect("valid configuration");

	assert_eq!(config.version(), 1);
	assert_eq!(config.active_profile_name().as_str(), "local");
	let ServerProfile::Local(local) = config.active_profile() else {
		panic!("active profile is local")
	};

	assert_eq!(local.policy(), LocalTrustPolicy::SameUid);
	#[cfg(unix)]
	// SAFETY: `geteuid` has no arguments or failure return.
	assert_eq!(local.service_owner_uid(), Some(unsafe { libc::geteuid() }));
	assert_eq!(local.expected_server_identity(), None);

	let remote = config
		.profiles()
		.iter()
		.find(|(name, _)| name.as_str() == "remote")
		.map(|(_, profile)| profile)
		.expect("remote profile");
	let ServerProfile::Remote(remote) = remote else { panic!("explicit remote profile") };

	assert_eq!(remote.host(), "server.example.test");
	assert_eq!(remote.port(), 49_152);
	assert_eq!(remote.expected_server_identity().as_str(), SERVER_ID);

	assert_eq!(config.postgres().socket_directory().to_str(), Some("/var/run/postgresql"));
	assert_eq!(config.postgres().expected_peer_uid(), 70);
	assert_eq!(config.postgres().port(), 5_432);
	assert_eq!(config.postgres().database(), "decodex");
	assert_eq!(config.postgres().runtime().user(), "decodex_runtime");
	assert_eq!(
		config.postgres().runtime().credential_env_var(),
		Some("DECODEX_POSTGRES_RUNTIME_PASSWORD")
	);
	assert_eq!(config.cache().limits().max_entries(), 128);
}

#[test]
fn landed_portless_postgres_config_keeps_the_standard_typed_default() {
	let input = support::valid_config().replace("port = 5432\n", "");
	let config = DecodexConfig::parse(input.as_bytes()).expect("test operation must succeed");

	assert_eq!(config.postgres().port(), 5_432);
}

#[test]
fn remote_profiles_have_no_client_local_repository_path_field() {
	let input = support::valid_config().replace(
		"expected_server_identity = \"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162\"",
		"expected_server_identity = \"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162\"\nrepository_path = \"/client/must-not-be-used\"",
	);

	assert_eq!(DecodexConfig::parse(input.as_bytes()).unwrap_err(), ConfigError::Malformed);
}

#[test]
fn remote_client_projection_never_validates_postgres_host_paths() {
	let input = support::valid_config()
		.replace("active_profile = \"local\"", "active_profile = \"remote\"")
		.replace(
			"socket_directory = \"/var/run/postgresql\"",
			"socket_directory = \"../server-only\"",
		);
	let client = DecodexClientConfig::parse(input.as_bytes())
		.expect("client projection treats server-host data as opaque");
	let (_, profile) = client.selected_profile(None).expect("active remote profile");

	assert!(matches!(profile, ServerProfile::Remote(_)));
	assert_eq!(
		DecodexConfig::parse(input.as_bytes()).unwrap_err(),
		ConfigError::InvalidPostgresHostPath,
	);
}

#[test]
fn client_profile_selection_supports_active_and_explicit_names() {
	let client = DecodexClientConfig::parse(support::valid_config().as_bytes())
		.expect("test operation must succeed");
	let (active_name, active) = client.selected_profile(None).expect("test operation must succeed");
	let (remote_name, remote) =
		client.selected_profile(Some("remote")).expect("test operation must succeed");

	assert_eq!(client.version(), 1);
	assert_eq!(client.active_profile_name().as_str(), "local");
	assert_eq!(active_name.as_str(), "local");
	assert!(matches!(active, ServerProfile::Local(_)));
	assert_eq!(remote_name.as_str(), "remote");
	assert!(matches!(remote, ServerProfile::Remote(_)));
	assert_eq!(client.selected_profile(Some("missing")).unwrap_err(), ConfigError::MissingProfile,);
}

#[test]
fn local_and_remote_profile_boundaries_fail_closed() {
	let missing_owner = support::valid_config()
		.lines()
		.filter(|line| !line.starts_with("service_owner_uid = "))
		.collect::<Vec<_>>()
		.join("\n");

	assert_eq!(
		DecodexConfig::parse(missing_owner.as_bytes()).unwrap_err(),
		ConfigError::InvalidProfile,
	);

	let disabled_with_owner =
		support::valid_config().replace("policy = \"same_uid\"", "policy = \"disabled\"");

	assert_eq!(
		DecodexConfig::parse(disabled_with_owner.as_bytes()).unwrap_err(),
		ConfigError::InvalidProfile,
	);

	let remote_loopback = support::valid_config().replace("server.example.test", "127.0.0.1");

	assert_eq!(
		DecodexConfig::parse(remote_loopback.as_bytes()).unwrap_err(),
		ConfigError::InvalidProfile,
	);

	let credential_endpoint =
		support::valid_config().replace("server.example.test", "user@server.test");

	assert_eq!(
		DecodexConfig::parse(credential_endpoint.as_bytes()).unwrap_err(),
		ConfigError::InvalidProfile,
	);
}

#[test]
fn relative_or_oversized_postgres_host_paths_are_rejected() {
	let relative_socket = support::valid_config().replace(
		"socket_directory = \"/var/run/postgresql\"",
		"socket_directory = \"../postgresql\"",
	);

	assert_eq!(
		DecodexConfig::parse(relative_socket.as_bytes()).unwrap_err(),
		ConfigError::InvalidPostgresHostPath,
	);

	let oversized_socket = support::valid_config()
		.replace("/var/run/postgresql", &format!("/var/{}", "x".repeat(4 * 1_024)));

	assert_eq!(
		DecodexConfig::parse(oversized_socket.as_bytes()).unwrap_err(),
		ConfigError::InvalidPostgresHostPath,
	);
}

#[test]
fn malformed_postgres_fields_are_distinct_from_unsafe_host_paths() {
	let invalid_port = support::valid_config().replace("port = 5432", "port = 0");

	assert_eq!(
		DecodexConfig::parse(invalid_port.as_bytes()).unwrap_err(),
		ConfigError::InvalidPostgres,
	);

	let missing_peer_uid = support::valid_config().replace("expected_peer_uid = 70\n", "");

	assert_eq!(
		DecodexConfig::parse(missing_peer_uid.as_bytes()).unwrap_err(),
		ConfigError::Malformed,
	);
}

#[test]
fn accepted_postgres_host_paths_are_stored_in_one_lexically_normalized_form() {
	let input = support::valid_config().replace("/var/run/postgresql", "/var//run/./postgresql");
	let config = DecodexConfig::parse(input.as_bytes()).expect("normalizable host paths");
	assert_eq!(config.postgres().socket_directory(), std::path::Path::new("/var/run/postgresql"),);
}

#[test]
fn malformed_unknown_and_oversized_configuration_are_bounded_and_redacted() {
	let marker = "xy1306-super-sensitive-marker";
	let malformed = format!("{}\nraw_password = \"{marker}\"\n", support::valid_config());
	let error = DecodexConfig::parse(malformed.as_bytes()).unwrap_err();

	assert_eq!(error, ConfigError::Malformed);
	assert!(!format!("{error}").contains(marker));
	assert!(!format!("{error:?}").contains(marker));

	let oversized = vec![b'x'; MAX_CONFIG_BYTES + 1];

	assert_eq!(
		DecodexConfig::parse(&oversized).unwrap_err(),
		ConfigError::Oversized { limit: MAX_CONFIG_BYTES },
	);
}

#[test]
fn successful_config_debug_redacts_operator_strings() {
	let input = support::valid_config()
		.replace("server.example.test", "xy1306-secret-marker.example")
		.replace("database = \"decodex\"", "database = \"xy1306_secret_marker\"")
		.replace("user = \"decodex\"", "user = \"xy1306_secret_user\"");
	let config = DecodexConfig::parse(input.as_bytes()).expect("valid marked configuration");
	let debug = format!("{config:?}");

	assert!(!debug.contains("xy1306-secret-marker"));
	assert!(!debug.contains("xy1306_secret_marker"));
	assert!(!debug.contains("xy1306_secret_user"));
}

#[test]
fn file_loading_enforces_input_size_before_parsing() {
	let fixture = TestRoot::new();

	support::write_private_config(&fixture, &vec![b'x'; MAX_CONFIG_BYTES + 1]);

	assert!(matches!(
		DecodexConfig::load(&fixture.paths),
		Err(ConfigError::Path(PathError::Oversized { limit: MAX_CONFIG_BYTES })),
	));
}

#[cfg(unix)]
#[test]
fn group_readable_configuration_is_rejected() {
	let fixture = TestRoot::new();

	support::write_private_config(&fixture, support::valid_config().as_bytes());
	support::set_mode(&fixture.paths.config_file(), 0o640);

	assert!(matches!(
		DecodexConfig::load(&fixture.paths),
		Err(ConfigError::Path(PathError::InsecurePermissions)),
	));
}

#[test]
fn stable_server_identity_is_standard_atomic_and_concurrent() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	let paths = Arc::new(fixture.paths.clone());
	let mut workers = Vec::new();

	for _ in 0..16 {
		let paths = Arc::clone(&paths);

		workers.push(thread::spawn(move || {
			ServerIdentity::load_or_create(&paths).expect("stable identity")
		}));
	}

	let identities = workers
		.into_iter()
		.map(|worker| worker.join().expect("identity worker"))
		.collect::<Vec<_>>();

	assert!(identities.windows(2).all(|pair| pair[0] == pair[1]));

	let identity = &identities[0];

	assert_eq!(identity.as_str().len(), 36);
	assert_eq!(identity.as_str().as_bytes()[14], b'4');
	assert!(matches!(identity.as_str().as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
	assert_eq!(ServerIdentity::load(&paths).expect("identity readback"), identity.clone());

	let names = support::private_file_names(&paths.server_dir());

	assert_eq!(names, vec![paths.server_identity_file()]);
}

#[cfg(unix)]
#[test]
fn identity_permissions_and_symlinks_fail_closed() {
	let fixture = TestRoot::new();
	let identity = ServerIdentity::load_or_create(&fixture.paths).expect("stable identity");

	support::set_mode(&fixture.paths.server_identity_file(), 0o644);

	assert!(matches!(
		ServerIdentity::load(&fixture.paths),
		Err(ConfigError::Path(PathError::InsecurePermissions)),
	));

	fs::remove_file(fixture.paths.server_identity_file()).expect("remove identity");

	let outside = NamedTempFile::new().expect("outside identity");

	std::os::unix::fs::symlink(outside.path(), fixture.paths.server_identity_file())
		.expect("identity symlink");

	assert!(matches!(
		ServerIdentity::load_or_create(&fixture.paths),
		Err(ConfigError::Path(PathError::Symlink)),
	));
	assert_eq!(identity.as_str().len(), 36);
}

#[test]
fn malformed_identity_errors_never_echo_file_contents() {
	let marker = "xy1306-sensitive-identity-marker";
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	support::write_private(&fixture.paths.server_identity_file(), marker.as_bytes());

	let error = ServerIdentity::load(&fixture.paths).unwrap_err();

	assert_eq!(error, ConfigError::InvalidServerIdentity);
	assert!(!format!("{error}").contains(marker));
	assert!(!format!("{error:?}").contains(marker));
}

#[test]
fn identity_file_accepts_only_canonical_text_and_one_optional_newline() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	support::write_private(
		&fixture.paths.server_identity_file(),
		format!("{SERVER_ID}\n\n").as_bytes(),
	);

	assert_eq!(
		ServerIdentity::load(&fixture.paths).unwrap_err(),
		ConfigError::InvalidServerIdentity,
	);
}
