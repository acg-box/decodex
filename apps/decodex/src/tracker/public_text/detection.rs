use crate::tracker::public_text::constants::{
	CREDENTIAL_MARKERS, HOST_PATH_PREFIXES, PRIVATE_CONFIG_FILES, PRIVATE_ENV_VAR_TOKENS,
	SENSITIVE_PHRASES,
};

pub(crate) fn public_text_violation(value: &str) -> Option<&'static str> {
	if contains_host_path(value) {
		return Some("host-local paths are not allowed");
	}
	if contains_credential_like_name(value) {
		return Some("credential-like names are not allowed");
	}
	if contains_private_env_var_token(value) {
		return Some("private environment variable names are not allowed");
	}
	if contains_email_address(value) {
		return Some("private identity details are not allowed");
	}
	if contains_sensitive_phrase(value) {
		return Some("local identity or account-routing details are not allowed");
	}

	None
}

fn contains_host_path(value: &str) -> bool {
	let lower = value.to_ascii_lowercase();

	lower.contains("~/")
		|| lower.contains("~\\")
		|| HOST_PATH_PREFIXES.iter().any(|prefix| lower.contains(prefix))
		|| contains_windows_absolute_path(value)
}

fn contains_windows_absolute_path(value: &str) -> bool {
	value.as_bytes().windows(3).enumerate().any(|(index, window)| {
		let preceded_by_separator =
			index == 0 || !value.as_bytes()[index - 1].is_ascii_alphanumeric();

		preceded_by_separator
			&& window[0].is_ascii_alphabetic()
			&& window[1] == b':'
			&& matches!(window[2], b'\\' | b'/')
	})
}

fn contains_credential_like_name(value: &str) -> bool {
	value.split(is_token_separator).any(is_credential_like_token)
}

fn is_token_separator(character: char) -> bool {
	!(character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

fn is_credential_like_token(token: &str) -> bool {
	if token.is_empty() {
		return false;
	}

	let has_name_shape = token.contains('_') || token.contains('-') || is_all_caps_token(token);

	if !has_name_shape {
		return false;
	}

	let normalized = token.replace('-', "_").to_ascii_uppercase();

	CREDENTIAL_MARKERS.iter().any(|marker| {
		normalized == *marker
			|| normalized.starts_with(&format!("{marker}_"))
			|| normalized.ends_with(&format!("_{marker}"))
			|| normalized.contains(&format!("_{marker}_"))
	})
}

fn is_all_caps_token(token: &str) -> bool {
	let mut has_letter = false;

	for character in token.chars() {
		if character.is_ascii_lowercase() {
			return false;
		}
		if character.is_ascii_alphabetic() {
			has_letter = true;
		}
	}

	has_letter
}

fn contains_private_env_var_token(value: &str) -> bool {
	value.split(is_token_separator).any(|token| {
		let normalized = token.to_ascii_uppercase();

		PRIVATE_ENV_VAR_TOKENS.iter().any(|env_var| normalized == *env_var)
	})
}

fn contains_email_address(value: &str) -> bool {
	value.split_whitespace().any(is_email_like_token)
}

fn is_email_like_token(token: &str) -> bool {
	let token = token.trim_matches(|character: char| {
		matches!(character, '`' | '\'' | '"' | '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';' | '.')
	});
	let Some((local, domain)) = token.split_once('@') else {
		return false;
	};

	!local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn contains_sensitive_phrase(value: &str) -> bool {
	let lower = value.to_ascii_lowercase();

	SENSITIVE_PHRASES.iter().any(|phrase| lower.contains(phrase))
		|| PRIVATE_CONFIG_FILES.iter().any(|file_name| lower.contains(file_name))
}
