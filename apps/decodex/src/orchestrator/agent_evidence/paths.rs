use std::path::{Path, PathBuf};

use time::OffsetDateTime;

pub(crate) fn issue_key(issue_identifier: Option<&str>, issue_id: &str) -> String {
	issue_identifier.map_or_else(
		|| sanitize_evidence_path_component(issue_id),
		sanitize_evidence_path_component,
	)
}

pub(crate) fn blocker_snapshot_path(blockers_dir: &Path, issue_key: &str) -> PathBuf {
	blockers_dir.join(format!("{issue_key}.json"))
}

pub(crate) fn run_capsule_path(runs_dir: &Path, month_bucket: &str, run_id: &str) -> PathBuf {
	runs_dir.join(month_bucket).join(sanitize_evidence_path_component(run_id)).join("capsule.json")
}

pub(crate) fn run_evidence_ref(project_id: &str, run_id: &str) -> String {
	format!("run:{project_id}/{run_id}")
}

pub(crate) fn blocker_evidence_ref(project_id: &str, issue_key: &str, reason_code: &str) -> String {
	format!("blocker:{project_id}/{issue_key}/{reason_code}")
}

pub(crate) fn sanitize_evidence_path_component(raw: &str) -> String {
	let mut out = String::new();
	let mut previous_dash = false;

	for byte in raw.bytes() {
		let character = byte as char;

		if character.is_ascii_alphanumeric() {
			out.push(character.to_ascii_lowercase());

			previous_dash = false;
		} else if !previous_dash {
			out.push('-');

			previous_dash = true;
		}
	}

	let out = out.trim_matches('-').to_owned();

	if out.is_empty() { String::from("unknown") } else { out }
}

pub(crate) fn current_month_bucket() -> String {
	let now = OffsetDateTime::now_utc();

	format!("{:04}-{:02}", now.year(), u8::from(now.month()))
}
