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

use crate::quick_task::{QuickTaskMethod, QuickTaskNotification};

/// Marker receipt retained for the checked-in structural fixture.
pub const ACCEPTED_SCHEMA_RECEIPT: &str = "decodex/vnext-codex-schema-receipt/1";
/// Request methods required by the current Decodex adapter.
pub const REQUIRED_REQUEST_METHODS: &[&str] = &[
	"initialize",
	"account/read",
	"thread/start",
	"thread/list",
	"thread/resume",
	"thread/name/set",
	"turn/start",
	"collaborationMode/list",
];
/// Notification methods required by the current Decodex adapter.
pub const REQUIRED_NOTIFICATION_METHODS: &[&str] =
	&["thread/started", "turn/started", "item/started", "item/completed", "turn/completed"];
/// Account-auth request initiated by a compatible Codex executable.
pub const ACCOUNT_LOGIN_METHOD: &str = "account/login/start";
/// Credential-refresh request initiated by a compatible Codex executable.
pub const ACCOUNT_REFRESH_CALLBACK_METHOD: &str = "account/chatgptAuthTokens/refresh";
#[doc(hidden)]
pub const MAX_SCHEMA_FILE_BYTES: u64 = 16 * 1_024 * 1_024;

pub(crate) const MAX_SCHEMA_FILES: usize = 512;
pub(crate) const MAX_SCHEMA_TOTAL_BYTES: u64 = 32 * 1_024 * 1_024;

const COLLABORATION_MARKERS: &[&str] =
	&["collabAgentToolCall", "parentThreadId", "agentNickname", "agentRole", "subAgentActivity"];
const MAX_SCHEMA_DIRECTORY_DEPTH: usize = 8;

/// One checked-in structural schema fixture, never a capability promise.
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
	notification_methods: BTreeSet<String>,
	paginated_history: bool,
	native_collaboration: bool,
}

/// One missing closed schema requirement for ordinary Quick Task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuickTaskSchemaRequirement {
	/// Required client request.
	Request(QuickTaskMethod),
	/// Required server notification.
	Notification(QuickTaskNotification),
}

