use std::{
	collections::{BTreeMap, BTreeSet},
	fs::{self, File},
	io::{ErrorKind, Read as _},
	path::{Path, PathBuf},
};

use serde::{
	Deserialize, Serialize, Serializer,
	ser::{SerializeMap as _, SerializeSeq as _},
};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

/// Accepted XY-1262 schema receipt used by the marker golden.
pub const ACCEPTED_SCHEMA_RECEIPT: &str = "decodex/vnext-codex-schema-receipt/1";
/// Request methods required by the accepted schema receipt.
pub const REQUIRED_REQUEST_METHODS: &[&str] = &[
	"initialize",
	"account/read",
	"thread/start",
	"thread/list",
	"thread/resume",
	"thread/name/set",
	"turn/start",
	"account/rateLimits/read",
	"collaborationMode/list",
];
/// Notification methods required by the accepted schema receipt.
pub const REQUIRED_NOTIFICATION_METHODS: &[&str] =
	&["thread/started", "turn/started", "item/started", "item/completed", "turn/completed"];
#[doc(hidden)]
pub const MAX_SCHEMA_FILE_BYTES: u64 = 16 * 1_024 * 1_024;

pub(crate) const MAX_SCHEMA_FILES: usize = 512;
pub(crate) const MAX_SCHEMA_TOTAL_BYTES: u64 = 32 * 1_024 * 1_024;

const COLLABORATION_MARKERS: &[&str] =
	&["collabAgentToolCall", "parentThreadId", "agentNickname", "agentRole", "subAgentActivity"];
const MAX_SCHEMA_DIRECTORY_DEPTH: usize = 8;
const ACCEPTED_DIGESTS: &[(&str, &str)] = &[
	("ClientRequest.json", "3f82e5aec5be786c40d21440dfb6d0667d194d872bfa7041bd81c39b4ba56dc3"),
	("ServerNotification.json", "16ce6adadf33aa182f98840c5d33f6294c3c37b2866bb05545c24e0dbf2cc2d2"),
	(
		"codex_app_server_protocol.v2.schemas.json",
		"f5e8d20f3a8f9bb5e5b23ab0c5aa6bde7b12e7e0713606c5d0132651a4959d37",
	),
];

/// One checked-in schema marker set, never a capability promise.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaMarker {
	/// Receipt schema identifier.
	receipt: String,
	/// Exact request method markers.
	request_methods: BTreeSet<String>,
	/// Exact notification method markers.
	notification_methods: BTreeSet<String>,
	/// Collaboration field markers from `ThreadReadResponse`.
	collaboration_markers: BTreeSet<String>,
	/// Whether the accepted thread-start schema includes paginated history.
	paginated_history: bool,
	/// Canonical hashes recorded by the accepted XY-1262 receipt.
	canonical_sha256: BTreeMap<String, String>,
}
impl SchemaMarker {
	/// Load the accepted checked-in marker golden.
	pub fn accepted() -> Self {
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct SchemaMarkerWire {
			receipt: String,
			request_methods: BTreeSet<String>,
			notification_methods: BTreeSet<String>,
			collaboration_markers: BTreeSet<String>,
			paginated_history: bool,
			canonical_sha256: BTreeMap<String, String>,
		}

		let marker: SchemaMarkerWire =
			serde_json::from_str(include_str!("../schema/xy-1262-markers.json"))
				.expect("checked-in schema marker golden must be valid");

		Self {
			receipt: marker.receipt,
			request_methods: marker.request_methods,
			notification_methods: marker.notification_methods,
			collaboration_markers: marker.collaboration_markers,
			paginated_history: marker.paginated_history,
			canonical_sha256: marker.canonical_sha256,
		}
	}

	#[doc(hidden)]
	pub fn canonical_digests(&self) -> &BTreeMap<String, String> {
		&self.canonical_sha256
	}
}

