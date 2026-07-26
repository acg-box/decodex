use crate::social_validation::{self, Map, SOCIAL_BLOCK_REASONS, Value};

#[derive(Clone, Copy)]
pub(in crate::social_validation) enum BrowserSessionRequirement {
	Complete,
	Terminal,
}

pub(super) fn validate_social_post_status_payload(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	let status = social_validation::string_field(entry, "status");

	match status {
		Some("published") => validate_social_post_publication(entry.get("publication"), errors),
		Some("blocked") => validate_social_post_block(entry, errors),
		Some("failed") =>
			validate_reason_object(entry.get("failure"), "failure", &["reason", "details"], errors),
		Some("skipped") => validate_reason_object(entry.get("skip"), "skip", &["reason"], errors),
		_ => {},
	}
	validate_exclusive_status_payload(entry, status, errors);
}

fn validate_exclusive_status_payload(
	entry: &Map<String, Value>,
	status: Option<&str>,
	errors: &mut Vec<String>,
) {
	let expected = match status {
		Some("published") => "publication",
		Some("blocked") => "block",
		Some("failed") => "failure",
		Some("skipped") => "skip",
		_ => return,
	};

	for field in ["publication", "block", "failure", "skip"] {
		if field != expected && entry.get(field).is_some() {
			errors.push(format!(
				"{field} must be absent when status is {}",
				status.unwrap_or_default()
			));
		}
	}
}

fn validate_social_post_publication(publication: Option<&Value>, errors: &mut Vec<String>) {
	let Some(publication) = publication.and_then(Value::as_object) else {
		errors.push("publication must be an object when status is published".into());

		return;
	};
	social_validation::validate_exact_keys(
		publication,
		"publication",
		&[
			"account_verified",
			"image_template",
			"made_with_ai",
			"posted_at",
			"published_urls",
			"publisher",
		],
		errors,
	);

	if !social_validation::is_non_empty_string(publication.get("posted_at")) {
		errors.push("publication.posted_at must be a non-empty string".into());
	}
	if social_validation::string_field(publication, "publisher") != Some("chrome") {
		errors.push("publication.publisher must be chrome".into());
	}

	social_validation::validate_rfc3339_field(publication, "posted_at", errors);

	if publication.get("account_verified").and_then(Value::as_bool) != Some(true) {
		errors.push("publication.account_verified must be true".into());
	}
	if !publication.get("made_with_ai").is_some_and(Value::is_boolean) {
		errors.push("publication.made_with_ai must be a boolean".into());
	}
	if !publication.get("published_urls").is_some_and(|urls| {
		social_validation::is_https_string_array(urls)
			&& !social_validation::is_empty_or_missing_array(Some(urls))
			&& urls.as_array().is_some_and(|urls| {
				urls.iter().all(|url| {
					url.as_str()
						.is_some_and(|url| url.starts_with("https://x.com/decodexspace/status/"))
				})
			})
	}) {
		errors
			.push("publication.published_urls must contain only decodexspace X status URLs".into());
	}
}

fn validate_social_post_block(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(block) = entry.get("block").and_then(Value::as_object) else {
		errors.push("block must be an object when status is blocked".into());

		return;
	};
	social_validation::validate_exact_keys(block, "block", &["operator_notice", "reason"], errors);

	if !social_validation::matches_one_of(block.get("reason"), SOCIAL_BLOCK_REASONS) {
		errors.push(format!(
			"block.reason must be one of {}",
			social_validation::choices(SOCIAL_BLOCK_REASONS)
		));
	}
	if !social_validation::is_non_empty_string(block.get("operator_notice")) {
		errors.push("block.operator_notice must be a non-empty string".into());
	}
}

fn validate_reason_object(
	value: Option<&Value>,
	label: &str,
	required_fields: &[&str],
	errors: &mut Vec<String>,
) {
	let Some(object) = value.and_then(Value::as_object) else {
		errors.push(format!("{label} must be an object when status is {label}"));

		return;
	};
	social_validation::validate_exact_keys(object, label, required_fields, errors);

	for field in required_fields {
		if !social_validation::is_non_empty_string(object.get(*field)) {
			errors.push(format!("{label}.{field} must be a non-empty string"));
		}
	}
}

