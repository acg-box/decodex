use serde_json::Value;

pub(super) fn redact_reasoning_protocol_activity(value: &mut Value) {
	match value {
		Value::Object(object) => {
			let is_reasoning_event = object
				.get("category")
				.and_then(Value::as_str)
				.is_some_and(|category| category.eq_ignore_ascii_case("reasoning"))
				|| object.get("event_type").and_then(Value::as_str).is_some_and(|event_type| {
					event_type.to_ascii_lowercase().contains("reasoning")
				});

			if is_reasoning_event {
				object.insert(
					String::from("detail"),
					Value::String(String::from("redacted_reasoning")),
				);
				object.remove("text");
				object.remove("summary");
				object.remove("content");
				object.remove("body");
			}

			for child in object.values_mut() {
				redact_reasoning_protocol_activity(child);
			}
		},
		Value::Array(items) =>
			for item in items {
				redact_reasoning_protocol_activity(item);
			},
		_ => {},
	}
}

pub(in crate::mcp) fn sanitize_mcp_observability_value(value: &mut Value) {
	match value {
		Value::Object(object) => {
			for key in [
				"worktreePath",
				"worktree_path",
				"channelPath",
				"channel_path",
				"requestPath",
				"request_path",
				"configPath",
				"config_path",
				"repoRoot",
				"repo_root",
				"effectiveCwd",
				"effective_cwd",
				"cwd",
				"privateEvidence",
				"private_evidence",
				"privateEvidenceRef",
				"private_evidence_ref",
				"privateEvidenceRefs",
				"private_evidence_refs",
				"executionProgramId",
				"execution_program_id",
				"executionProgramNodeIds",
				"execution_program_node_ids",
				"graphId",
				"graph_id",
				"nodeId",
				"node_id",
				"programId",
				"program_id",
				"readCommand",
				"read_command",
				"githubCliAuthority",
				"github_cli_authority",
				"githubCommandPath",
				"github_command_path",
				"ghCommandPath",
				"gh_command_path",
				"githubTokenEnvVar",
				"github_token_env_var",
				"path",
			] {
				object.remove(key);
			}
			for child in object.values_mut() {
				sanitize_mcp_observability_value(child);
			}
		},
		Value::String(text) =>
			if observability_string_contains_sensitive_text(text) {
				*text = String::from("redacted_sensitive_detail");
			},
		Value::Array(items) =>
			for item in items {
				sanitize_mcp_observability_value(item);
			},
		_ => {},
	}
}

pub(in crate::mcp) fn mcp_sanitized_value(mut value: Value) -> Value {
	sanitize_mcp_observability_value(&mut value);

	value
}

fn observability_string_contains_sensitive_text(value: &str) -> bool {
	let lower = value.to_ascii_lowercase();
	let upper = value.to_ascii_uppercase();

	lower.contains("/private")
		|| lower.contains("/users/")
		|| lower.contains("/var/folders/")
		|| lower.contains("/tmp/")
		|| lower.contains("file://")
		|| observability_string_contains_absolute_path(value)
		|| observability_string_contains_windows_path(value)
		|| observability_string_contains_secret_like_token(value)
		|| upper.contains("GITHUB_PAT_")
		|| upper.contains("LINEAR_API_KEY")
		|| upper.contains("OPENAI_API_KEY")
		|| lower.contains("authorization:")
		|| lower.contains("bearer ")
		|| lower.contains("token=")
		|| lower.contains("api_key")
}

fn observability_string_contains_absolute_path(value: &str) -> bool {
	let mut previous = None;
	let mut chars = value.char_indices().peekable();

	while let Some((index, character)) = chars.next() {
		if character != '/' {
			previous = Some(character);

			continue;
		}
		if previous == Some(':') || previous == Some('/') {
			previous = Some(character);

			continue;
		}

		let path_boundary = index == 0
			|| previous.is_some_and(|previous| {
				previous.is_whitespace()
					|| matches!(previous, '"' | '\'' | '`' | '(' | '[' | '{' | '=')
			});
		let path_component = chars
			.peek()
			.map(|(_, next)| next.is_ascii_alphanumeric() || matches!(next, '.' | '_' | '-'))
			.unwrap_or(false);

		if path_boundary && path_component {
			return true;
		}

		previous = Some(character);
	}

	false
}

fn observability_string_contains_windows_path(value: &str) -> bool {
	let bytes = value.as_bytes();

	bytes.windows(3).enumerate().any(|(index, window)| {
		let boundary = index == 0 || {
			let previous = bytes[index - 1];

			previous.is_ascii_whitespace()
				|| matches!(previous, b'"' | b'\'' | b'`' | b'(' | b'[' | b'{' | b'=')
		};

		boundary
			&& window[0].is_ascii_alphabetic()
			&& window[1] == b':'
			&& matches!(window[2], b'\\' | b'/')
	})
}

fn observability_string_contains_secret_like_token(value: &str) -> bool {
	value
		.split(|character: char| {
			!(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':' | '/'))
		})
		.any(|token| {
			let lower = token.to_ascii_lowercase();

			(lower.starts_with("ghp_") && token.len() >= 20)
				|| (lower.starts_with("github_pat_") && token.len() >= 20)
				|| (lower.starts_with("sk-") && token.len() >= 20)
				|| (lower.starts_with("sk-proj-") && token.len() >= 20)
				|| (lower.starts_with("xoxb-") && token.len() >= 20)
				|| (lower.starts_with("xoxp-") && token.len() >= 20)
				|| observability_token_looks_high_entropy_secret(token)
				|| observability_token_looks_like_jwt(token)
		})
}

fn observability_token_looks_high_entropy_secret(token: &str) -> bool {
	if token.len() < 32 || token.len() > 256 {
		return false;
	}
	if !token.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) {
		return false;
	}

	let mut has_lower = false;
	let mut has_upper = false;
	let mut digit_count = 0_usize;
	let mut seen = [false; 128];
	let mut unique_count = 0_usize;

	for byte in token.bytes() {
		has_lower |= byte.is_ascii_lowercase();
		has_upper |= byte.is_ascii_uppercase();

		if byte.is_ascii_digit() {
			digit_count += 1;
		}
		if byte.is_ascii() && !seen[byte as usize] {
			seen[byte as usize] = true;
			unique_count += 1;
		}
	}

	has_lower && has_upper && digit_count >= 4 && unique_count >= 16
}

fn observability_token_looks_like_jwt(token: &str) -> bool {
	let mut segments = token.split('.');
	let Some(header) = segments.next() else {
		return false;
	};
	let Some(payload) = segments.next() else {
		return false;
	};
	let Some(signature) = segments.next() else {
		return false;
	};

	segments.next().is_none()
		&& header.starts_with("eyJ")
		&& payload.len() >= 16
		&& signature.len() >= 16
}