/// Validated generated-schema evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaContract {
	request_methods: BTreeSet<String>,
	paginated_history: bool,
	native_collaboration: bool,
}
impl SchemaContract {
	/// Validate all required markers before any process or side effect is started.
	pub fn validate(marker: SchemaMarker) -> Result<Self, Vec<String>> {
		let mut missing = Vec::new();

		if marker.receipt != ACCEPTED_SCHEMA_RECEIPT {
			missing.push(format!("receipt:{ACCEPTED_SCHEMA_RECEIPT}"));
		}

		for method in REQUIRED_REQUEST_METHODS {
			if !marker.request_methods.contains(*method) {
				missing.push(format!("request:{method}"));
			}
		}
		for method in REQUIRED_NOTIFICATION_METHODS {
			if !marker.notification_methods.contains(*method) {
				missing.push(format!("notification:{method}"));
			}
		}
		for field in COLLABORATION_MARKERS {
			if !marker.collaboration_markers.contains(*field) {
				missing.push(format!("collaboration:{field}"));
			}
		}
		for (file, digest) in ACCEPTED_DIGESTS {
			if marker.canonical_sha256.get(*file).map(String::as_str) != Some(*digest) {
				missing.push(format!("digest:{file}"));
			}
		}

		if missing.is_empty() {
			Ok(Self {
				request_methods: marker.request_methods,
				paginated_history: marker.paginated_history,
				native_collaboration: true,
			})
		} else {
			Err(missing)
		}
	}

	pub(crate) fn from_generated(
		request_methods: BTreeSet<String>,
		notification_methods: BTreeSet<String>,
		paginated_history: bool,
		native_collaboration: bool,
	) -> Result<Self, Vec<String>> {
		let mut missing = Vec::new();

		for method in REQUIRED_REQUEST_METHODS {
			if !request_methods.contains(*method) {
				missing.push(format!("request:{method}"));
			}
		}
		for method in REQUIRED_NOTIFICATION_METHODS {
			if !notification_methods.contains(*method) {
				missing.push(format!("notification:{method}"));
			}
		}

		if missing.is_empty() {
			Ok(Self { request_methods, paginated_history, native_collaboration })
		} else {
			Err(missing)
		}
	}

	/// Return whether the schema advertises an exact request method.
	pub fn advertises_request(&self, method: &str) -> bool {
		self.request_methods.contains(method)
	}

	/// Return whether an accepted collaboration marker is present.
	pub fn advertises_collaboration(&self) -> bool {
		self.native_collaboration
	}

	/// Return whether thread-start structurally advertises paginated history.
	pub fn advertises_paginated_history(&self) -> bool {
		self.paginated_history
	}
}

/// Evidence derived from schema files generated by the exact executable being probed.
#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct GeneratedSchemaEvidence {
	pub fingerprint: String,
	contract: SchemaContract,
}
impl GeneratedSchemaEvidence {
	pub fn load(
		directory: &Path,
		expected_digests: Option<&BTreeMap<String, String>>,
	) -> Result<Self, Vec<String>> {
		validate_generated_directory_budget(directory)?;

		let request = read_json(directory.join("ClientRequest.json"))?;
		let notification = read_json(directory.join("ServerNotification.json"))?;
		let aggregate_path = directory.join("codex_app_server_protocol.v2.schemas.json");
		let aggregate = read_json(&aggregate_path)?;
		let collaboration = read_optional_json(directory.join("v2/ThreadReadResponse.json"))?;
		let thread_start = read_optional_json(directory.join("v2/ThreadStartParams.json"))?;
		let request_methods = extract_methods(&request)?;
		let notification_methods = extract_methods(&notification)?;
		let native_collaboration = collaboration
			.as_ref()
			.is_some_and(|schema| validate_collaboration_schema(schema).is_ok());
		let paginated_history =
			thread_start.as_ref().is_some_and(validate_paginated_history_schema);
		let mut actual_digests = [
			("ClientRequest.json", &request),
			("ServerNotification.json", &notification),
			("codex_app_server_protocol.v2.schemas.json", &aggregate),
		]
		.into_iter()
		.map(|(name, value)| (name.to_owned(), canonical_digest(value)))
		.collect::<BTreeMap<_, _>>();

		if let Some(value) = collaboration.as_ref() {
			actual_digests.insert("v2/ThreadReadResponse.json".into(), canonical_digest(value));
		}
		if let Some(value) = thread_start.as_ref() {
			actual_digests.insert("v2/ThreadStartParams.json".into(), canonical_digest(value));
		}
		if let Some(expected) = expected_digests {
			let mismatches = expected
				.iter()
				.filter(|(name, digest)| actual_digests.get(*name) != Some(*digest))
				.map(|(name, _)| format!("digest:{name}"))
				.collect::<Vec<_>>();

			if !mismatches.is_empty() {
				return Err(mismatches);
			}
		}

		let contract = SchemaContract::from_generated(
			request_methods,
			notification_methods,
			paginated_history,
			native_collaboration,
		)?;
		let fingerprint = canonical_digest(
			&serde_json::to_value(actual_digests).expect("schema digest map must serialize"),
		);

		validate_generated_directory_budget(directory)?;

		Ok(Self { fingerprint, contract })
	}

