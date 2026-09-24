//! Native policy for the direct, tool-free quota activation request.
//! Source: openai/codex 595cc91e8cbb1c2ca822d0311dcf12709410c582,
//! account_processor/workspace_routing.rs and model-provider/workspace_routing.rs.

use std::time::Duration;

use decodex_core::{AccountId, ProcessGenerationAccountBinding};
use reqwest::{RequestBuilder, Url};
use serde_json::Value;

use super::{
	AttestedAppServerLaunch, AttestedAppServerProfile, RunnerCapacity,
	process::{
		AccountBinding, AccountIdentity, CredentialProjection, CredentialVault,
		CredentialVaultError,
	},
};
use crate::account_service::AccountApiCredential;

const POLICY_TIMEOUT: Duration = Duration::from_secs(20);

/// An exact native workspace route, consumed only by the model activation request.
pub(crate) struct ActivationPolicy {
	responses_url: Url,
	routing: Option<&'static str>,
	residency: Option<&'static str>,
}

impl ActivationPolicy {
	pub(super) fn decode(
		account: &Value,
		config: &Value,
		expected_account: &str,
	) -> Result<Self, ()> {
		let route = account.get("workspaceRouting").and_then(Value::as_object).ok_or(())?;
		if route.get("chatgptAccountId").and_then(Value::as_str) != Some(expected_account) {
			return Err(());
		}
		let origin = route.get("backendOrigin").and_then(Value::as_str).ok_or(())?;
		let mut responses_url = https_url(origin)?;
		if responses_url.path() != "/"
			|| responses_url.query().is_some()
			|| responses_url.fragment().is_some()
		{
			return Err(());
		}
		let routing = match route.get("accountRoutingOverride").and_then(Value::as_str) {
			Some("NO_CONSTRAINT") => None,
			Some("us") => Some("us"),
			Some("us_cr") => Some("us_cr"),
			_ => return Err(()),
		};
		// Native discovery resolves required backend origins. Compare fresh requirements too:
		// a change between the two read-only calls must not send content to the old origin.
		let requirements = config.get("requirements").ok_or(())?;
		let residency = if requirements.is_null() {
			None
		} else {
			let requirements = requirements.as_object().ok_or(())?;
			match requirements.get("chatgptBaseUrl") {
				Some(Value::String(base))
					if https_url(base)?.origin() == responses_url.origin() => {},
				Some(Value::Null) => {},
				_ => return Err(()),
			}
			match requirements.get("enforceResidency") {
				Some(Value::Null) => None,
				Some(Value::String(value)) if value == "us" => Some("us"),
				_ => return Err(()),
			}
		};
		// Change only the origin of the existing activation endpoint. Account APIs keep
		// their separate account backend; no provider URL or thread is created here.
		responses_url.set_path("/backend-api/codex/responses");
		Ok(Self { responses_url, routing, residency })
	}

	pub(crate) fn request(&self, client: &reqwest::Client) -> RequestBuilder {
		let mut request = client.post(self.responses_url.clone());
		if let Some(routing) = self.routing {
			request = request.header("x-openai-account-routing-override", routing);
		}
		if let Some(residency) = self.residency {
			request = request.header("x-openai-internal-codex-residency", residency);
		}
		request
	}
}

fn https_url(value: &str) -> Result<Url, ()> {
	if value.trim() != value || value.chars().any(char::is_control) {
		return Err(());
	}
	let url = Url::parse(value).map_err(|_| ())?;
	if url.scheme() != "https"
		|| url.host_str().is_none()
		|| !url.username().is_empty()
		|| url.password().is_some()
	{
		return Err(());
	}
	Ok(url)
}

