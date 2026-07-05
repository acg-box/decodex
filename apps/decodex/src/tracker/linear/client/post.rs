use serde::{Deserialize, Serialize};

use crate::{
	prelude::{Result, eyre},
	tracker::linear::{
		LinearClient,
		queries::LINEAR_GRAPHQL_URL,
		schema::{GraphqlRequest, GraphqlResponse},
		transport,
	},
};

impl LinearClient {
	pub(in crate::tracker::linear) fn post<V, T>(&self, query: &str, variables: &V) -> Result<T>
	where
		V: Serialize,
		T: for<'de> Deserialize<'de>,
	{
		let response = self
			.http
			.post(LINEAR_GRAPHQL_URL)
			.header("Authorization", &self.api_token)
			.json(&GraphqlRequest { query, variables })
			.send()
			.map_err(transport::linear_transport_error)?;
		let status = response.status();
		let body = response.text().map_err(transport::linear_transport_error)?;
		let payload = serde_json::from_str::<GraphqlResponse<T>>(&body).map_err(|error| {
			if status.is_success() {
				eyre::eyre!("Failed to parse Linear GraphQL response: {error}")
			} else {
				eyre::eyre!(
					"Linear HTTP request failed with status `{}` and an unparseable GraphQL body: {error}",
					status
				)
			}
		})?;

		if let Some(errors) = payload.errors {
			if let Some(message) = transport::rate_limited_error_message(&errors) {
				eyre::bail!("{message}");
			}

			let messages =
				errors.into_iter().map(|error| error.message).collect::<Vec<_>>().join("; ");

			eyre::bail!("Linear GraphQL request failed: {messages}");
		}

		if !status.is_success() {
			eyre::bail!("Linear HTTP request failed with status `{status}`.");
		}

		payload.data.ok_or_else(|| eyre::eyre!("Linear GraphQL response did not include data."))
	}
}
