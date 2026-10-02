//! Isolated installed-native control launch with production executable attestation.
#[cfg(target_os = "linux")] use std::os::fd::FromRawFd as _;
use std::sync::atomic::AtomicUsize;

use reqwest::Client;
use tokio::{
	io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
	task,
};

#[cfg(test)] use crate::account_launch::process::RunnerCapacity;
use crate::account_launch::process::{
	AccountBinding, AccountId, Arc, AttestedAppServerLaunch, AttestedProcessChild, Duration,
	Ordering, OsStr, Path, ProcessGenerationAccountBinding, env, fs,
	native_control_tests::{self, SyntheticVault},
	serde_json,
};
use decodex_core::{
	AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
};

fn control_child(binary: &OsStr, home: &Path) -> AttestedProcessChild {
	let profile = native_control_tests::attested_profile(binary, home);
	let callback_profile = profile.generated.account_callback_profile_sha256().to_owned();
	let codex_home = home.join(".codex");
	let account = AccountId::new("10000000-0000-4000-8000-000000000001").expect("fixture account");
	let credential = CredentialBinding {
		schema_version: CredentialStoreSchemaVersion::V1,
		version: CredentialVersion::new(1).expect("fixture version"),
		fingerprint: CredentialFingerprint::new("1".repeat(64)).expect("fixture fingerprint"),
		provider: ProviderIdentity::new(AccountProvider::Chatgpt, "workspace-fixture")
			.expect("fixture provider"),
		writer_operation_id: AccountOperationId::new("20000000-0000-4000-8000-000000000001")
			.expect("fixture operation"),
	};
	let binding = AccountBinding {
		personal_access_token: None,
		account_id: account.clone(),
		expected_codex_home: codex_home,
		process_binding: Some(
			ProcessGenerationAccountBinding::new(1, credential, callback_profile)
				.expect("account binding"),
		),
		refresh_callback: None,
	};
	let capacity = RunnerCapacity::try_with_limit(1).expect("fixture capacity");
	let launch = AttestedAppServerLaunch::bind(
		profile,
		binding,
		Duration::from_secs(15),
		capacity.reserve(account.clone(), 1).expect("account permit"),
	)
	.expect("bound native launch");

	launch.spawn().expect("attested native child")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated attested native policy discovery"]
async fn installed_native_activation_policy_uses_selected_workspace_and_fails_closed() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().expect("isolated control home");
	let directory = home.path().canonicalize().expect("canonical fixture directory");
	let codex_home = directory.join(".codex");

	fs::create_dir(&codex_home).expect("isolated native home");

	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("fixture backend");
	let address = listener.local_addr().expect("backend address");

	fs::write(codex_home.join("config.toml"), format!("chatgpt_base_url=\"http://{address}\"\ncli_auth_credentials_store=\"file\"\n[analytics]\nenabled=false\n")).expect("fixture configuration");

	let mode = Arc::new(AtomicUsize::new(0));
	let reads = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve_policy(listener, mode.clone(), reads.clone()));

	for scenario in 0..3 {
		mode.store(scenario, Ordering::Release);

		let binary = binary.clone();
		let directory = directory.clone();

		task::spawn_blocking(move || {
			let mut child = control_child(&binary, &directory);
			let id =
				AccountId::new("10000000-0000-4000-8000-000000000001").expect("fixture account");
			let result = child
				.initialize_ordinary_turns(&SyntheticVault(id))
				.and_then(|()| child.read_activation_policy());

			child.shutdown().expect("confirmed native cleanup");

			if scenario == 1 {
				assert!(result.is_err(), "invalid discovery cannot authorize activation");
			} else {
				let request = result
					.expect("native activation policy")
					.request(&Client::new())
					.build()
					.expect("routed request");

				assert_eq!(
					request.url().as_str(),
					"https://gov.example/backend-api/codex/responses"
				);
				assert_eq!(request.headers()["x-openai-account-routing-override"], "us_cr");
			}
		})
		.await
		.expect("attested fixture owner");
	}

	assert!(reads.load(Ordering::Acquire) >= 3, "each cold lookup must discover current policy");
	assert!(
		!codex_home.join("auth.json").exists(),
		"ephemeral projection must not persist credentials"
	);
	assert!(!backend.is_finished(), "backend assertions must pass");

	backend.abort();
}

