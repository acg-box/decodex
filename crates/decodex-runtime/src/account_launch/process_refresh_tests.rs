//! Exercise the production refresh response owner with synthetic credentials only.
use super::*;
use decodex_core::{
	AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
};
use serde_json::{Value, json};
use std::sync::{
	Mutex,
	atomic::{AtomicUsize, Ordering},
};

const PROVIDER: &str = "123e4567-e89b-42d3-a456-426614174011";
struct Refresh {
	token: String,
	provider: String,
	fail: bool,
	calls: Arc<AtomicUsize>,
}
impl AccountRefreshCallback for Refresh {
	fn refresh(
		&self,
		account: &AccountId,
		binding: &ProcessGenerationAccountBinding,
		reason: &str,
		previous: Option<&str>,
	) -> Result<ChatgptRefreshProjection, CredentialVaultError> {
		assert_eq!(account.as_str(), "10000000-0000-4000-8000-000000000001");
		assert_eq!(binding.credential.provider.account_id(), PROVIDER);
		assert_eq!(reason, "unauthorized");
		assert!(previous.is_none_or(|p| p == PROVIDER));
		self.calls.fetch_add(1, Ordering::SeqCst);
		if self.fail {
			return Err(CredentialVaultError::Unavailable);
		}
		ChatgptRefreshProjection::new(self.token.clone(), self.provider.clone(), None)
	}
}
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
	fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
		self.0.lock().expect("capture").extend_from_slice(bytes);
		Ok(bytes.len())
	}

	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}
fn binding(token: &str, provider: &str, fail: bool) -> (AccountBinding, Arc<AtomicUsize>) {
	let calls = Arc::new(AtomicUsize::new(0));
	let credential = CredentialBinding {
		schema_version: CredentialStoreSchemaVersion::V1,
		version: CredentialVersion::new(1).expect("version"),
		fingerprint: CredentialFingerprint::new("1".repeat(64)).expect("fingerprint"),
		provider: ProviderIdentity::new(AccountProvider::Chatgpt, PROVIDER).expect("provider"),
		writer_operation_id: AccountOperationId::new("20000000-0000-4000-8000-000000000001")
			.expect("operation"),
	};
	let binding = AccountBinding {
		account_id: AccountId::new("10000000-0000-4000-8000-000000000001").expect("account"),
		expected_codex_home: PathBuf::from("/unused-fixture"),
		process_binding: Some(
			ProcessGenerationAccountBinding::new(1, credential, "a".repeat(64))
				.expect("process binding"),
		),
		refresh_callback: Some(Arc::new(Refresh {
			token: token.into(),
			provider: provider.into(),
			fail,
			calls: calls.clone(),
		})),
	};
	(binding, calls)
}
fn handle(
	binding: &AccountBinding,
	id: u64,
	method: &str,
	request: &Value,
) -> Result<Vec<u8>, ProbeError> {
	let capture = Capture::default();
	let mut writer: Box<dyn Write + Send> = Box::new(capture.clone());
	SupervisedProcess::service_inbound_request(
		binding,
		&mut writer,
		id,
		method,
		&serde_json::to_vec(request).expect("request"),
	)?;
	let bytes = capture.0.lock().expect("capture").clone();
	Ok(bytes)
}
#[test]
fn refresh_owner_preserves_optional_fields_and_rejects_wrong_identity_before_reply() {
	const METHOD: &str = "account/chatgptAuthTokens/refresh";
	for (provider, fail) in [(PROVIDER, false), (PROVIDER, true), ("other-account", false)] {
		for previous in [None, Some(json!(null)), Some(json!(PROVIDER))] {
			let (binding, calls) = binding("synthetic-nonsecret-token", provider, fail);
			let mut request = json!({"id":17,"method":METHOD,"params":{"reason":"unauthorized"}});
			if let Some(previous) = previous {
				request["params"]["previousAccountId"] = previous;
			}
			let result = handle(&binding, 17, METHOD, &request);
			assert_eq!(calls.load(Ordering::SeqCst), 1);
			if provider != PROVIDER {
				assert!(result.is_err());
				continue;
			}
			let response: Value = serde_json::from_slice(&result.unwrap()).unwrap();
			assert_eq!(response["id"], 17);
			if fail {
				assert_eq!(response["error"]["code"], -32001);
				assert!(response.get("result").is_none());
			} else {
				assert_eq!(response["result"]["accessToken"], "synthetic-nonsecret-token");
				assert_eq!(response["result"]["chatgptAccountId"], PROVIDER);
				assert!(response["result"]["chatgptPlanType"].is_null());
			}
		}
	}
	for mutation in ["id", "method", "reason", "empty_previous", "bad_previous"] {
		let (binding, calls) = binding("synthetic-nonsecret-token", PROVIDER, false);
		let mut request = json!({"id":17,"method":METHOD,"params":{"reason":"unauthorized"}});
		match mutation {
			"id" => request["id"] = json!(18),
			"method" => request["method"] = json!("other"),
			"reason" => request["params"]["reason"] = json!("other"),
			"empty_previous" => request["params"]["previousAccountId"] = json!(""),
			_ => request["params"]["previousAccountId"] = json!(false),
		}
		assert!(handle(&binding, 17, METHOD, &request).is_err());
		assert_eq!(calls.load(Ordering::SeqCst), 0);
	}
}

#[path = "process_refresh_native_tests.rs"] mod native;