	pub fn contract(&self) -> &SchemaContract {
		&self.contract
	}
}

struct CanonicalJson<'a>(&'a Value);
impl Serialize for CanonicalJson<'_> {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		match self.0 {
			Value::Null => serializer.serialize_unit(),
			Value::Bool(value) => serializer.serialize_bool(*value),
			Value::Number(value) => value.serialize(serializer),
			Value::String(value) => serializer.serialize_str(value),
			Value::Array(values) => {
				let mut sequence = serializer.serialize_seq(Some(values.len()))?;

				for value in values {
					sequence.serialize_element(&Self(value))?;
				}

				sequence.end()
			},
			Value::Object(values) => {
				let mut entries = values.iter().collect::<Vec<_>>();

				entries.sort_unstable_by_key(|(key, _)| *key);

				let mut map = serializer.serialize_map(Some(entries.len()))?;

				for (key, value) in entries {
					map.serialize_entry(key, &Self(value))?;
				}

				map.end()
			},
		}
	}
}

#[derive(Clone, Copy)]
enum PropertyShape {
	String,
	StringArray,
	Reference(&'static str),
	ReferenceMap(&'static str),
}

pub(crate) fn validate_generated_directory_budget(directory: &Path) -> Result<(), Vec<String>> {
	let root_metadata =
		directory.symlink_metadata().map_err(|_| vec!["schema:directory".into()])?;

	if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
		return Err(vec!["schema:directory".into()]);
	}

	let mut pending = vec![(directory.to_owned(), 0_usize)];
	let mut file_count = 0_usize;
	let mut total_bytes = 0_u64;

	while let Some((path, depth)) = pending.pop() {
		if depth > MAX_SCHEMA_DIRECTORY_DEPTH {
			return Err(vec!["schema:directory-depth-limit".into()]);
		}

		let entries = fs::read_dir(path).map_err(|_| vec!["schema:directory".into()])?;

		for entry in entries {
			let entry = entry.map_err(|_| vec!["schema:directory".into()])?;
			let metadata =
				entry.path().symlink_metadata().map_err(|_| vec!["schema:directory".into()])?;

			if metadata.file_type().is_symlink() {
				return Err(vec!["schema:symlink".into()]);
			}
			if metadata.is_dir() {
				pending.push((entry.path(), depth + 1));

				continue;
			}
			if !metadata.is_file() {
				return Err(vec!["schema:file-type".into()]);
			}

			file_count =
				file_count.checked_add(1).ok_or_else(|| vec!["schema:file-count-limit".into()])?;
			total_bytes = total_bytes
				.checked_add(metadata.len())
				.ok_or_else(|| vec!["schema:directory-byte-limit".into()])?;

			if file_count > MAX_SCHEMA_FILES {
				return Err(vec!["schema:file-count-limit".into()]);
			}
			if metadata.len() > MAX_SCHEMA_FILE_BYTES || total_bytes > MAX_SCHEMA_TOTAL_BYTES {
				return Err(vec!["schema:directory-byte-limit".into()]);
			}
		}
	}

	Ok(())
}

pub(crate) fn hex_digest(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn extract_methods(value: &Value) -> Result<BTreeSet<String>, Vec<String>> {
	let variants =
		value.get("oneOf").and_then(Value::as_array).ok_or_else(|| vec!["schema:oneOf".into()])?;
	let methods = variants
		.iter()
		.filter_map(|variant| variant.get("properties")?.get("method")?.get("enum")?.as_array())
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect::<BTreeSet<_>>();

	if methods.is_empty() { Err(vec!["schema:method-enum".into()]) } else { Ok(methods) }
}

fn canonical_digest(value: &Value) -> String {
	let bytes = serde_json::to_vec(&CanonicalJson(value)).expect("parsed JSON must serialize");

	hex_digest(Sha256::digest(bytes).as_ref())
}

fn read_json(path: impl AsRef<Path>) -> Result<Value, Vec<String>> {
	let path = path.as_ref();
	let path_metadata = path.symlink_metadata().map_err(|_| vec!["schema:document".into()])?;

	if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
		return Err(vec!["schema:file-type".into()]);
	}

	let file = File::open(path).map_err(|_| vec!["schema:document".into()])?;
	let size = file.metadata().map_err(|_| vec!["schema:document".into()])?.len();

	if size > MAX_SCHEMA_FILE_BYTES {
		return Err(vec!["schema:document-limit".into()]);
	}

	let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));

	file.take(MAX_SCHEMA_FILE_BYTES + 1)
		.read_to_end(&mut bytes)
		.map_err(|_| vec!["schema:document".into()])?;

	if bytes.len() as u64 > MAX_SCHEMA_FILE_BYTES {
		return Err(vec!["schema:document-limit".into()]);
	}

	serde_json::from_slice(&bytes).map_err(|_| vec!["schema:document".into()])
}

