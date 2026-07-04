use reqwest::header::LINK;
use serde_json::Value;

use crate::{
	github_bundle_client::GithubClient,
	prelude::{Result, eyre},
};

impl GithubClient {
	pub(in crate::github_bundle_client) fn github_paginated(
		&self,
		url: &str,
	) -> Result<Vec<Value>> {
		let mut items = Vec::new();
		let mut next_url = Some(url.to_owned());

		while let Some(url) = next_url {
			let (payload, headers) = self.github_request(&url)?;
			let Some(values) = payload.as_array() else {
				eyre::bail!("Expected list payload from {url}");
			};

			items.extend(values.iter().cloned());

			next_url =
				headers.get(LINK).and_then(|value| value.to_str().ok()).and_then(parse_next_link);
		}

		Ok(items)
	}
}

fn parse_next_link(header: &str) -> Option<String> {
	for part in header.split(',') {
		let mut sections = part.trim().split(';');
		let Some(url_part) = sections.next() else {
			continue;
		};

		if sections.any(|section| section.trim() == r#"rel="next""#) {
			return Some(url_part.trim().trim_start_matches('<').trim_end_matches('>').into());
		}
	}

	None
}