pub(in crate::social_validation) fn validate_browser_session(
	session: Option<&Value>,
	requirement: BrowserSessionRequirement,
	errors: &mut Vec<String>,
) {
	let Some(session) = session.and_then(Value::as_object) else {
		errors.push("browser_session must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(
		session,
		"browser_session",
		&[
			"initial_account",
			"restore_status",
			"switch_status",
			"target_account",
			"target_account_verified",
		],
		errors,
	);

	let initial_account = social_validation::string_field(session, "initial_account");
	if !matches!(initial_account, Some("unknown" | "hackink" | "decodexspace")) {
		errors.push(
			"browser_session.initial_account must be unknown, hackink, or decodexspace".into(),
		);
	}
	if social_validation::string_field(session, "target_account") != Some("decodexspace") {
		errors.push("browser_session.target_account must be decodexspace".into());
	}
	let target_verified = session.get("target_account_verified").and_then(Value::as_bool);
	if target_verified.is_none() {
		errors.push("browser_session.target_account_verified must be a boolean".into());
	}

	let switch_status = social_validation::string_field(session, "switch_status");
	let restore_status = social_validation::string_field(session, "restore_status");

	if matches!(requirement, BrowserSessionRequirement::Complete) && target_verified != Some(true) {
		errors.push("browser_session.target_account_verified must be true".into());
	}

	match (initial_account, target_verified) {
		(Some("hackink"), Some(true)) =>
			validate_switched_and_restored(switch_status, restore_status, errors),
		(Some("decodexspace"), Some(true)) =>
			validate_no_switch_required(switch_status, restore_status, errors),
		(Some("unknown"), Some(true)) => errors.push(
			"browser_session.initial_account cannot be unknown after target verification".into(),
		),
		(Some("hackink"), Some(false)) => {
			if !matches!(switch_status, Some("failed" | "not_attempted")) {
				errors.push(
					"browser_session.switch_status must be failed or not_attempted when target verification failed"
						.into(),
				);
			}
			match switch_status {
				Some("failed") if !matches!(restore_status, Some("restored" | "failed")) =>
					errors.push(
						"browser_session.restore_status must be restored or failed after an uncertain switch"
							.into(),
					),
				Some("not_attempted") if restore_status != Some("not_attempted") =>
					errors.push(
						"browser_session.restore_status must be not_attempted when switch was not attempted"
							.into(),
					),
				_ => {},
			}
		},
		(Some("decodexspace"), Some(false)) =>
			validate_no_switch_required(switch_status, restore_status, errors),
		(Some("unknown"), Some(false))
			if switch_status != Some("not_attempted")
				|| restore_status != Some("not_attempted") =>
			errors.push(
				"browser_session switch and restore must be not_attempted when initial account is unknown"
					.into(),
			),
		_ => {},
	}
}

fn validate_switched_and_restored(
	switch_status: Option<&str>,
	restore_status: Option<&str>,
	errors: &mut Vec<String>,
) {
	if switch_status != Some("switched") {
		errors.push(
			"browser_session.switch_status must be switched when initial account is hackink".into(),
		);
	}
	if !matches!(restore_status, Some("restored" | "failed")) {
		errors.push(
			"browser_session.restore_status must be restored or failed when initial account is hackink"
				.into(),
		);
	}
}

fn validate_no_switch_required(
	switch_status: Option<&str>,
	restore_status: Option<&str>,
	errors: &mut Vec<String>,
) {
	if switch_status != Some("not_required") {
		errors.push(
			"browser_session.switch_status must be not_required when initial account is decodexspace"
				.into(),
		);
	}
	if restore_status != Some("not_required") {
		errors.push(
			"browser_session.restore_status must be not_required when initial account is decodexspace"
				.into(),
		);
	}
}
