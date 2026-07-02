pub(crate) fn account_recovery_action(
	status: &str,
	refresh_token_present: bool,
	refresh_status: Option<&str>,
	note: Option<&str>,
) -> Option<String> {
	let status = status.trim().to_ascii_lowercase();
	let refresh_status = refresh_status.unwrap_or_default().trim().to_ascii_lowercase();

	if status == "disabled" || status == "cooldown" {
		return None;
	}
	if status == "auth_failed" || refresh_status == "auth_failed" {
		return Some(String::from("login"));
	}
	if !refresh_token_present {
		return Some(String::from("login"));
	}
	if refresh_status == "failed" {
		let note = note.unwrap_or_default().to_ascii_lowercase();

		if note.contains("401") || note.contains("unauthorized") {
			return Some(String::from("login"));
		}

		return Some(String::from("retry_probe"));
	}

	match status.as_str() {
		"expired" => Some(String::from("refresh")),
		"unusable" => Some(String::from("login")),
		"probe_failed" => Some(String::from("retry_probe")),
		_ => None,
	}
}
