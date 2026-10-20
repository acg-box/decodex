#[path = "process_startup_config_socket_tests.rs"] mod socket;
// Preserve useful native requirements and locations without retaining raw configuration errors.
use super::{
	NativeStartupConfigStatus, NativeStartupConfiguration, NativeStartupLoginMethod,
	inspect_native_startup_configuration, project,
};
use serde_json::json;

#[test]
fn startup_projection_keeps_policy_and_location_without_raw_error_details() {
	let loaded = project(&json!({"appServerVersion":"0.0.0","configRequirements":{"requirements":{"allowedLoginMethods":["chatgpt","api"]}}}), true).expect("native requirements");
	assert!(
		matches!(loaded, NativeStartupConfigStatus::Loaded { login_methods:Some(methods),requirements_present:true,.. } if methods==vec![NativeStartupLoginMethod::Chatgpt,NativeStartupLoginMethod::Api])
	);
	let status = project(&json!({"error":{"code":-32600,"message":"DO_NOT_COPY_PRIVATE_MESSAGE","data":{"reason":"configLoad","detail":"DO_NOT_COPY_PRIVATE_DETAIL","filePath":"/tmp/config.toml","line":2,"column":8,"credentials":"DO_NOT_COPY_EXTRA_FIELDS"}}}), false).expect("native error location");
	let report = NativeStartupConfiguration {
		directory: Some("/tmp/project".into()),
		codex_sha256: None,
		status,
	};
	let text = report.report_text();
	assert!(text.contains("/tmp/config.toml") && text.contains("Line: 2"));
	assert!(!text.contains("DO_NOT_COPY") && !format!("{report:?}").contains("DO_NOT_COPY"));
	let cloud = project(&json!({"error":{"code":-32600,"message":"private","data":{"reason":"cloudConfigBundle","action":"relogin","detail":"private-account-state"}}}), false).expect("cloud failure");
	assert!(matches!(cloud, NativeStartupConfigStatus::Rejected { cloud_policy: true, .. }));
}