fn read_optional_json(path: PathBuf) -> Result<Option<Value>, Vec<String>> {
	match path.symlink_metadata() {
		Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() =>
			Err(vec!["schema:file-type".into()]),
		Ok(_) => Ok(read_json(path).ok()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(_) => Err(vec!["schema:document".into()]),
	}
}

fn validate_paginated_history_schema(value: &Value) -> bool {
	let Some(definitions) = value.get("definitions").and_then(Value::as_object) else {
		return false;
	};
	let Some(properties) = value.get("properties").and_then(Value::as_object) else {
		return false;
	};
	let Some(history_mode) = properties.get("historyMode") else {
		return false;
	};
	let Some(values) = definitions
		.get("ThreadHistoryMode")
		.and_then(|schema| schema.get("enum"))
		.and_then(Value::as_array)
	else {
		return false;
	};

	references(history_mode, "ThreadHistoryMode")
		&& values.iter().any(|value| value.as_str() == Some("legacy"))
		&& values.iter().any(|value| value.as_str() == Some("paginated"))
}

fn validate_collaboration_schema(value: &Value) -> Result<(), Vec<String>> {
	let definitions = value
		.get("definitions")
		.and_then(Value::as_object)
		.ok_or_else(|| vec!["collaboration:definitions".into()])?;
	let thread = definitions.get("Thread").ok_or_else(|| vec!["collaboration:Thread".into()])?;
	let thread_properties = properties(thread, "Thread")?;

	for field in ["parentThreadId", "agentNickname", "agentRole"] {
		let schema = thread_properties
			.get(field)
			.ok_or_else(|| vec![format!("collaboration:Thread.{field}")])?;

		if !allows_string_or_null(schema) {
			return Err(vec![format!("collaboration:Thread.{field}:type")]);
		}
	}

	let variants = definitions
		.get("ThreadItem")
		.and_then(|schema| schema.get("oneOf"))
		.and_then(Value::as_array)
		.ok_or_else(|| vec!["collaboration:ThreadItem.oneOf".into()])?;

	validate_string_enum(
		definitions,
		"CollabAgentTool",
		&["spawnAgent", "sendInput", "resumeAgent", "wait", "closeAgent"],
	)?;
	validate_string_enum(
		definitions,
		"CollabAgentToolCallStatus",
		&["inProgress", "completed", "failed"],
	)?;
	validate_string_enum(
		definitions,
		"SubAgentActivityKind",
		&["started", "interacted", "interrupted"],
	)?;

	if !definitions.contains_key("CollabAgentState") {
		return Err(vec!["collaboration:CollabAgentState".into()]);
	}

	validate_item_variant(
		variants,
		"collabAgentToolCall",
		&["agentsStates", "id", "receiverThreadIds", "senderThreadId", "status", "tool", "type"],
		&[
			("agentsStates", PropertyShape::ReferenceMap("CollabAgentState")),
			("id", PropertyShape::String),
			("receiverThreadIds", PropertyShape::StringArray),
			("senderThreadId", PropertyShape::String),
			("status", PropertyShape::Reference("CollabAgentToolCallStatus")),
			("tool", PropertyShape::Reference("CollabAgentTool")),
		],
	)?;
	validate_item_variant(
		variants,
		"subAgentActivity",
		&["agentPath", "agentThreadId", "id", "kind", "type"],
		&[
			("agentPath", PropertyShape::String),
			("agentThreadId", PropertyShape::String),
			("id", PropertyShape::String),
			("kind", PropertyShape::Reference("SubAgentActivityKind")),
		],
	)?;

	Ok(())
}

fn validate_string_enum(
	definitions: &Map<String, Value>,
	name: &str,
	required: &[&str],
) -> Result<(), Vec<String>> {
	let values = definitions
		.get(name)
		.and_then(|schema| schema.get("enum"))
		.and_then(Value::as_array)
		.ok_or_else(|| vec![format!("collaboration:{name}:enum")])?;

	if definitions.get(name).and_then(|schema| schema.get("type")).and_then(Value::as_str)
		!= Some("string")
		|| required
			.iter()
			.any(|required| !values.iter().any(|value| value.as_str() == Some(required)))
	{
		return Err(vec![format!("collaboration:{name}:enum")]);
	}

	Ok(())
}

fn validate_item_variant(
	variants: &[Value],
	kind: &str,
	required_fields: &[&str],
	property_shapes: &[(&str, PropertyShape)],
) -> Result<(), Vec<String>> {
	let variant = variants
		.iter()
		.find(|variant| {
			variant
				.get("properties")
				.and_then(|properties| properties.get("type"))
				.and_then(|schema| schema.get("enum"))
				.and_then(Value::as_array)
				.is_some_and(|values| values.as_slice() == [Value::String(kind.into())])
		})
		.ok_or_else(|| vec![format!("collaboration:ThreadItem.{kind}")])?;

	if variant.get("type").and_then(Value::as_str) != Some("object") {
		return Err(vec![format!("collaboration:ThreadItem.{kind}:type")]);
	}

	let required = variant
		.get("required")
		.and_then(Value::as_array)
		.ok_or_else(|| vec![format!("collaboration:ThreadItem.{kind}:required")])?;

	for field in required_fields {
		if !required.iter().any(|value| value.as_str() == Some(field)) {
			return Err(vec![format!("collaboration:ThreadItem.{kind}:required:{field}")]);
		}
	}

	let properties = properties(variant, kind)?;

	for (field, shape) in property_shapes {
		let schema = properties
			.get(*field)
			.ok_or_else(|| vec![format!("collaboration:ThreadItem.{kind}.{field}")])?;
		let valid = match shape {
			PropertyShape::String => schema.get("type").and_then(Value::as_str) == Some("string"),
			PropertyShape::StringArray =>
				schema.get("type").and_then(Value::as_str) == Some("array")
					&& schema
						.get("items")
						.and_then(|items| items.get("type"))
						.and_then(Value::as_str)
						== Some("string"),
			PropertyShape::Reference(name) => references(schema, name),
			PropertyShape::ReferenceMap(name) =>
				schema.get("type").and_then(Value::as_str) == Some("object")
					&& schema
						.get("additionalProperties")
						.is_some_and(|value| references(value, name)),
		};

		if !valid {
			return Err(vec![format!("collaboration:ThreadItem.{kind}.{field}:type")]);
		}
	}

	Ok(())
}

fn properties<'a>(value: &'a Value, owner: &str) -> Result<&'a Map<String, Value>, Vec<String>> {
	value
		.get("properties")
		.and_then(Value::as_object)
		.ok_or_else(|| vec![format!("collaboration:{owner}:properties")])
}

