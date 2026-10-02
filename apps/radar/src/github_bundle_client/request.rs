use serde_json::Value;

use crate::{github_bundle_client::GithubClient, prelude::Result};

impl GithubClient {
	pub(in crate::github_bundle_client) fn github_commit_request(
		&self,
		url: &str,
	) -> Result<Value> {
		self.api.get_paginated_field(url, "files")
	}

	pub(in crate::github_bundle_client) fn github_request(&self, url: &str) -> Result<Value> {
		let response = self.api.get(url)?;

		Ok(response.payload)
	}
}

#[cfg(test)]
mod tests {
	use crate::{github_bundle_client::GithubClient, tests::automation::github_api};

	#[test]
	fn commit_request_collects_paginated_files_for_bundle_construction() {
		let server = github_api::spawn_server_with(4, |url, page| {
			let files = (page * 100..((page + 1) * 100).min(301)).map(|i| serde_json::json!({"filename": format!("src/file-{i}.rs"), "status": "modified", "additions": 1, "deletions": 0})).collect::<Vec<_>>();
			let body = serde_json::json!({"sha": "a".repeat(40), "html_url": "https://github.com/openai/codex/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "commit": {"message": format!("page-{page}"), "committer": {"date": "2026-06-01T00:00:00Z"}}, "files": files}).to_string();

			if page < 3 {
				github_api::response(
					"200 OK",
					&[("Link", &format!("<{url}?page={}>; rel=\"next\"", page + 2))],
					&body,
				)
			} else {
				github_api::response("200 OK", &[], &body)
			}
		});
		let client = GithubClient { api: server.api(None) };
		let payload = client.github_commit_request(server.url());
		// Join the fixture before inspecting response data.
		let requests = server.finish_with_requests();
		let payload = payload.unwrap();
		let bundle =
			crate::build_commit_bundle_from_sources("openai/codex", &payload, "main", &[]).unwrap();

		assert_eq!(bundle["files"].as_array().unwrap().len(), 301);
		assert_eq!(bundle["files"][300]["path"], "src/file-300.rs");
		assert_eq!(bundle["commits"][0]["message"], "page-0");
		assert_eq!(requests.len(), 4);
	}
}
