//! Observe original EGM instruction reads, without parsing reports or controlling the workflow.
// CommandAction/read and CommandExecution verified against openai/codex 33d306d4
// and the schema generated from the pinned binary. Unknown remains unclassified.
use decodex_protocol::AgentReviewActivity;
use serde_json::Value;

pub(super) fn observe(item: &Value, completed: bool) -> Option<AgentReviewActivity> {
	if !completed || item["type"] != "commandExecution" || item["exitCode"] != 0 {
		return None;
	}

	let attributed = item["pluginId"] == "egm@egm";
	let mut observations: Vec<_> = item["commandActions"]
		.as_array()
		.into_iter()
		.flatten()
		.filter(|action| action["type"] == "read")
		.filter_map(|action| classify(action["path"].as_str()?, attributed))
		.collect();

	// Native CommandAction is Unknown for a cat with several operands. Accept only
	// a plain cat command, not scripts, searches, pipelines or echoed path strings.
	if let Some(command) = item["command"].as_str().and_then(plain_cat) {
		observations.extend(command.filter_map(|path| classify(path, attributed)));
	}

	observations.into_iter().max_by_key(|step| match step {
		AgentReviewActivity::Prepare => 0,
		AgentReviewActivity::Assess => 1,
		AgentReviewActivity::Reviewer { .. } => 2,
	})
}

fn plain_cat(command: &str) -> Option<impl Iterator<Item = &str>> {
	let command = ["/bin/zsh -lc '", "/bin/bash -lc '", "/bin/sh -lc '"]
		.into_iter()
		.find_map(|prefix| command.strip_prefix(prefix)?.strip_suffix('\''))
		.unwrap_or(command);
	if command.contains(['\n', ';', '|', '&', '>', '<', '$', '`', '\\']) {
		return None;
	}
	let mut words = command.split_whitespace();
	if !["cat", "/bin/cat"].contains(&words.next()?) {
		return None;
	}
	Some(words.map(|word| word.trim_matches(['\'', '"'])))
}

fn classify(path: &str, attributed: bool) -> Option<AgentReviewActivity> {
	let suffix = if let Some((_, versioned)) = path.split_once("/plugins/cache/egm/egm/") {
		let (version, suffix) = versioned.split_once('/')?;
		if version.is_empty() || version == ".." {
			return None;
		}
		suffix
	} else if attributed {
		path.split_once("/skills/")
			.map(|(_, suffix)| ("skills", suffix))
			.or_else(|| path.split_once("/agents/").map(|(_, suffix)| ("agents", suffix)))
			.and_then(|(kind, suffix)| match (kind, suffix) {
				("skills", "egm-review-loop/SKILL.md") => Some("skills/egm-review-loop/SKILL.md"),
				("skills", "egm-resolve/SKILL.md") => Some("skills/egm-resolve/SKILL.md"),
				_ => None,
			})?
	} else {
		return None;
	};

	match suffix {
		"skills/egm-review-loop/SKILL.md" => Some(AgentReviewActivity::Prepare),
		"skills/egm-resolve/SKILL.md" => Some(AgentReviewActivity::Assess),
		_ => {
			let role = suffix.strip_prefix("agents/")?.strip_suffix(".md")?;
			if role == "synthesizer"
				|| role.is_empty()
				|| role.len() > 64
				|| !role.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
			{
				return None;
			}
			Some(AgentReviewActivity::Reviewer { role: role.into() })
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn native_multi_file_read_identifies_the_reviewer_without_exposing_the_command() {
		let item = json!({"type":"commandExecution","exitCode":0,"command":
			"/bin/zsh -lc 'cat /tmp/home/plugins/cache/egm/egm/0.1.0/agents/implementation_readiness.md /tmp/home/plugins/cache/egm/egm/0.1.0/skills/egm-review-loop/SKILL.md'"});
		assert_eq!(
			observe(&item, true),
			Some(AgentReviewActivity::Reviewer { role: "implementation_readiness".into() })
		);
		assert_eq!(observe(&item, false), None);
	}

	#[test]
	fn failed_reads_and_unrelated_text_do_not_create_review_state() {
		let path = "/tmp/home/plugins/cache/egm/egm/0.1.0/skills/egm-review-loop/SKILL.md";
		for command in [
			format!("echo {path}"),
			format!("rg {path} notes.md"),
			format!("cat notes.md; echo {path}"),
			format!("cat {path} | head"),
		] {
			assert_eq!(
				observe(&json!({"type":"commandExecution","exitCode":0,"command":command}), true),
				None
			);
		}
		assert_eq!(
			observe(
				&json!({"type":"commandExecution","exitCode":1,"command":format!("cat {path}")}),
				true
			),
			None
		);
		assert_eq!(
			observe(
				&json!({"type":"commandExecution","exitCode":0,"command":"cat /tmp/egm-review-loop/SKILL.md"}),
				true
			),
			None
		);
	}

	#[test]
	fn native_read_actions_keep_the_exact_workflow_stage() {
		let action = |path| json!({"type":"commandExecution","exitCode":0,"commandActions":[{"type":"read","path":path}]});
		assert_eq!(
			observe(
				&action("/tmp/home/plugins/cache/egm/egm/0.1.0/skills/egm-resolve/SKILL.md"),
				true
			),
			Some(AgentReviewActivity::Assess)
		);
		assert_eq!(
			observe(&action("/tmp/home/plugins/cache/egm/egm/0.1.0/agents/synthesizer.md"), true),
			None
		);
	}
}
