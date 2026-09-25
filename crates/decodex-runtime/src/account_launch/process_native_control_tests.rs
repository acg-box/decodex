//! Isolated installed-native control launch with production executable attestation.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use decodex_core::{
	AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
};

pub(crate) fn attested_profile(binary: &OsStr, home: &Path) -> AttestedAppServerProfile {
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

pub(crate) fn initialized_control_child(binary: &OsStr, home: &Path) -> AttestedProcessChild {
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
	let mut child = launch.spawn().expect("attested native child");
	child
		.initialize_ordinary_turns(&SyntheticVault(account))
		.expect("native credential projection and identity check");
	child
}

pub(crate) fn read_native_account(child: &mut AttestedProcessChild) -> serde_json::Value {
	child
		.process
		.request(ReadOnlyMethod::AccountRead, &serde_json::json!({}), Duration::from_secs(15))
		.expect("account routing through the attested process")
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
