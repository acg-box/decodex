#[allow(clippy::wildcard_imports)]
use super::*;

pub(in crate::orchestrator::dispatch_policy) fn description_is_machine_only_fenced_block(
	description: &str,
) -> bool {
	let trimmed = description.trim();

	if trimmed.is_empty() {
		return false;
	}

	let mut saw_fence = false;
	let mut inside_fence = false;
	let mut current_fence_marker = b'`';
	let mut current_fence_ticks = 0;
	let mut current_fence_info = String::new();
	let mut current_fence_body = String::new();

	for line in trimmed.lines() {
		let trimmed_line = line.trim();

		if let Some((fence_marker, fence_ticks, fence_tail)) = parse_code_fence(trimmed_line) {
			if inside_fence {
				if fence_marker == current_fence_marker
					&& fence_ticks >= current_fence_ticks
					&& fence_tail.is_empty()
				{
					if !fenced_block_is_machine_readable(&current_fence_info, &current_fence_body) {
						return false;
					}

					inside_fence = false;
					current_fence_marker = b'`';
					current_fence_ticks = 0;

					current_fence_info.clear();
					current_fence_body.clear();

					continue;
				}
			} else {
				saw_fence = true;
				inside_fence = true;
				current_fence_marker = fence_marker;
				current_fence_ticks = fence_ticks;
				current_fence_info = fence_tail.to_ascii_lowercase();

				current_fence_body.clear();

				continue;
			}
		}

		if inside_fence {
			current_fence_body.push_str(line);
			current_fence_body.push('\n');

			continue;
		}
		if !inside_fence && !trimmed_line.is_empty() {
			return false;
		}
	}

	saw_fence && !inside_fence
}

fn parse_code_fence(line: &str) -> Option<(u8, usize, &str)> {
	let first_byte = *line.as_bytes().first()?;

	if first_byte != b'`' && first_byte != b'~' {
		return None;
	}

	let fence_ticks = line.bytes().take_while(|byte| *byte == first_byte).count();

	if fence_ticks < 3 {
		return None;
	}

	Some((first_byte, fence_ticks, line[fence_ticks..].trim()))
}

fn fenced_block_is_machine_readable(fence_info: &str, fence_body: &str) -> bool {
	if !fence_info.is_empty() && fence_info != "json" {
		return false;
	}

	match serde_json::from_str::<Value>(fence_body.trim()) {
		Ok(payload) => payload.is_object() || payload.is_array(),
		Err(_) => false,
	}
}

pub(in crate::orchestrator) fn render_issue_description_for_prompt(issue: &TrackerIssue) -> String {
	if issue.description.trim().is_empty() {
		return String::from("(no description)");
	}
	if description_is_machine_only_fenced_block(&issue.description) {
		return String::from(
			"(machine-only tracker description omitted; this lane requires a separate generic issue briefing surface)",
		);
	}

	issue.description.clone()
}