/// Bounded schema gap that contains no generated schema or raw protocol text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskSchemaError {
	missing: Vec<QuickTaskSchemaRequirement>,
}
impl QuickTaskSchemaError {
	/// Return missing requirements in fixed contract order.
	pub fn missing(&self) -> &[QuickTaskSchemaRequirement] {
		&self.missing
	}
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
		if missing.is_empty() {
			Ok(Self {
				request_methods: marker.request_methods,
				notification_methods: marker.notification_methods,
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
			Ok(Self {
				request_methods,
				notification_methods,
				paginated_history,
				native_collaboration,
			})
		} else {
			Err(missing)
		}
	}

	/// Return whether the schema advertises an exact request method.
	pub fn advertises_request(&self, method: &str) -> bool {
		self.request_methods.contains(method)
	}

	/// Return whether the schema advertises an exact notification method.
	pub fn advertises_notification(&self, method: &str) -> bool {
		self.notification_methods.contains(method)
	}

	/// Check the complete ordinary Quick Task schema without granting dispatch authority.
	pub fn check_quick_task_contract(&self) -> Result<(), QuickTaskSchemaError> {
		let mut missing = Vec::new();

		for method in QuickTaskMethod::ALL {
			if !self.advertises_request(method.as_str()) {
				missing.push(QuickTaskSchemaRequirement::Request(method));
			}
		}
		for notification in QuickTaskNotification::ALL {
			if !self.advertises_notification(notification.as_str()) {
				missing.push(QuickTaskSchemaRequirement::Notification(notification));
			}
		}

		if missing.is_empty() { Ok(()) } else { Err(QuickTaskSchemaError { missing }) }
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
	pub fn load(directory: &Path) -> Result<Self, Vec<String>> {
		validate_generated_directory_budget(directory)?;

		let request = read_json(directory.join("ClientRequest.json"))?;
		let server_request = read_json(directory.join("ServerRequest.json"))?;
		let notification = read_json(directory.join("ServerNotification.json"))?;
		let aggregate_path = directory.join("codex_app_server_protocol.v2.schemas.json");
		let aggregate = read_json(&aggregate_path)?;
		let collaboration = read_optional_json(directory.join("v2/ThreadReadResponse.json"))?;
		let thread_start = read_optional_json(directory.join("v2/ThreadStartParams.json"))?;
		let login_params = read_json(directory.join("v2/LoginAccountParams.json"))?;
		let refresh_params = read_json(directory.join("ChatgptAuthTokensRefreshParams.json"))?;
		let refresh_response = read_json(directory.join("ChatgptAuthTokensRefreshResponse.json"))?;
		let request_methods = extract_methods(&request)?;
		let server_request_methods = extract_methods(&server_request)?;
		let notification_methods = extract_methods(&notification)?;
		validate_account_callback_contract(
			&request_methods,
			&server_request_methods,
			&login_params,
			&refresh_params,
			&refresh_response,
		)?;
		let native_collaboration = collaboration
			.as_ref()
			.is_some_and(|schema| validate_collaboration_schema(schema).is_ok());
		let paginated_history =
			thread_start.as_ref().is_some_and(validate_paginated_history_schema);
		let mut actual_digests = [
			("ClientRequest.json", &request),
			("ServerRequest.json", &server_request),
			("ServerNotification.json", &notification),
			("codex_app_server_protocol.v2.schemas.json", &aggregate),
			("v2/LoginAccountParams.json", &login_params),
			("ChatgptAuthTokensRefreshParams.json", &refresh_params),
			("ChatgptAuthTokensRefreshResponse.json", &refresh_response),
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

	/// Canonical generated-schema fingerprint bound into callback and ProcessGeneration facts.
	pub fn account_callback_profile_sha256(&self) -> &str {
		&self.fingerprint
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

fn validate_account_callback_contract(
	client_methods: &BTreeSet<String>,
	server_methods: &BTreeSet<String>,
	login: &Value,
	refresh_params: &Value,
	refresh_response: &Value,
) -> Result<(), Vec<String>> {
	let mut missing = Vec::new();
	if !client_methods.contains(ACCOUNT_LOGIN_METHOD) {
		missing.push(format!("request:{ACCOUNT_LOGIN_METHOD}"));
	}
	if !server_methods.contains(ACCOUNT_REFRESH_CALLBACK_METHOD) {
		missing.push(format!("server-request:{ACCOUNT_REFRESH_CALLBACK_METHOD}"));
	}
	if !has_tagged_object(
		login,
		"type",
		"chatgptAuthTokens",
		&["accessToken", "chatgptAccountId", "chatgptPlanType"],
		&["accessToken", "chatgptAccountId"],
	) {
		missing.push("schema:account-login-chatgpt-auth-tokens".into());
	}
	if !has_object_shape(refresh_params, &["reason", "previousAccountId"], &["reason"])
		|| !contains_enum_value(refresh_params, "unauthorized")
	{
		missing.push("schema:account-refresh-params".into());
	}
	if !has_object_shape(
		refresh_response,
		&["accessToken", "chatgptAccountId", "chatgptPlanType"],
		&["accessToken", "chatgptAccountId"],
	) {
		missing.push("schema:account-refresh-response".into());
	}

	if missing.is_empty() { Ok(()) } else { Err(missing) }
}

fn has_tagged_object(
	value: &Value,
	tag: &str,
	tag_value: &str,
	properties: &[&str],
	required: &[&str],
) -> bool {
	match value {
		Value::Object(object) => {
			let tagged =
				object.get("properties").and_then(Value::as_object).is_some_and(|fields| {
					fields.get(tag).is_some_and(|schema| contains_enum_value(schema, tag_value))
						&& properties.iter().all(|name| fields.contains_key(*name))
						&& required_fields(object, required)
				});
			tagged
				|| object
					.values()
					.any(|nested| has_tagged_object(nested, tag, tag_value, properties, required))
		},
		Value::Array(values) => values
			.iter()
			.any(|nested| has_tagged_object(nested, tag, tag_value, properties, required)),
		_ => false,
	}
}

fn has_object_shape(value: &Value, properties: &[&str], required: &[&str]) -> bool {
	match value {
		Value::Object(object) => {
			let shaped =
				object.get("properties").and_then(Value::as_object).is_some_and(|fields| {
					properties.iter().all(|name| fields.contains_key(*name))
						&& required_fields(object, required)
				});
			shaped || object.values().any(|nested| has_object_shape(nested, properties, required))
		},
		Value::Array(values) =>
			values.iter().any(|nested| has_object_shape(nested, properties, required)),
		_ => false,
	}
}

fn required_fields(object: &Map<String, Value>, required: &[&str]) -> bool {
	object.get("required").and_then(Value::as_array).is_some_and(|fields| {
		required.iter().all(|name| fields.iter().any(|field| field.as_str() == Some(*name)))
	})
}

fn contains_enum_value(value: &Value, expected: &str) -> bool {
	match value {
		Value::Object(object) =>
			object
				.get("enum")
				.and_then(Value::as_array)
				.is_some_and(|values| values.iter().any(|value| value.as_str() == Some(expected)))
				|| object.values().any(|nested| contains_enum_value(nested, expected)),
		Value::Array(values) => values.iter().any(|nested| contains_enum_value(nested, expected)),
		_ => false,
	}
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

	use crate::{
		quick_task::{QuickTaskMethod, QuickTaskNotification},
		schema::{ACCEPTED_SCHEMA_RECEIPT, REQUIRED_REQUEST_METHODS, SchemaContract, SchemaMarker},
	};

	#[test]
	fn accepted_marker_golden_satisfies_the_xy_1262_contract() {
		let marker = SchemaMarker::accepted();

		assert_eq!(marker.receipt, ACCEPTED_SCHEMA_RECEIPT);
		assert_eq!(marker.canonical_digests().len(), 9);
		for (file, digest) in [
			(
				"ClientRequest.json",
				"6ffc593d603d21a051840539a4dbfad95cad2e7fec315e252b6722bd71bf37b4",
			),
			(
				"ServerRequest.json",
				"6455b23a65fa3d9c7749ecd2ecbc4b829c9039f6cd8f9adc44d86ad4522e37ec",
			),
			(
				"ServerNotification.json",
				"abbb54060ea6a6005e63267bc6996eacd70cbb7954a7e0d61f50ea02af4acf02",
			),
			(
				"codex_app_server_protocol.v2.schemas.json",
				"e554a74bd59d38d16acb1744750b2999156ee3d65d0fe906b22ab52edf17fbbc",
			),
			(
				"v2/LoginAccountParams.json",
				"3bec7003eb85aabbeaf0ba8a22ec54b68ec26d2657d6878a31ca0d01dfe642e0",
			),
			(
				"ChatgptAuthTokensRefreshParams.json",
				"74d490082dab616ac01c94d388c9a836304c96092db37290cfdd10a46b0f3ef9",
			),
			(
				"ChatgptAuthTokensRefreshResponse.json",
				"ff76f5cc58bff40216f9d5f3c5be921268059f6d66d6c034970cddf0e08f0ced",
			),
			(
				"v2/ThreadReadResponse.json",
				"94689cd705b4936a5c361deaa51fed69101eaba0629899ef8a39b600180de9b3",
			),
			(
				"v2/ThreadStartParams.json",
				"001c07a58981df5d860335bf8cee4d336df2165db6dc9c645cefed0467ccebbe",
			),
		] {
			assert_eq!(marker.canonical_digests().get(file).map(String::as_str), Some(digest));
		}

		let contract = SchemaContract::validate(marker).unwrap();

		for method in REQUIRED_REQUEST_METHODS {
			assert!(contract.advertises_request(method));
		}
		for method in QuickTaskMethod::ALL {
			assert!(contract.advertises_request(method.as_str()));
		}
		for notification in QuickTaskNotification::ALL {
			assert!(contract.advertises_notification(notification.as_str()));
		}

		assert_eq!(contract.check_quick_task_contract(), Ok(()));
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
	fn marker_validation_ignores_observed_schema_digests() {
		let mut marker = SchemaMarker::accepted();

		marker
			.canonical_sha256
			.insert("ClientRequest.json".into(), "observed-from-user-codex".into());

		assert!(SchemaContract::validate(marker).is_ok());
	}

	#[test]
	fn marker_validation_accepts_the_prior_schema_shape() {
		let mut marker = SchemaMarker::accepted();
		for (file, digest) in [
			(
				"ClientRequest.json",
				"3f82e5aec5be786c40d21440dfb6d0667d194d872bfa7041bd81c39b4ba56dc3",
			),
			(
				"ServerNotification.json",
				"16ce6adadf33aa182f98840c5d33f6294c3c37b2866bb05545c24e0dbf2cc2d2",
			),
			(
				"codex_app_server_protocol.v2.schemas.json",
				"f5e8d20f3a8f9bb5e5b23ab0c5aa6bde7b12e7e0713606c5d0132651a4959d37",
			),
		] {
			marker.canonical_sha256.insert(file.into(), digest.into());
		}

		assert!(SchemaContract::validate(marker).is_ok());
	}

	#[test]
	fn marker_validation_accepts_a_different_schema_shape() {
		let mut marker = SchemaMarker::accepted();
		for (file, digest) in [
			(
				"ClientRequest.json",
				"ee9fcbf5c0b3af8526dea54d3c1c7a6ca480f0847b049b9b7d4cde00ddd82735",
			),
			(
				"ServerNotification.json",
				"189dc3b9bf8e96a115cf1102e60c379d8e34382ddca2868d1b2b46847d122166",
			),
			(
				"codex_app_server_protocol.v2.schemas.json",
				"2ad5e818b870a6a26387678bbe276e4c67b3b078f6ac03143fba623b0969605d",
			),
		] {
			marker.canonical_sha256.insert(file.into(), digest.into());
		}

		assert!(SchemaContract::validate(marker).is_ok());
	}

	#[test]
	fn marker_validation_accepts_an_older_schema_shape_when_methods_match() {
		let mut marker = SchemaMarker::accepted();
		for (file, digest) in [
			(
				"ClientRequest.json",
				"92085c18742dd355e5afa7d570170c74629635082e8e3341a952068735dc28b2",
			),
			(
				"ServerNotification.json",
				"97c6bf194b9edfa1e2ffe62547e4497fa5ea8a1af5c94687956b69966ac6f9e2",
			),
			(
				"codex_app_server_protocol.v2.schemas.json",
				"27f8d983f19d8e1a5548d52176de0a460fb05aaf2a72110f913c6f4af2bd4f27",
			),
		] {
			marker.canonical_sha256.insert(file.into(), digest.into());
		}

		assert!(SchemaContract::validate(marker).is_ok());
	}

	#[test]
	fn marker_validation_accepts_a_current_schema_shape_without_a_release_pin() {
		let mut marker = SchemaMarker::accepted();
		for (file, digest) in [
			(
				"ClientRequest.json",
				"6755a5eb5fcc0502a9d3b56c8ebd43a857f2c22820cf9cfa12e2dd0d5d48234c",
			),
			(
				"ServerNotification.json",
				"28fd2f3e9020a1a26503facff7038f84137c2a0139df8443eab6d63a71deb240",
			),
			(
				"codex_app_server_protocol.v2.schemas.json",
				"5ff4540622e002308ad5e6bac6df49b7ab5d52d79c8f71537b1098951b946b2d",
			),
		] {
			marker.canonical_sha256.insert(file.into(), digest.into());
		}

		assert!(SchemaContract::validate(marker).is_ok());
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
