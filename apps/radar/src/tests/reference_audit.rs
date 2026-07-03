use std::{fs, path::Path};

pub(in crate::tests) fn collect_assign_task_reference_violations(
	path: &Path,
	repo_root: &Path,
	offenders: &mut Vec<String>,
) {
	let Ok(metadata) = fs::metadata(path) else {
		return;
	};

	if metadata.is_dir() {
		let entries = fs::read_dir(path).expect("reference audit directory should be readable");

		for entry in entries {
			let entry = entry.expect("reference audit directory entry should be readable");

			collect_assign_task_reference_violations(&entry.path(), repo_root, offenders);
		}

		return;
	}
	if !metadata.is_file() || !should_audit_multi_agent_v2_reference_file(path) {
		return;
	}

	let text = fs::read_to_string(path).expect("reference audit file should be utf-8 text");
	let lower = text.to_ascii_lowercase();

	if !lower.contains("assign_task") {
		return;
	}
	if lower.contains("followup_task") && crate::has_legacy_multi_agent_v2_context(&lower) {
		return;
	}

	let relative = path.strip_prefix(repo_root).unwrap_or(path);

	offenders.push(relative.display().to_string());
}

fn should_audit_multi_agent_v2_reference_file(path: &Path) -> bool {
	let extension = path.extension().and_then(|value| value.to_str());

	matches!(extension, Some("json" | "md" | "py" | "rs" | "ts" | "tsx"))
}
