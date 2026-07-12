mod archive;
mod blockers;
mod comments;
mod identity;
mod pagination;
mod post;

use std::time::Duration;

use reqwest::blocking::Client;

use crate::prelude::Result;

const LINEAR_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const LINEAR_HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct LinearClient {
	api_token: String,
	http: Client,
}
impl LinearClient {
	pub(crate) fn new(api_token: String) -> Result<Self> {
		let http = Client::builder()
			.connect_timeout(LINEAR_HTTP_CONNECT_TIMEOUT)
			.timeout(LINEAR_HTTP_TIMEOUT)
			.build()?;

		Ok(Self { api_token, http })
	}
}
