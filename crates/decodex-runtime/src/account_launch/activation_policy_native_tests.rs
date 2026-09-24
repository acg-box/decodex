//! Isolated installed-native control launch with production executable attestation.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use decodex_core::{
	AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
};

fn attested_profile(binary: &OsStr, home: &Path) -> AttestedAppServerProfile {
	let (program, executable, digest) =
		resolve_executable(binary).expect("explicit native executable");
	let mut command =
		AppServerCommand::production_from_resolved(program, executable, digest, home.into());
	command.attested_code_identity = Some(
		AttestedCodeIdentity::capture(&command.executable.execution_path(), &command.program)
			.expect("native code identity"),
	);
	validated_working_directory(&command).expect("isolated control directory");
	let capability =
		ExactBuildLaunchCapability::attest_profile(&command).expect("production launch capability");
	let codex_home = home.join(".codex");
	let (build, generated, guard) =
		attest_executable_for_home(&command, &codex_home, Duration::from_secs(20), None)
			.expect("native schema and executable attestation");
	assert!(guard.is_none());
	AttestedAppServerProfile { command, build, generated, capability }
}

fn control_child(binary: &OsStr, home: &Path) -> AttestedProcessChild {
	let profile = attested_profile(binary, home);
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

struct SyntheticVault(AccountId);
impl CredentialVault for SyntheticVault {
	fn project(
		&self,
		account_id: &AccountId,
		projection: &mut CredentialProjection<'_>,
	) -> Result<AccountIdentity, CredentialVaultError> {
		assert_eq!(account_id, &self.0);
		let claims = serde_json::json!({"email":"fixture@example.test","exp":4102444800_u64,
			"https://api.openai.com/auth":{"chatgpt_account_id":"workspace-fixture",
			"chatgpt_user_id":"user-fixture","chatgpt_plan_type":"team"}});
		let token = format!(
			"{}.{}.fixture-signature",
			URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
			URL_SAFE_NO_PAD.encode(claims.to_string())
		);
		projection.authenticate_chatgpt(&token, "workspace-fixture", Some("team"))?;
		Ok(AccountIdentity::from_observation("chatgpt", Some("fixture@example.test"), true))
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated attested native policy discovery"]
async fn installed_native_activation_policy_uses_selected_workspace_and_fails_closed() {
	use std::sync::atomic::AtomicUsize;
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
		tokio::task::spawn_blocking(move || {
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
					.request(&reqwest::Client::new())
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
	mode: Arc<std::sync::atomic::AtomicUsize>,
	reads: Arc<std::sync::atomic::AtomicUsize>,
) {
	use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
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
