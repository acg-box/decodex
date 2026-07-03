use std::sync::OnceLock;

use regex::{Error, Regex};

use crate::eyre;

pub(super) fn collect_issue_refs(texts: &[&str]) -> crate::prelude::Result<Vec<String>> {
	collect_regex_matches(issue_ref_regex()?, texts)
}

pub(super) fn collect_flags(texts: &[&str]) -> crate::prelude::Result<Vec<String>> {
	collect_regex_matches(flag_regex()?, texts)
}

fn collect_regex_matches(regex: &Regex, texts: &[&str]) -> crate::prelude::Result<Vec<String>> {
	let mut found = Vec::new();

	for text in texts {
		for captures in regex.captures_iter(text) {
			let Some(value) = captures.get(1).map(|matched| matched.as_str()) else {
				continue;
			};

			if !found.iter().any(|found_value| found_value == value) {
				found.push(value.to_owned());
			}
		}
	}

	Ok(found)
}

fn issue_ref_regex() -> crate::prelude::Result<&'static Regex> {
	static ISSUE_REF_RE: OnceLock<std::result::Result<Regex, Error>> = OnceLock::new();

	ISSUE_REF_RE
		.get_or_init(|| Regex::new(r"(?:^|[^\w])((?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#\d+)"))
		.as_ref()
		.map_err(|error| eyre::eyre!("Failed to compile issue reference regex: {error}"))
}

fn flag_regex() -> crate::prelude::Result<&'static Regex> {
	static FLAG_RE: OnceLock<std::result::Result<Regex, Error>> = OnceLock::new();

	FLAG_RE
		.get_or_init(|| {
			Regex::new(r"(?:^|[^\w-])(--[a-zA-Z0-9][\w-]*|[A-Z][A-Z0-9_]{2,}(?:=[^\s,`]+)?)")
		})
		.as_ref()
		.map_err(|error| eyre::eyre!("Failed to compile flag regex: {error}"))
}
