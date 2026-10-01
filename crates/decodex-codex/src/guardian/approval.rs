//! Explicit public-to-core action conversion for the native denial approval RPC.
use std::{num::NonZeroUsize, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::{
	app_server_client::AppServerClient,
	guardian::{self, GuardianReview, ReviewStatus},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum CommandSource {
	Shell,
	UnifiedExec,
}
impl CommandSource {
	fn core(&self) -> &'static str {
		match self {
			Self::Shell => "shell",
			Self::UnifiedExec => "unified_exec",
		}
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum NetworkProtocol {
	Http,
	Https,
	Socks5Tcp,
	Socks5Udp,
}
impl NetworkProtocol {
	fn core(&self) -> &'static str {
		match self {
			Self::Http => "http",
			Self::Https => "https",
			Self::Socks5Tcp => "socks5_tcp",
			Self::Socks5Udp => "socks5_udp",
		}
	}
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Action {
	Command {
		source: CommandSource,
		command: String,
		cwd: String,
	},
	Execve {
		source: CommandSource,
		program: String,
		argv: Vec<String>,
		cwd: String,
	},
	#[serde(rename_all = "camelCase")]
	WriteStdin {
		approval_id: String,
		process_id: String,
		stdin: String,
		cwd: String,
	},
	ApplyPatch {
		cwd: String,
		files: Vec<String>,
	},
	NetworkAccess {
		target: String,
		host: String,
		protocol: NetworkProtocol,
		port: u16,
	},
	#[serde(rename_all = "camelCase")]
	McpToolCall {
		server: String,
		tool_name: String,
		connector_id: Option<String>,
		connector_name: Option<String>,
		tool_title: Option<String>,
	},
	RequestPermissions {
		reason: Option<String>,
		permissions: Permissions,
	},
}
impl Action {
	fn into_core(self) -> Option<Value> {
		Some(match self {
			Self::Command { source, command, cwd } =>
				serde_json::json!({"type":"command","source":source.core(),"command":command,"cwd":cwd}),
			Self::Execve { source, program, argv, cwd } => {
				if !Path::new(&cwd).is_absolute() {
					return None;
				}

				serde_json::json!({"type":"execve","source":source.core(),"program":program,"argv":argv,"cwd":cwd})
			},
			Self::WriteStdin { approval_id, process_id, stdin, cwd } =>
				serde_json::json!({"type":"write_stdin","approval_id":approval_id,"process_id":process_id,"stdin":stdin,"cwd":native_path_uri(&cwd)?}),
			Self::ApplyPatch { cwd, files } =>
				serde_json::json!({"type":"apply_patch","cwd":cwd,"files":files}),
			Self::NetworkAccess { target, host, protocol, port } =>
				serde_json::json!({"type":"network_access","target":target,"host":host,"protocol":protocol.core(),"port":port}),
			Self::McpToolCall { server, tool_name, connector_id, connector_name, tool_title } =>
				serde_json::json!({"type":"mcp_tool_call","server":server,"tool_name":tool_name,"connector_id":connector_id,"connector_name":connector_name,"tool_title":tool_title}),
			Self::RequestPermissions { reason, permissions } =>
				serde_json::json!({"type":"request_permissions","reason":reason,"permissions":permissions.into_core()?}),
		})
	}
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Access {
	Read,
	Write,
	Deny,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum PermissionPath {
	Path { path: String },
	GlobPattern { pattern: String },
	Special { value: SpecialPath },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SpecialPath {
	Root,
	Minimal,
	Tmpdir,
	SlashTmp,
	#[serde(alias = "current_working_directory")]
	ProjectRoots {
		subpath: Option<String>,
	},
	Unknown {
		path: String,
		subpath: Option<String>,
	},
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Permissions {
	network: Option<Network>,
	file_system: Option<FileSystem>,
}
impl Permissions {
	fn into_core(self) -> Option<Value> {
		let filesystem = self.file_system.map(|fs| {
			// Public entries is authoritative, even when empty. Never merge the
			// legacy read/write mirrors into the explicit permission entry list.
			let entries = fs.entries.unwrap_or_else(|| {
				fs.read
					.into_iter()
					.flatten()
					.map(|path| Entry { path: PermissionPath::Path { path }, access: Access::Read })
					.chain(fs.write.into_iter().flatten().map(|path| Entry {
						path: PermissionPath::Path { path },
						access: Access::Write,
					}))
					.collect()
			});
			let mut output = Vec::with_capacity(entries.len());

			for entry in entries {
				if let PermissionPath::Path { path } = &entry.path {
					// Core RawFileSystemPath uses the native string, not a file URI.
					// Validate the supported host path but retain its exact spelling.
					native_path_uri(path)?;
				}

				output.push(serde_json::json!({"path":entry.path,"access":entry.access}));
			}

			Some(serde_json::json!({"entries":output,"glob_scan_max_depth":fs.glob_scan_max_depth}))
		});
		let filesystem = match filesystem {
			Some(value) => Some(value?),
			None => None,
		};

		Some(
			serde_json::json!({"network":self.network.map(|n|serde_json::json!({"enabled":n.enabled})),"file_system":filesystem}),
		)
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Network {
	enabled: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileSystem {
	read: Option<Vec<String>>,
	write: Option<Vec<String>>,
	entries: Option<Vec<Entry>>,
	glob_scan_max_depth: Option<NonZeroUsize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
	path: PermissionPath,
	access: Access,
}

/// Convert an observed denial without discarding unknown approval-relevant data.
/// This only builds a payload; the caller must bind it to a saved observation and
/// an explicit user decision. RPC success is submission, not action execution.
pub fn core_denial_event(observation: &GuardianReview) -> Option<Value> {
	let review = guardian::decode_review("item/autoApprovalReview/completed", &observation.event)?;

	if review.status != ReviewStatus::Denied {
		return None;
	}

	let event = &review.event;

	if !known_keys(
		event,
		&[
			"threadId",
			"turnId",
			"reviewId",
			"targetItemId",
			"startedAtMs",
			"completedAtMs",
			"decisionSource",
			"review",
			"action",
		],
	) || !known_keys(
		&event["review"],
		&["status", "riskLevel", "userAuthorization", "rationale"],
	) {
		return None;
	}

	let action: Action = serde_json::from_value(event["action"].clone()).ok()?;
	let converted = serde_json::json!({
		"id":review.review_id,"target_item_id":review.target_item_id,
		"turn_id":review.turn_id,"started_at_ms":review.started_at_ms,
		"completed_at_ms":review.completed_at_ms,"status":"denied",
		"risk_level":event["review"]["riskLevel"],
		"user_authorization":event["review"]["userAuthorization"],
		"rationale":event["review"]["rationale"],"decision_source":"agent",
		"action":action.into_core()?
	});

	// Check the complete native frame before the service reserves a submission.
	AppServerClient::preflight_request(
		"thread/approveGuardianDeniedAction",
		&serde_json::json!({"threadId":review.thread_id,"event":&converted}),
	)
	.ok()?;

	Some(converted)
}

fn known_keys(value: &Value, allowed: &[&str]) -> bool {
	value.as_object().is_some_and(|map| map.keys().all(|key| allowed.contains(&key.as_str())))
}

// Decodex's native app-server runs on the same POSIX host. Normalize components
// like upstream LegacyAppPathString -> PathUri, preserving literal percent signs,
// non-ASCII text and separators. Do not guess at ambiguous or foreign spellings.
fn native_path_uri(path: &str) -> Option<String> {
	let tail = path.strip_prefix('/')?;

	if path.contains('\0') {
		return None;
	}

	let mut parts = Vec::new();
	let mut trailing = false;

	for part in tail.split('/') {
		match part {
			"" => trailing = true,
			"." => trailing = false,
			".." => {
				parts.pop();

				trailing = false;
			},
			part => {
				parts.push(part);

				trailing = false;
			},
		}
	}

	// Upstream uses an opaque fallback for POSIX paths that resemble a drive.
	// They are not losslessly representable by the ordinary conversion here.
	if parts.first().is_some_and(|part| {
		let bytes = part.as_bytes();

		bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
	}) {
		return None;
	}
	if trailing {
		parts.push("");
	}

	let mut uri = Url::parse("file:///").ok()?;

	uri.path_segments_mut().ok()?.clear().extend(parts);

	Some(uri.to_string())
}

#[cfg(test)]
mod tests {
	use crate::{
		guardian,
		guardian::approval::{self, Value},
	};
	#[test]
	fn a_retained_review_that_cannot_fit_its_approval_frame_is_not_submittable() {
		let mut event = serde_json::json!({"threadId":"thread","turnId":"turn","reviewId":"review","targetItemId":null,"startedAtMs":1,"completedAtMs":2,"decisionSource":"agent","review":{"status":"denied","riskLevel":"high","userAuthorization":"low","rationale":"exact rationale"},"action":{"type":"command","source":"shell","command":"","cwd":"/tmp"}});
		let overhead = serde_json::to_vec(&event).unwrap().len();

		event["action"]["command"] =
			serde_json::json!("x".repeat(crate::guardian::MAX_REVIEW_BYTES - overhead - 1));

		let observed =
			guardian::decode_review("item/autoApprovalReview/completed", &event).unwrap();

		assert!(approval::core_denial_event(&observed).is_none());
	}
	fn convert(action: Value) -> Option<Value> {
		let event = serde_json::json!({"threadId":"thread","turnId":"turn","reviewId":"review","targetItemId":null,"startedAtMs":1,"completedAtMs":2,"decisionSource":"agent","review":{"status":"denied","riskLevel":"high","userAuthorization":"low","rationale":"exact rationale"},"action":action});

		approval::core_denial_event(&guardian::decode_review(
			"item/autoApprovalReview/completed",
			&event,
		)?)
	}
	#[test]
	fn maps_all_native_actions_without_rewriting_arbitrary_strings() {
		let cases = [
			(
				serde_json::json!({"type":"command","source":"unifiedExec","command":"echo toolName","cwd":"/tmp"}),
				serde_json::json!({"type":"command","source":"unified_exec","command":"echo toolName","cwd":"/tmp"}),
			),
			(
				serde_json::json!({"type":"execve","source":"shell","program":"/bin/echo","argv":["echo","toolName"],"cwd":"/tmp"}),
				serde_json::json!({"type":"execve","source":"shell","program":"/bin/echo","argv":["echo","toolName"],"cwd":"/tmp"}),
			),
			(
				serde_json::json!({"type":"writeStdin","approvalId":"child","processId":"terminal","stdin":"toolName\n","cwd":"/tmp/a #/%/中文/"}),
				serde_json::json!({"type":"write_stdin","approval_id":"child","process_id":"terminal","stdin":"toolName\n","cwd":"file:///tmp/a%20%23/%25/%E4%B8%AD%E6%96%87/"}),
			),
			(
				serde_json::json!({"type":"applyPatch","cwd":"/tmp","files":["/tmp/toolName"]}),
				serde_json::json!({"type":"apply_patch","cwd":"/tmp","files":["/tmp/toolName"]}),
			),
			(
				serde_json::json!({"type":"networkAccess","target":"target","host":"example.test","protocol":"socks5Tcp","port":443}),
				serde_json::json!({"type":"network_access","target":"target","host":"example.test","protocol":"socks5_tcp","port":443}),
			),
			(
				serde_json::json!({"type":"mcpToolCall","server":"server","toolName":"toolName","connectorId":"connector","connectorName":null,"toolTitle":"Title"}),
				serde_json::json!({"type":"mcp_tool_call","server":"server","tool_name":"toolName","connector_id":"connector","connector_name":null,"tool_title":"Title"}),
			),
			(
				serde_json::json!({"type":"requestPermissions","reason":"toolName","permissions":{"network":{"enabled":true},"fileSystem":{"read":["/tmp/a"],"write":["/tmp/b"]}}}),
				serde_json::json!({"type":"request_permissions","reason":"toolName","permissions":{"network":{"enabled":true},"file_system":{"entries":[{"path":{"type":"path","path":"/tmp/a"},"access":"read"},{"path":{"type":"path","path":"/tmp/b"},"access":"write"}],"glob_scan_max_depth":null}}}),
			),
		];

		for (input, expected) in cases {
			let result = convert(input).unwrap();

			assert_eq!(result["action"], expected);
			assert_eq!(result["status"], "denied");
			assert_eq!(result["rationale"], "exact rationale");
			assert_eq!(result["id"], "review");
			assert!(result.get("threadId").is_none());
		}
	}

	#[test]
	fn explicit_permission_entries_preserve_denies_globs_and_unknown_special_paths() {
		let entries = serde_json::json!([
			{"path":{"type":"path","path":"/tmp/private"},"access":"deny"},
			{"path":{"type":"glob_pattern","pattern":"**/toolName"},"access":"read"},
			{"path":{"type":"special","value":{"kind":"unknown","path":":future","subpath":"toolName"}},"access":"write"}
		]);
		let mut action = serde_json::json!({"type":"requestPermissions","reason":null,"permissions":{"fileSystem":{"read":["/ignored"],"write":["/also-ignored"],"entries":entries,"globScanMaxDepth":3}}});
		let core = convert(action.clone()).unwrap();

		assert_eq!(
			core["action"]["permissions"]["file_system"],
			serde_json::json!({"entries":entries,"glob_scan_max_depth":3})
		);

		action["permissions"]["fileSystem"]["entries"] = serde_json::json!([]);

		assert_eq!(
			convert(action.clone()).unwrap()["action"]["permissions"]["file_system"]["entries"],
			serde_json::json!([])
		);

		action["permissions"]["fileSystem"]["globScanMaxDepth"] = serde_json::json!(0);

		assert!(convert(action).is_none());
	}

	#[test]
	fn refuses_unknown_fields_variants_and_lossy_path_conversion() {
		for action in [
			serde_json::json!({"type":"command","source":"shell","command":"echo x","cwd":"/tmp","futurePolicy":true}),
			serde_json::json!({"type":"futureAction"}),
			serde_json::json!({"type":"requestPermissions","permissions":{"futurePermission":true}}),
			serde_json::json!({"type":"writeStdin","approvalId":"a","processId":"p","stdin":"x","cwd":"relative"}),
		] {
			assert!(convert(action).is_none());
		}

		assert_eq!(approval::native_path_uri("/tmp/a/.././b//"), Some("file:///tmp/b/".into()));
		assert_eq!(approval::native_path_uri("/C:/tmp"), None);
		assert_eq!(approval::native_path_uri("/tmp/\0"), None);
	}
}
