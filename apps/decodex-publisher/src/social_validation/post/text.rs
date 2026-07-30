use crate::social_validation::{self, Value};

pub(in crate::social_validation) fn validate_social_post_text(
	text: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(items) = social_validation::non_empty_array(text) else {
		errors.push("text must be a non-empty list of X-sized strings".into());

		return;
	};

	for (index, text) in items.iter().enumerate() {
		let Some(text) = text.as_str() else {
			errors.push(format!("text[{index}] must be a string"));

			continue;
		};

		validate_social_post_text_item(text, index, errors);
	}
}

fn validate_social_post_text_item(text: &str, index: usize, errors: &mut Vec<String>) {
	if text.is_empty() || exceeds_conservative_x_weighted_length(text, 260) {
		errors.push(format!("text[{index}] must be a non-empty X-sized string"));
	}
	if text.contains("Automated by @hackink") {
		errors.push(format!("text[{index}] must not include automation attribution"));
	}
	if contains_link_like_text(text) {
		errors.push(format!(
			"text[{index}] must not contain URL, domain, email, or other link-like text"
		));
	}

	let normalized = text.trim().to_ascii_lowercase();

	if normalized == "watching this"
		|| normalized.starts_with("watching this.")
		|| normalized.starts_with("tracking this.")
		|| normalized.contains("new release available")
	{
		errors.push(format!(
			"text[{index}] must name a concrete source-backed release, PR, protocol surface, workflow impact, or operator action"
		));
	}
}

fn exceeds_conservative_x_weighted_length(text: &str, maximum: usize) -> bool {
	let mut weighted_length = 0_usize;

	for character in text.chars() {
		let codepoint = u32::from(character);
		let weight = if (0..=4_351).contains(&codepoint)
			|| (8_192..=8_205).contains(&codepoint)
			|| (8_208..=8_223).contains(&codepoint)
			|| (8_242..=8_247).contains(&codepoint)
		{
			1
		} else {
			2
		};
		weighted_length = weighted_length.saturating_add(weight);
		if weighted_length > maximum {
			return true;
		}
	}

	false
}

pub(crate) fn contains_link_like_text(text: &str) -> bool {
	let normalized = text.replace(['。', '．', '｡'], ".").to_lowercase();
	if normalized.contains("://") || normalized.contains("www.") || normalized.contains("mailto:") {
		return true;
	}

	normalized.split_whitespace().any(token_is_link_like)
}

fn token_is_link_like(token: &str) -> bool {
	let token = token.trim_matches(|character: char| {
		matches!(
			character,
			',' | ';' | ':' | '!' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\''
		)
	});
	let host = token
		.rsplit_once('@')
		.map_or(token, |(_, host)| host)
		.split(['/', '?', '#'])
		.next()
		.unwrap_or_default()
		.trim_end_matches('.');
	if host.is_empty() || !host.contains('.') {
		return false;
	}
	let host = host
		.rsplit_once(':')
		.filter(|(_, port)| {
			!port.is_empty() && port.chars().all(|character| character.is_ascii_digit())
		})
		.map_or(host, |(host, _)| host);
	let labels = host.split('.').collect::<Vec<_>>();
	if labels.len() < 2 || labels.iter().any(|label| !valid_domain_label(label)) {
		return false;
	}
	if labels.len() == 4
		&& labels.iter().all(|label| {
			label.parse::<u8>().is_ok() && label.bytes().all(|byte| byte.is_ascii_digit())
		}) {
		return true;
	}
	let top_level = labels.last().copied().unwrap_or_default();
	top_level.len() >= 2 && top_level.chars().all(char::is_alphabetic)
}

fn valid_domain_label(label: &str) -> bool {
	!label.is_empty()
		&& !label.starts_with('-')
		&& !label.ends_with('-')
		&& label.chars().all(|character| character.is_alphanumeric() || character == '-')
}

#[cfg(test)]
mod tests {
	use super::{contains_link_like_text, validate_social_post_text_item};

	#[test]
	fn enforces_a_conservative_x_v3_weighted_length() {
		for text in ["a".repeat(260), "界".repeat(130)] {
			let mut errors = Vec::new();

			validate_social_post_text_item(&text, 0, &mut errors);

			assert!(errors.is_empty(), "{errors:?}");
		}

		let mut errors = Vec::new();
		validate_social_post_text_item(&"界".repeat(131), 0, &mut errors);

		assert_eq!(errors, ["text[0] must be a non-empty X-sized string"]);
	}

	#[test]
	fn rejects_practical_link_forms() {
		for text in [
			"See example.com",
			"Read docs.example.dev/path",
			"Mail user@example.org",
			"Open www.example.net",
			"Use https://example.com",
			"See 192.0.2.1/status",
		] {
			assert!(contains_link_like_text(text), "{text}");
		}
	}

	#[test]
	fn permits_versions_and_repository_names() {
		for text in ["Codex v1.3.1 is ready", "Use openai/codex", "Fix app-server v2"] {
			assert!(!contains_link_like_text(text), "{text}");
		}
	}
}