fn allows_string_or_null(value: &Value) -> bool {
	value.get("type").and_then(Value::as_array).is_some_and(|types| {
		types.len() == 2
			&& types.iter().any(|value| value.as_str() == Some("string"))
			&& types.iter().any(|value| value.as_str() == Some("null"))
	})
}

fn references(value: &Value, name: &str) -> bool {
	let expected = format!("#/definitions/{name}");

	value.get("$ref").and_then(Value::as_str) == Some(expected.as_str())
		|| ["allOf", "anyOf", "oneOf"].iter().any(|key| {
			value
				.get(key)
				.and_then(Value::as_array)
				.is_some_and(|values| values.iter().any(|value| references(value, name)))
		})
}

#[cfg(test)]
mod tests {
	use std::{
		fs::{self, File},
		os::unix::net::UnixListener,
	};

	use crate::schema::{REQUIRED_REQUEST_METHODS, SchemaContract, SchemaMarker};

	#[test]
	fn accepted_marker_golden_satisfies_the_xy_1262_contract() {
		let marker = SchemaMarker::accepted();
		let contract = SchemaContract::validate(marker).unwrap();

		for method in REQUIRED_REQUEST_METHODS {
			assert!(contract.advertises_request(method));
		}
		for method in ["thread/archive", "thread/search"] {
			assert!(contract.advertises_request(method));
		}

		assert!(contract.advertises_collaboration());
		assert!(contract.advertises_paginated_history());
	}