#[test]
fn startup_projection_rejects_malformed_or_conflicting_success_claims() {
	for value in [
		json!({}),
		json!({"appServerVersion":"private\nvalue","configRequirements":{"requirements":null}}),
		json!({"appServerVersion":"0.0.0","configRequirements":{"requirements":{"allowedLoginMethods":["unrecognized"]}}}),
		json!({"appServerVersion":"0.0.0","configRequirements":{"requirements":false}}),
	] {
		assert!(project(&value, true).is_none());
	}
	let loaded = json!({"appServerVersion":"0.0.0","configRequirements":{"requirements":null}});
	assert!(project(&loaded, false).is_none());
	let error = json!({"error":{"code":-32600,"message":"private"}});
	assert!(project(&error, true).is_none());
	let redacted=project(&json!({"error":{"code":-32600,"message":"private","data":{"filePath":"/tmp/Authorization: Bearer private/config.toml"}}}),false).expect("safe error");
	assert!(matches!(redacted, NativeStartupConfigStatus::Rejected { location: None, .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated managed-runtime startup_config.py fixture"]
async fn installed_native_startup_configuration_preserves_auth_config_and_locked_history() {
	use std::{env, fs, path::PathBuf};
	let home = PathBuf::from(env::var_os("DECODEX_TEST_ACCOUNT_HOME").expect("isolated home"));
	assert_eq!(PathBuf::from(env::var_os("HOME").expect("HOME")), home);
	assert_eq!(
		fs::read_to_string(home.join(".decodex-startup-config-fixture")).expect("fixture marker"),
		"isolated-native\n"
	);
	let native = home.join(".codex");
	assert!(!native.exists());
	fs::create_dir(&native).expect("native home");
	let config = native.join("config.toml");
	let auth = native.join("auth.json");
	let saved_auth = b"{\"OPENAI_API_KEY\":\"fixture-only-key\"}";
	let saved_config = "forced_login_method='api'\ncli_auth_credentials_store='file'\n[analytics]\nenabled=false\n";
	fs::write(&config, saved_config).expect("local config");
	fs::write(&auth, saved_auth).expect("fixture native auth");
	let report = inspect_native_startup_configuration(&home);
	assert!(
		matches!(&report.status,NativeStartupConfigStatus::Loaded { login_methods:Some(methods),.. } if methods==&vec![NativeStartupLoginMethod::Api]),
		"{report:?}"
	);
	use sha2::{Digest as _, Sha256};
	let expected = Sha256::digest(
		fs::read(env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit qualified CLI"))
			.expect("CLI bytes"),
	)
	.iter()
	.map(|b| format!("{b:02x}"))
	.collect::<String>();
	assert_eq!(report.codex_sha256.as_ref(), Some(&expected), "exact qualified CLI");

	assert_eq!(fs::read(&auth).expect("auth readback"), saved_auth);
	assert_eq!(fs::read_to_string(&config).expect("config readback"), saved_config);
	assert!(!native.join("sessions").exists(), "configuration checks cannot start a thread");
	let public = socket::check(&home, &home).await;
	assert_eq!(public, report, "the public service must return the same qualified native snapshot");

	fs::remove_file(&auth).expect("remove only synthetic auth");
	let folder = home.join("project");
	fs::create_dir(&folder).expect("project folder");
	fs::create_dir(folder.join(".git")).expect("project boundary");
	fs::create_dir(folder.join(".codex")).expect("project config folder");
	let project_config = folder.join(".codex/config.toml");
	fs::write(&project_config, "forced_login_method='chatgpt'\n").expect("project login policy");
	let trusted = format!(
		"{saved_config}\n[projects.{}]\ntrust_level='trusted'\n",
		serde_json::to_string(&folder).expect("project key")
	);
	fs::write(&config, &trusted).expect("trust isolated project");
	let report = inspect_native_startup_configuration(&folder);
	assert!(
		matches!(&report.status,NativeStartupConfigStatus::Loaded { login_methods:Some(methods),.. } if methods==&vec![NativeStartupLoginMethod::Chatgpt]),
		"{report:?}"
	);
	assert_eq!(fs::read_to_string(&config).expect("shared config"), trusted);
	assert_eq!(
		fs::read_to_string(&project_config).expect("project config"),
		"forced_login_method='chatgpt'\n"
	);
	assert!(!auth.exists(), "a check cannot sign in");
	fs::write(&config, "[broken\nPRIVATE_CONFIGURATION_VALUE").expect("invalid config");
	let report = inspect_native_startup_configuration(&home);
	assert!(
		matches!(&report.status,NativeStartupConfigStatus::Rejected { location:Some(location),cloud_policy:false } if location.file==config.to_str().expect("config path") && location.line==Some(1)),
		"{report:?}"
	);
	assert!(!report.report_text().contains("PRIVATE_CONFIGURATION_VALUE"));
	fs::write(&config, saved_config).expect("restore fixture config");
	fs::write(&auth, saved_auth).expect("restore fixture auth");
	let history = native.join("state_5.sqlite");
	let db = rusqlite::Connection::open(&history).expect("native history fixture");
	db.execute_batch(
		"PRAGMA journal_mode=DELETE; CREATE TABLE synthetic_history(id INTEGER); BEGIN EXCLUSIVE;",
	)
	.expect("lock native history");
	let before = fs::read(&history).expect("history bytes");
	let report = inspect_native_startup_configuration(&home);
	assert!(matches!(report.status, NativeStartupConfigStatus::Loaded { .. }), "{report:?}");
	assert_eq!(fs::read(&history).expect("unchanged history"), before);
	assert_eq!(fs::read(&auth).expect("unchanged auth"), saved_auth);
	assert_eq!(fs::read_to_string(&config).expect("unchanged config"), saved_config);
	db.execute_batch("ROLLBACK").expect("release history fixture");
}