async fn serve_policy(
	listener: tokio::net::TcpListener,
	mode: Arc<AtomicUsize>,
	reads: Arc<AtomicUsize>,
) {
	loop {
		let (stream, _) = listener.accept().await.expect("native fixture connection");
		let mut stream = BufReader::new(stream);
		let mut line = String::new();

		stream.read_line(&mut line).await.expect("request line");

		let route = line.contains("/accounts/check ");

		assert!(line.starts_with("GET "), "policy lookup must not send a model request: {line}");

		let mut account = None;

		loop {
			line.clear();

			assert!(stream.read_line(&mut line).await.expect("request header") > 0);

			if line == "\r\n" {
				break;
			}

			let (name, value) = line.split_once(':').expect("HTTP header");

			if name.eq_ignore_ascii_case("chatgpt-account-id") {
				account = Some(value.trim().to_owned());
			}
		}

		let body = if route {
			reads.fetch_add(1, Ordering::AcqRel);

			assert_eq!(account.as_deref(), Some("workspace-fixture"));

			let origin = if mode.load(Ordering::Acquire) == 1 {
				"https://gov.example/invalid"
			} else {
				"https://gov.example"
			};

			serde_json::json!({"default_account_id":"other", "accounts":[
				{"id":"other","workspace_backend_origin":"https://wrong.example","account_routing_override":"us"},
				{"id":"workspace-fixture","workspace_backend_origin":origin,"account_routing_override":"us_cr"}]}).to_string()
		} else {
			"{}".into()
		};

		stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.expect("native policy response");
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; attested selected-directory model defaults"]
async fn installed_attested_model_defaults_preserve_selected_directory_and_sources() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
	let home = tempfile::tempdir().expect("home");
	let directory = home.path().canonicalize().expect("directory");
	let workspace = directory.join("workspace");

	fs::create_dir_all(workspace.join(".codex")).expect("project config");
	fs::create_dir(workspace.join(".git")).expect("project root");
	fs::create_dir(directory.join(".codex")).expect("native home");

	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("backend");
	let address = listener.local_addr().expect("address");
	let backend = tokio::spawn(serve_policy(
		listener,
		Arc::new(AtomicUsize::new(0)),
		Arc::new(AtomicUsize::new(0)),
	));

	fs::write(directory.join(".codex/config.toml"),format!("model=\"global-model\"\nmodel_reasoning_effort=\"low\"\nchatgpt_base_url=\"http://{address}\"\n[projects.{}]\ntrust_level=\"trusted\"\n[analytics]\nenabled=false\n",serde_json::json!(workspace))).expect("native config");
	fs::write(
		workspace.join(".codex/config.toml"),
		"model=\"project-model\"\nmodel_reasoning_effort=\"high\"\nservice_tier=\"flex\"\n",
	)
	.expect("project defaults");

	task::spawn_blocking(move || {
		let id = AccountId::new("10000000-0000-4000-8000-000000000001").expect("account");
		let mut child = control_child(&binary, &directory);

		child.initialize_ordinary_turns(&SyntheticVault(id)).expect("initialize");

		for (path, model, effort) in
			[(&directory, "global-model", "low"), (&workspace, "project-model", "high")]
		{
			let (defaults, events) =
				child.read_ordinary_model_defaults(path.to_str().expect("directory"), false);

			child.retain_ordinary_events(events).expect("retain events");

			let defaults = defaults.expect("native defaults");

			assert_eq!(defaults.model.as_deref(), Some(model));
			assert_eq!(defaults.reasoning_effort.as_deref(), Some(effort));
		}

		let (managed, events) =
			child.read_ordinary_model_defaults(workspace.to_str().expect("directory"), true);

		child.retain_ordinary_events(events).expect("retain events");

		assert_eq!(
			managed.expect("managed defaults"),
			decodex_codex::app_server_client::NativeExecutionDefaults::default()
		);

		child.shutdown().expect("confirmed cleanup");
	})
	.await
	.expect("native owner");

	assert!(!backend.is_finished());

	backend.abort();
}
