use serde_json::Value;

use crate::{github_bundle_client::GithubClient, prelude::Result};

impl GithubClient {
	pub(in crate::github_bundle_client) fn github_paginated(
		&self,
		url: &str,
	) -> Result<Vec<Value>> {
		self.api.get_paginated(url)
	}
}

#[cfg(test)]
mod tests {
	use crate::{github_bundle_client::GithubClient, tests::automation::github_api};

	#[test]
	fn bundle_pagination_rejects_cycles_before_repeating_requests() {
		let server = github_api::spawn_server_with(1, |url, _| {
			github_api::response("200 OK", &[("Link", &format!("<{url}>; rel=\"next\""))], "[]")
		});
		let client = GithubClient { api: server.api(None) };
		let result = client.github_paginated(server.url());
		let requests = server.finish_with_requests();

		assert!(result.unwrap_err().to_string().contains("cycle detected"));
		assert_eq!(requests.len(), 1);
	}

	#[test]
	fn bundle_pagination_preserves_order_and_rejects_oversized_collections() {
		let server = github_api::spawn_server_with(2, |url, index| {
			if index == 0 {
				github_api::response(
					"200 OK",
					&[("Link", &format!("<{url}?page=2>; rel=\"next\""))],
					"[1,2]",
				)
			} else {
				github_api::response("200 OK", &[], "[3]")
			}
		});
		let client = GithubClient { api: server.api(None) };
		let result = client.github_paginated(server.url());
		let _requests = server.finish_with_requests();

		assert_eq!(
			result.unwrap(),
			vec![serde_json::json!(1), serde_json::json!(2), serde_json::json!(3)]
		);

		let body = serde_json::to_string(&vec![0; 10_001]).unwrap();
		let server =
			github_api::spawn_server_with(1, |_, _| github_api::response("200 OK", &[], &body));
		let client = GithubClient { api: server.api(None) };
		let result = client.github_paginated(server.url());
		let _requests = server.finish_with_requests();

		assert!(result.unwrap_err().to_string().contains("10000-item limit"));
	}
}
