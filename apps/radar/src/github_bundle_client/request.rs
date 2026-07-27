use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::{github_bundle_client::GithubClient, prelude::Result};

impl GithubClient {
	pub(in crate::github_bundle_client) fn github_request(
		&self,
		url: &str,
	) -> Result<(Value, HeaderMap)> {
		let response = self.api.get(url)?;

		Ok((response.payload, response.headers))
	}
}