/// Keep the same account lock from credential selection through discovery and HTTP dispatch.
/// The short native child cannot refresh that credential or issue a model request.
pub(crate) async fn read_activation_policy(
	profile: AttestedAppServerProfile,
	account_id: AccountId,
	credential: AccountApiCredential,
) -> Result<(ActivationPolicy, AccountApiCredential), ()> {
	tokio::task::spawn_blocking(move || {
		let binding = ProcessGenerationAccountBinding::new(
			credential.account_revision,
			credential.binding.clone(),
			profile.account_callback_attestation().callback_profile_sha256,
		)
		.map_err(|_| ())?;
		let binding =
			AccountBinding::shared_home_fixed(account_id.clone(), binding).map_err(|_| ())?;
		let capacity = RunnerCapacity::daemon().map_err(|_| ())?;
		let permit =
			capacity.reserve(account_id.clone(), credential.account_revision).map_err(|_| ())?;
		let launch = AttestedAppServerLaunch::bind(profile, binding, POLICY_TIMEOUT, permit)
			.map_err(|_| ())?;
		let mut child = launch.spawn().map_err(|_| ())?;
		let policy = child
			.initialize_ordinary_turns(&ActivationVault { account_id, credential: &credential })
			.and_then(|()| child.read_activation_policy());
		child.shutdown().map_err(|_| ())?;
		Ok((policy.map_err(|_| ())?, credential))
	})
	.await
	.map_err(|_| ())?
}

struct ActivationVault<'a> {
	account_id: AccountId,
	credential: &'a AccountApiCredential,
}

impl CredentialVault for ActivationVault<'_> {
	fn project(
		&self,
		account_id: &AccountId,
		projection: &mut CredentialProjection<'_>,
	) -> Result<AccountIdentity, CredentialVaultError> {
		if account_id != &self.account_id {
			return Err(CredentialVaultError::Unavailable);
		}
		let bundle = self.credential.stored.bundle();
		projection.authenticate_chatgpt(
			bundle.access_token(),
			self.credential.binding.provider.account_id(),
			bundle.plan_type(),
		)?;
		Ok(AccountIdentity::from_observation("chatgpt", Some(bundle.provider_email()), true))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	fn account(origin: &str, routing: &str) -> Value {
		json!({"workspaceRouting":{"chatgptAccountId":"selected", "backendOrigin":origin,"accountRoutingOverride":routing}})
	}

	#[test]
	fn native_route_keeps_path_and_both_independent_policy_headers() {
		let requirements = json!({"requirements":{"chatgptBaseUrl":"https://gov.example/custom", "enforceResidency":"us"}});
		let policy = ActivationPolicy::decode(
			&account("https://gov.example", "us_cr"),
			&requirements,
			"selected",
		)
		.unwrap();
		let request = policy.request(&reqwest::Client::new()).build().unwrap();
		assert_eq!(request.url().as_str(), "https://gov.example/backend-api/codex/responses");
		assert_eq!(request.headers()["x-openai-account-routing-override"], "us_cr");
		assert_eq!(request.headers()["x-openai-internal-codex-residency"], "us");
		let policy = ActivationPolicy::decode(
			&account("https://chatgpt.com", "NO_CONSTRAINT"),
			&json!({"requirements":null}),
			"selected",
		)
		.unwrap();
		assert!(policy.request(&reqwest::Client::new()).build().unwrap().headers().is_empty());
	}

	#[test]
	fn incomplete_mismatched_or_changed_native_policy_cannot_authorize_a_request() {
		let route = account("https://gov.example", "us");
		let unrestricted = json!({"requirements":null});
		assert!(ActivationPolicy::decode(&route, &unrestricted, "other").is_err());
		assert!(
			ActivationPolicy::decode(&json!({"workspaceRouting":null}), &unrestricted, "selected")
				.is_err()
		);
		for requirements in [
			json!({}),
			json!({"requirements":{}}),
			json!({"requirements":{"chatgptBaseUrl":"https://other.example", "enforceResidency":null}}),
			json!({"requirements":{"chatgptBaseUrl":null,"enforceResidency":"future"}}),
		] {
			assert!(ActivationPolicy::decode(&route, &requirements, "selected").is_err());
		}
		for origin in [
			"http://gov.example",
			"https://user@gov.example",
			"https://gov.example/path",
			"https://gov.example/?x=1",
			"https://gov.example/#x",
			" https://gov.example",
			"NO_CONSTRAINT",
		] {
			assert!(
				ActivationPolicy::decode(&account(origin, "us"), &unrestricted, "selected")
					.is_err(),
				"{origin}"
			);
		}
		assert!(
			ActivationPolicy::decode(
				&account("https://gov.example", "future"),
				&unrestricted,
				"selected"
			)
			.is_err()
		);
	}
}
