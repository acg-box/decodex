//! Restore native configuration boundaries through the retained bridge.
use super::*;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native settings through retained bridge"]
async fn installed_native_account_settings_cross_retained_bridge_and_cold_restart() {
	use decodex_codex::app_server_client::AppLinkSettingEdit;
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().unwrap();
	assert!(!home.path().join("config.toml").exists());
	let cwd = home.path().to_str().unwrap();
	let session = NativeSession::start(&binary, home.path());
	let saved = tokio::time::timeout(Duration::from_secs(20), async {
		let before =
			session.client.app_link_settings(cwd, "calendar.app", "工作.\"link\\1").await.unwrap();
		let saved = session
			.client
			.write_app_link_setting(
				&before,
				AppLinkSettingEdit::ApprovalMode(Some("prompt".into())),
			)
			.await
			.unwrap();
		let readback =
			session.client.app_link_settings(cwd, "calendar.app", "工作.\"link\\1").await.unwrap();
		assert_eq!(readback.config_version(), saved.version);
		assert_eq!(readback.effective_mode.as_deref(), Some("prompt"));
		assert!(matches!(
			session
				.client
				.write_app_link_setting(
					&before,
					AppLinkSettingEdit::Reviewer(Some("auto_review".into()))
				)
				.await,
			Err(ClientError::Remote(_))
		));
		readback
	})
	.await
	.unwrap();
	assert!(home.path().join("config.toml").is_file());
	drop(session);
	let reopened = NativeSession::start(&binary, home.path());
	tokio::time::timeout(Duration::from_secs(20), async {
		assert!(matches!(
			reopened
				.client
				.write_app_link_setting(&saved, AppLinkSettingEdit::Reviewer(None))
				.await,
			Err(ClientError::InvalidFrame)
		));
		let cold =
			reopened.client.app_link_settings(cwd, "calendar.app", "工作.\"link\\1").await.unwrap();
		assert_eq!(cold.user_mode.as_deref(), Some("prompt"));
		let cleared = reopened
			.client
			.write_app_link_setting(&cold, AppLinkSettingEdit::ApprovalMode(None))
			.await
			.unwrap();
		let readback =
			reopened.client.app_link_settings(cwd, "calendar.app", "工作.\"link\\1").await.unwrap();
		assert_eq!(readback.config_version(), cleared.version);
		assert_eq!(readback.user_mode, None);
	})
	.await
	.unwrap();
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native login policy"]
async fn installed_native_login_methods_follow_running_policy_until_restart() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let config = home.path().join("config.toml");
	std::fs::write(&config, "cli_auth_credentials_store = 'file'\n").unwrap();
	let unrestricted = NativeSession::start(&binary, home.path());
	let requirements =
		unrestricted.client.request("configRequirements/read", json!({})).await.unwrap();
	assert!(requirements["requirements"].is_null());
	drop(unrestricted);
	for (current, changed) in [("api", "chatgpt"), ("chatgpt", "api")] {
		std::fs::write(
			&config,
			format!("cli_auth_credentials_store = 'file'\nforced_login_method = '{current}'\n"),
		)
		.unwrap();
		let session = NativeSession::start(&binary, home.path());
		for rewrite in [false, true] {
			if rewrite {
				std::fs::write(
					&config,
					format!(
						"cli_auth_credentials_store = 'file'\nforced_login_method = '{changed}'\n"
					),
				)
				.unwrap();
			}
			let requirements =
				session.client.request("configRequirements/read", json!({})).await.unwrap();
			assert_eq!(requirements["requirements"]["allowedLoginMethods"], json!([current]));
		}
		drop(session);
		let restarted = NativeSession::start(&binary, home.path());
		let requirements =
			restarted.client.request("configRequirements/read", json!({})).await.unwrap();
		assert_eq!(requirements["requirements"]["allowedLoginMethods"], json!([changed]));
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated initial thread preference"]
async fn installed_native_daybreak_preference_is_staged_without_selecting_access() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	std::fs::write(home.path().join("config.toml"), "cli_auth_credentials_store = 'file'\n")
		.unwrap();
	let session = NativeSession::start(&binary, home.path());
	tokio::time::timeout(Duration::from_secs(30), async {
		for choice in [Some(true), Some(false), None] {
			let mut params = json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"});
			if let Some(choice) = choice {
				params["daybreakEnabled"] = json!(choice);
			}
			let started = session.client.thread_start(params.clone()).await.unwrap();
			assert_eq!(started["thread"]["daybreakEnabled"], json!(choice));
			let read = session.client.request("thread/read", json!({"threadId":started["thread"]["id"],"includeTurns":false})).await.unwrap();
			assert_eq!(read["thread"]["daybreakEnabled"], json!(choice));
			if choice.is_some() {
				params["ephemeral"] = json!(true);
				let result = session.client.thread_start(params).await;
				assert!(matches!(&result, Err(ClientError::Remote(error)) if error.message.contains("daybreakEnabled is not supported for ephemeral threads")), "{result:?}");
			}
		}
	}).await.unwrap();
}