	#[test]
	fn marker_validation_reports_exact_missing_contract_members() {
		let mut marker = SchemaMarker::accepted();

		marker.notification_methods.remove("turn/completed");
		marker.collaboration_markers.remove("parentThreadId");

		let missing = SchemaContract::validate(marker).unwrap_err();

		assert_eq!(missing, ["notification:turn/completed", "collaboration:parentThreadId"]);
	}

	#[test]
	fn marker_validation_rejects_a_tampered_receipt_digest() {
		let mut marker = SchemaMarker::accepted();

		marker.canonical_sha256.insert("ClientRequest.json".into(), "wrong".into());

		assert_eq!(SchemaContract::validate(marker).unwrap_err(), ["digest:ClientRequest.json"]);
	}

	#[test]
	fn canonical_digest_ignores_json_object_order() {
		let first: serde_json::Value =
			serde_json::from_str(r#"{"b":2,"a":{"d":4,"c":3}}"#).unwrap();
		let second: serde_json::Value =
			serde_json::from_str(r#"{"a":{"c":3,"d":4},"b":2}"#).unwrap();

		assert_eq!(super::canonical_digest(&first), super::canonical_digest(&second));
		assert_eq!(
			super::canonical_digest(&first),
			"c461c47a913352f1a21e3f2ea49e1fd34754c0dc12cb7366e4636d5e186c6c6e"
		);
	}

	#[test]
	fn canonical_digest_preserves_json_array_order_and_scalar_values() {
		let baseline: serde_json::Value =
			serde_json::from_str(r#"{"array":[1,2],"scalar":true}"#).unwrap();
		let reordered_array: serde_json::Value =
			serde_json::from_str(r#"{"array":[2,1],"scalar":true}"#).unwrap();
		let changed_scalar: serde_json::Value =
			serde_json::from_str(r#"{"array":[1,2],"scalar":false}"#).unwrap();

		assert_ne!(super::canonical_digest(&baseline), super::canonical_digest(&reordered_array));
		assert_ne!(super::canonical_digest(&baseline), super::canonical_digest(&changed_scalar));
	}

	#[cfg(feature = "preserve-order-regression")]
	#[test]
	fn preserve_order_regression_configuration_retains_insertion_order() {
		let value: serde_json::Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
		let keys = value.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>();

		assert_eq!(keys, ["b", "a"]);
	}

	#[test]
	fn collaboration_markers_in_descriptions_or_unrelated_tokens_are_rejected() {
		let false_schema = serde_json::json!({
			"description": "collabAgentToolCall parentThreadId agentNickname agentRole subAgentActivity",
			"definitions": {
				"Thread": {"properties": {"unrelated": {"enum": ["parentThreadId", "agentNickname", "agentRole"]}}},
				"ThreadItem": {"oneOf": [{"description": "collabAgentToolCall subAgentActivity"}]}
			}
		});

		assert!(super::validate_collaboration_schema(&false_schema).is_err());
	}

	#[test]
	fn collaboration_variants_require_expected_fields_and_shapes() {
		let mut schema = structurally_valid_collaboration_schema();

		schema["definitions"]["ThreadItem"]["oneOf"][0]["properties"]["receiverThreadIds"]["items"]
			["type"] = serde_json::json!("number");

		assert_eq!(
			super::validate_collaboration_schema(&schema).unwrap_err(),
			["collaboration:ThreadItem.collabAgentToolCall.receiverThreadIds:type"]
		);
	}

	#[test]
	fn paginated_history_requires_the_exact_history_mode_shape() {
		let valid = serde_json::json!({
			"properties": {
				"historyMode": {"anyOf": [{"$ref": "#/definitions/ThreadHistoryMode"}, {"type": "null"}]}
			},
			"definitions": {
				"ThreadHistoryMode": {"type": "string", "enum": ["legacy", "paginated"]}
			}
		});
		let mut invalid = valid.clone();

		invalid["definitions"]["ThreadHistoryMode"]["enum"] = serde_json::json!(["legacy"]);

		assert!(super::validate_paginated_history_schema(&valid));
		assert!(!super::validate_paginated_history_schema(&invalid));
	}

	#[test]
	fn generated_schema_directory_file_count_is_bounded() {
		let directory = tempfile::tempdir().unwrap();

		for index in 0..=super::MAX_SCHEMA_FILES {
			fs::write(directory.path().join(format!("schema-{index}.json")), b"{}").unwrap();
		}

		assert_eq!(
			super::validate_generated_directory_budget(directory.path()).unwrap_err(),
			["schema:file-count-limit"]
		);
	}

	#[test]
	fn generated_schema_directory_bytes_and_file_bytes_are_bounded() {
		let aggregate = tempfile::tempdir().unwrap();

		for index in 0..3 {
			File::create(aggregate.path().join(format!("schema-{index}.json")))
				.unwrap()
				.set_len(super::MAX_SCHEMA_FILE_BYTES)
				.unwrap();
		}

		assert_eq!(
			super::validate_generated_directory_budget(aggregate.path()).unwrap_err(),
			["schema:directory-byte-limit"]
		);

		let per_file = tempfile::tempdir().unwrap();

		File::create(per_file.path().join("oversized.json"))
			.unwrap()
			.set_len(super::MAX_SCHEMA_FILE_BYTES + 1)
			.unwrap();

		assert_eq!(
			super::validate_generated_directory_budget(per_file.path()).unwrap_err(),
			["schema:directory-byte-limit"]
		);
	}

	#[test]
	fn generated_schema_depth_symlinks_and_special_files_are_rejected() {
		let deep = tempfile::tempdir().unwrap();
		let mut directory = deep.path().to_owned();

		for index in 0..=super::MAX_SCHEMA_DIRECTORY_DEPTH {
			directory = directory.join(format!("level-{index}"));

			fs::create_dir(&directory).unwrap();
		}

		assert_eq!(
			super::validate_generated_directory_budget(deep.path()).unwrap_err(),
			["schema:directory-depth-limit"]
		);

		let linked = tempfile::tempdir().unwrap();
		let target = linked.path().join("target.json");

		fs::write(&target, b"{}").unwrap();
		std::os::unix::fs::symlink(&target, linked.path().join("linked.json")).unwrap();

		assert_eq!(
			super::validate_generated_directory_budget(linked.path()).unwrap_err(),
			["schema:symlink"]
		);

		let special = tempfile::tempdir().unwrap();
		let _listener = UnixListener::bind(special.path().join("schema.sock")).unwrap();

		assert_eq!(
			super::validate_generated_directory_budget(special.path()).unwrap_err(),
			["schema:file-type"]
		);
	}

	fn structurally_valid_collaboration_schema() -> serde_json::Value {
		serde_json::json!({
			"definitions": {
				"CollabAgentState": {"type": "object"},
				"CollabAgentTool": {"type": "string", "enum": ["spawnAgent", "sendInput", "resumeAgent", "wait", "closeAgent"]},
				"CollabAgentToolCallStatus": {"type": "string", "enum": ["inProgress", "completed", "failed"]},
				"SubAgentActivityKind": {"type": "string", "enum": ["started", "interacted", "interrupted"]},
				"Thread": {"properties": {
					"parentThreadId": {"type": ["string", "null"]},
					"agentNickname": {"type": ["string", "null"]},
					"agentRole": {"type": ["string", "null"]}
				}},
				"ThreadItem": {"oneOf": [
					{
						"type": "object",
						"required": ["agentsStates", "id", "receiverThreadIds", "senderThreadId", "status", "tool", "type"],
						"properties": {
							"agentsStates": {"type": "object", "additionalProperties": {"$ref": "#/definitions/CollabAgentState"}},
							"id": {"type": "string"},
							"receiverThreadIds": {"type": "array", "items": {"type": "string"}},
							"senderThreadId": {"type": "string"},
							"status": {"allOf": [{"$ref": "#/definitions/CollabAgentToolCallStatus"}]},
							"tool": {"allOf": [{"$ref": "#/definitions/CollabAgentTool"}]},
							"type": {"enum": ["collabAgentToolCall"]}
						}
					},
					{
						"type": "object",
						"required": ["agentPath", "agentThreadId", "id", "kind", "type"],
						"properties": {
							"agentPath": {"type": "string"},
							"agentThreadId": {"type": "string"},
							"id": {"type": "string"},
							"kind": {"$ref": "#/definitions/SubAgentActivityKind"},
							"type": {"enum": ["subAgentActivity"]}
						}
					}
				]}
			}
		})
	}
}
