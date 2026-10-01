//! Hydrate an explicitly selected PAT with the native ChatGPT whoami authority.

use std::{mem, time::Duration};

use reqwest::{Client, redirect::Policy, retry};
use serde::Deserialize;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{
	account_import::{CredentialImportError, CredentialSource, ImportedCredential},
	host_credentials::CredentialSecretBundle,
};
use decodex_core::{AccountProvider, ProviderIdentity};

const WHOAMI_ENDPOINT: &str = "https://auth.openai.com/api/accounts/v1/user-auth-credential/whoami";
const MAX_METADATA_BYTES: usize = 64 * 1_024;

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct Metadata {
	email: Option<String>,
	chatgpt_user_id: String,
	chatgpt_account_id: String,
	chatgpt_plan_type: String,
	chatgpt_account_is_fedramp: bool,
}

pub(super) async fn resolve_import(
	source: Result<CredentialSource, CredentialImportError>,
) -> Result<ImportedCredential, CredentialImportError> {
	match source? {
		CredentialSource::Oauth(imported) => Ok(*imported),
		CredentialSource::PersonalAccessToken(token) => hydrate(token, WHOAMI_ENDPOINT).await,
	}
}

async fn hydrate(
	mut token: Zeroizing<String>,
	endpoint: &str,
) -> Result<ImportedCredential, CredentialImportError> {
	let client = Client::builder()
		.connect_timeout(Duration::from_secs(5))
		.timeout(Duration::from_secs(10))
		.redirect(Policy::none())
		.retry(retry::never())
		.user_agent("decodex")
		.build()
		.map_err(|_| CredentialImportError::Unavailable)?;
	let mut response = client
		.get(endpoint)
		.bearer_auth(token.as_str())
		.send()
		.await
		.map_err(|_| CredentialImportError::Unavailable)?;

	if !response.status().is_success() {
		return Err(if matches!(response.status().as_u16(), 401 | 403) {
			CredentialImportError::InvalidCredential
		} else {
			CredentialImportError::Unavailable
		});
	}

	let mut bytes = Zeroizing::new(Vec::new());

	while let Some(chunk) =
		response.chunk().await.map_err(|_| CredentialImportError::Unavailable)?
	{
		if chunk.len() > MAX_METADATA_BYTES.saturating_sub(bytes.len()) {
			return Err(CredentialImportError::InvalidCredential);
		}

		bytes.extend_from_slice(&chunk);
	}

	let mut metadata: Metadata =
		serde_json::from_slice(&bytes).map_err(|_| CredentialImportError::InvalidCredential)?;

	if metadata.chatgpt_account_is_fedramp
		|| metadata.chatgpt_plan_type.is_empty()
		|| metadata.chatgpt_plan_type.len() > 64
		|| metadata.chatgpt_plan_type.chars().any(char::is_control)
	{
		return Err(CredentialImportError::InvalidCredential);
	}

	let provider = ProviderIdentity::new(
		AccountProvider::Chatgpt,
		mem::take(&mut metadata.chatgpt_account_id),
	)
	.map_err(|_| CredentialImportError::InvalidCredential)?;
	let bundle = CredentialSecretBundle::personal_access_token(
		mem::take(&mut *token),
		mem::take(&mut metadata.chatgpt_user_id),
		Some(mem::take(&mut metadata.chatgpt_plan_type)),
		metadata.email.take(),
	)
	.map_err(|_| CredentialImportError::InvalidCredential)?;

	Ok(ImportedCredential { provider, bundle })
}

#[cfg(test)]
mod tests {
	use std::{
		io::{Read as _, Write as _},
		net::TcpListener,
		thread,
	};

	use crate::account_service::personal_access_token::{self, Duration, Zeroizing};

	fn server(status: &str, body: &str) -> (String, thread::JoinHandle<String>) {
		let listener = TcpListener::bind("127.0.0.1:0").unwrap();
		let url = format!("http://{}/whoami", listener.local_addr().unwrap());
		let response = format!(
			"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nLocation: /must-not-follow\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
			body.len()
		);
		let handle = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();

			stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

			let mut request = Vec::new();
			let mut buffer = [0; 1_024];

			while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
				let read = stream.read(&mut buffer).unwrap();

				assert_ne!(read, 0);

				request.extend_from_slice(&buffer[..read]);
			}

			stream.write_all(response.as_bytes()).unwrap();

			String::from_utf8(request).unwrap()
		});

		(url, handle)
	}

	fn metadata() -> serde_json::Value {
		serde_json::json!({"email":"pat@example.invalid","chatgpt_user_id":"pat-user","chatgpt_account_id":"pat-account","chatgpt_plan_type":"pro","chatgpt_account_is_fedramp":false})
	}

	#[tokio::test]
	async fn pat_identity_comes_from_whoami_without_jwt_or_refresh_material() {
		let (url, request) = server("200 OK", &metadata().to_string());
		let credential =
			personal_access_token::hydrate(Zeroizing::new("synthetic-pat".into()), &url)
				.await
				.unwrap();

		assert_eq!(credential.provider.account_id(), "pat-account");
		assert_eq!(credential.bundle.personal_access_token_user_id(), Some("pat-user"));
		assert_eq!(credential.bundle.provider_email(), Some("pat@example.invalid"));
		assert_eq!(credential.bundle.refresh_token(), None);
		assert_eq!(credential.bundle.access_token_expires_at_unix_micros(), None);

		let request = request.join().unwrap().to_ascii_lowercase();

		assert!(request.starts_with("get /whoami "));
		assert!(request.contains("authorization: bearer synthetic-pat\r\n"));
	}

	#[tokio::test]
	async fn pat_hydration_rejects_redirects_invalid_identity_and_unsupported_region() {
		let mut missing_user = metadata();

		missing_user.as_object_mut().unwrap().remove("chatgpt_user_id");

		let mut unsupported_region = metadata();

		unsupported_region["chatgpt_account_is_fedramp"] = true.into();

		for (status, body) in [
			("302 Found", metadata().to_string()),
			("401 Unauthorized", "{}".into()),
			("200 OK", missing_user.to_string()),
			("200 OK", unsupported_region.to_string()),
		] {
			let (url, request) = server(status, &body);

			assert!(
				personal_access_token::hydrate(Zeroizing::new("synthetic-pat".into()), &url)
					.await
					.is_err()
			);

			request.join().unwrap();
		}
	}
}
