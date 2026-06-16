use std::{
	env,
	fmt::Display,
	fs,
	io::{self, BufRead as _, BufReader, ErrorKind, Read, Write},
	path::{Path, PathBuf},
};

use clap::ValueEnum;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use crate::{
	config::ServiceConfig,
	orchestrator,
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const SERVER_NAME: &str = "decodex";
const DOCS_HOST: &str = "docs";
const RESEARCH_HOST: &str = "research";
const DECISION_CONTRACTS_HOST: &str = "decision-contracts";
const PROJECTS_HOST: &str = "projects";
const RESOURCE_NOT_FOUND_CODE: i64 = -32_002;
const DEFAULT_MCP_STATUS_LIMIT: usize = 10;

/// MCP transport supported by the native Decodex gateway.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum McpTransport {
	/// JSON-RPC messages over stdin/stdout.
	Stdio,
}

/// Request to start the native Decodex MCP gateway.
#[derive(Clone, Copy, Debug)]
pub(crate) struct McpServeRequest<'a> {
	pub(crate) transport: McpTransport,
	pub(crate) config_path: Option<&'a Path>,
}

struct McpServer {
	context: McpContext,
}
impl McpServer {
	fn handle_line(&self, line: &str) -> Option<Value> {
		let parsed = serde_json::from_str::<Value>(line);
		let value = match parsed {
			Ok(value) => value,
			Err(_) => return Some(json_rpc_error(Value::Null, -32_700, "Parse error")),
		};
		let request = match serde_json::from_value::<JsonRpcRequest>(value) {
			Ok(request) => request,
			Err(_) => return Some(json_rpc_error(Value::Null, -32_600, "Invalid Request")),
		};

		self.handle_request(request)
	}

	fn handle_request(&self, request: JsonRpcRequest) -> Option<Value> {
		let id = request.id?;

		if request.jsonrpc.as_deref() != Some("2.0") {
			return Some(json_rpc_error(id, -32_600, "Invalid Request"));
		}

		let Some(method) = request.method else {
			return Some(json_rpc_error(id, -32_600, "Invalid Request"));
		};
		let result = match method.as_str() {
			"initialize" => Ok(self.initialize()),
			"ping" => Ok(serde_json::json!({})),
			"resources/list" => self.list_resources(),
			"resources/read" => self.read_resource(request.params),
			_ => Err(McpError::method_not_found()),
		};

		Some(match result {
			Ok(result) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
			Err(error) => json_rpc_error(id, error.code, &error.message),
		})
	}

	fn initialize(&self) -> Value {
		serde_json::json!({
			"protocolVersion": MCP_PROTOCOL_VERSION,
			"capabilities": {
				"resources": {}
			},
			"serverInfo": {
				"name": SERVER_NAME,
				"version": env!("CARGO_PKG_VERSION")
			}
		})
	}

	fn list_resources(&self) -> Result<Value, McpError> {
		let mut resources = self.context.docs_resources()?;

		resources.extend(self.context.decision_contract_resources()?);

		if let Some(project_id) = self.context.project_id() {
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/status"),
				format!("Project {project_id} status"),
				"Read-only local runtime status snapshot.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/lane-control"),
				format!("Project {project_id} lane-control readback"),
				"Read-only lane-control state for current and recent local lanes.",
			));
		}

		Ok(serde_json::json!({ "resources": resources }))
	}

	fn read_resource(&self, params: Option<Value>) -> Result<Value, McpError> {
		let params = params.ok_or_else(McpError::invalid_params)?;
		let params = serde_json::from_value::<ReadResourceParams>(params)
			.map_err(|_| McpError::invalid_params())?;
		let content = self.context.read_resource(&params.uri)?;

		Ok(serde_json::json!({
			"contents": [
				{
					"uri": content.uri,
					"mimeType": content.mime_type,
					"text": content.text
				}
			]
		}))
	}
}

struct McpContext {
	repo_root: PathBuf,
	config_path: Option<PathBuf>,
	project_id: Option<String>,
	state_store: Option<StateStore>,
}
impl McpContext {
	fn for_process(config_path: Option<&Path>) -> Result<Self> {
		let state_store = runtime::open_runtime_store_lazy().ok();
		let config_path = resolve_context_config_path(config_path, state_store.as_ref())?;
		let config = config_path.as_ref().map(ServiceConfig::from_path).transpose()?;
		let repo_root = config
			.as_ref()
			.map(|config| config.repo_root().to_path_buf())
			.or_else(|| discover_repo_root_from_current_dir().ok().flatten())
			.ok_or_else(|| {
				eyre::eyre!(
					"Failed to find the Decodex repository root for MCP docs resources; start from a checkout or pass --config."
				)
			})?;
		let project_id = config.map(|config| config.service_id().to_owned());

		Ok(Self { repo_root, config_path, project_id, state_store })
	}

	fn project_id(&self) -> Option<&str> {
		self.project_id.as_deref()
	}

	fn docs_resources(&self) -> Result<Vec<McpResource>, McpError> {
		let mut resources = Vec::new();

		push_file_resource(
			&mut resources,
			self.repo_root.join("docs/index.md"),
			"decodex://docs/index",
			"Documentation index",
			"Checked-in Decodex documentation router.",
		);
		push_file_resource(
			&mut resources,
			self.repo_root.join("docs/policy.md"),
			"decodex://docs/policy",
			"Documentation policy",
			"Checked-in Decodex documentation policy.",
		);

		for lane in ["spec", "runbook", "reference", "decisions"] {
			let docs_dir = self.repo_root.join("docs").join(lane);

			for entry in read_sorted_dir(&docs_dir)? {
				let Some(stem) = markdown_stem(&entry) else {
					continue;
				};

				resources.push(McpResource::markdown(
					format!("decodex://docs/{lane}/{stem}"),
					format!("docs/{lane}/{stem}.md"),
					"Checked-in Decodex documentation resource.",
				));
			}
		}
		for entry in read_sorted_dir(&self.repo_root.join("docs/research"))? {
			let Some(stem) = json_stem(&entry) else {
				continue;
			};

			resources.push(McpResource::json(
				format!("decodex://research/{stem}"),
				format!("docs/research/{stem}.json"),
				"Checked-in JSON research report.",
			));
		}

		Ok(resources)
	}

	fn decision_contract_resources(&self) -> Result<Vec<McpResource>, McpError> {
		let Some(project_id) = self.project_id.as_deref() else {
			return Ok(Vec::new());
		};
		let Some(state_store) = self.state_store.as_ref() else {
			return Ok(Vec::new());
		};
		let records = state_store
			.list_decision_contracts_for_project(project_id)
			.map_err(McpError::internal)?;

		Ok(records
			.into_iter()
			.map(|record| {
				McpResource::json(
					format!("decodex://decision-contracts/{}", record.contract_id()),
					format!("Decision Contract {}", record.contract_id()),
					"Read-only local runtime Decision Contract readback.",
				)
			})
			.collect())
	}

	fn read_resource(&self, uri: &str) -> Result<ResourceContent, McpError> {
		let resource_uri = ResourceUri::parse(uri)?;

		match resource_uri.host.as_str() {
			DOCS_HOST => self.read_docs_resource(&resource_uri),
			RESEARCH_HOST => self.read_research_resource(&resource_uri),
			DECISION_CONTRACTS_HOST => self.read_decision_contract_resource(&resource_uri),
			PROJECTS_HOST => self.read_project_resource(&resource_uri),
			_ => Err(McpError::resource_not_found()),
		}
	}

	fn read_docs_resource(&self, uri: &ResourceUri) -> Result<ResourceContent, McpError> {
		let path = match uri.segments.as_slice() {
			[segment] if segment == "index" => self.repo_root.join("docs/index.md"),
			[segment] if segment == "policy" => self.repo_root.join("docs/policy.md"),
			[lane, topic] if docs_lane_allowed(lane) && safe_resource_stem(topic) =>
				self.repo_root.join("docs").join(lane).join(format!("{topic}.md")),
			_ => return Err(McpError::resource_not_found()),
		};

		read_file_resource(&uri.raw, path, "text/markdown")
	}

	fn read_research_resource(&self, uri: &ResourceUri) -> Result<ResourceContent, McpError> {
		let [artifact] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if !safe_research_artifact(artifact) {
			return Err(McpError::resource_not_found());
		}

		let file_name = if artifact.ends_with(".json") {
			artifact.to_owned()
		} else {
			format!("{artifact}.json")
		};

		read_file_resource(
			&uri.raw,
			self.repo_root.join("docs/research").join(file_name),
			"application/json",
		)
	}

	fn read_decision_contract_resource(
		&self,
		uri: &ResourceUri,
	) -> Result<ResourceContent, McpError> {
		let [contract_id] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if !safe_runtime_identifier(contract_id) {
			return Err(McpError::resource_not_found());
		}

		let Some(project_id) = self.project_id.as_deref() else {
			return Err(McpError::resource_not_found());
		};
		let Some(state_store) = self.state_store.as_ref() else {
			return Err(McpError::resource_not_found());
		};
		let Some(record) =
			state_store.decision_contract(project_id, contract_id).map_err(McpError::internal)?
		else {
			return Err(McpError::resource_not_found());
		};
		let value = serde_json::json!({
			"schema": "decodex.mcp.decision_contract_resource/1",
			"project_id": record.project_id(),
			"source_issue_id": record.source_issue_id(),
			"status": record.status(),
			"created_at": record.created_at(),
			"updated_at": record.updated_at(),
			"decision_contract": record.contract()
		});

		ResourceContent::json(&uri.raw, value)
	}

	fn read_project_resource(&self, uri: &ResourceUri) -> Result<ResourceContent, McpError> {
		let [project_id, resource_kind, rest @ ..] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if Some(project_id.as_str()) != self.project_id.as_deref() {
			return Err(McpError::resource_not_found());
		}

		let Some(config_path) = self.config_path.as_deref() else {
			return Err(McpError::resource_not_found());
		};
		let value = match (resource_kind.as_str(), rest) {
			("status", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT),
			("lane-control", []) => orchestrator::build_mcp_lane_control_resource(
				Some(config_path),
				None,
				None,
				DEFAULT_MCP_STATUS_LIMIT,
			),
			("lane-control", [issue]) if safe_runtime_identifier(issue) =>
				orchestrator::build_mcp_lane_control_resource(
					Some(config_path),
					Some(issue),
					None,
					DEFAULT_MCP_STATUS_LIMIT,
				),
			_ => return Err(McpError::resource_not_found()),
		}
		.map_err(McpError::internal)?;

		ResourceContent::json(&uri.raw, value)
	}
}

#[derive(Deserialize)]
struct JsonRpcRequest {
	jsonrpc: Option<String>,
	id: Option<Value>,
	method: Option<String>,
	params: Option<Value>,
}

#[derive(Deserialize)]
struct ReadResourceParams {
	uri: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct McpResource {
	uri: String,
	name: String,
	description: String,
	mime_type: String,
}
impl McpResource {
	fn markdown(
		uri: impl Into<String>,
		name: impl Into<String>,
		description: impl Into<String>,
	) -> Self {
		Self {
			uri: uri.into(),
			name: name.into(),
			description: description.into(),
			mime_type: String::from("text/markdown"),
		}
	}

	fn json(
		uri: impl Into<String>,
		name: impl Into<String>,
		description: impl Into<String>,
	) -> Self {
		Self {
			uri: uri.into(),
			name: name.into(),
			description: description.into(),
			mime_type: String::from("application/json"),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResourceContent {
	uri: String,
	mime_type: String,
	text: String,
}
impl ResourceContent {
	fn json(uri: &str, value: Value) -> Result<Self, McpError> {
		let text = serde_json::to_string_pretty(&value).map_err(McpError::internal)?;

		Ok(Self { uri: uri.to_owned(), mime_type: String::from("application/json"), text })
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResourceUri {
	raw: String,
	host: String,
	segments: Vec<String>,
}
impl ResourceUri {
	fn parse(uri: &str) -> Result<Self, McpError> {
		let parsed = Url::parse(uri).map_err(|_| McpError::invalid_params())?;

		if parsed.scheme() != "decodex" {
			return Err(McpError::invalid_params());
		}

		let host = parsed.host_str().map(str::to_owned).ok_or_else(McpError::invalid_params)?;
		let segments = parsed
			.path_segments()
			.map(|segments| {
				segments
					.filter(|segment| !segment.is_empty())
					.map(str::to_owned)
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();

		Ok(Self { raw: uri.to_owned(), host, segments })
	}
}

#[derive(Debug)]
struct McpError {
	code: i64,
	message: String,
}
impl McpError {
	fn invalid_params() -> Self {
		Self { code: -32_602, message: String::from("Invalid params") }
	}

	fn method_not_found() -> Self {
		Self { code: -32_601, message: String::from("Method not found") }
	}

	fn resource_not_found() -> Self {
		Self { code: RESOURCE_NOT_FOUND_CODE, message: String::from("Resource not found") }
	}

	fn internal(error: impl Display) -> Self {
		tracing::warn!(error = %error, "MCP resource read failed.");

		Self { code: -32_603, message: String::from("Internal error") }
	}
}

/// Start the read-only Decodex MCP gateway.
pub(crate) fn serve(request: McpServeRequest<'_>) -> Result<()> {
	match request.transport {
		McpTransport::Stdio => {
			let context = McpContext::for_process(request.config_path)?;
			let stdin = io::stdin();
			let stdout = io::stdout();

			serve_stdio_with_context(stdin.lock(), stdout.lock(), context)
		},
	}
}

fn serve_stdio_with_context<R, W>(reader: R, mut writer: W, context: McpContext) -> Result<()>
where
	R: Read,
	W: Write,
{
	let server = McpServer { context };
	let reader = BufReader::new(reader);

	for line in reader.lines() {
		let line = line?;

		if line.trim().is_empty() {
			continue;
		}

		if let Some(response) = server.handle_line(&line) {
			write_json_line(&mut writer, &response)?;
		}
	}

	Ok(())
}

fn write_json_line<W>(writer: &mut W, value: &Value) -> Result<()>
where
	W: Write,
{
	let line = serde_json::to_string(value)?;

	match writer
		.write_all(line.as_bytes())
		.and_then(|()| writer.write_all(b"\n"))
		.and_then(|()| writer.flush())
	{
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
		Err(error) => Err(error.into()),
	}
}

fn resolve_context_config_path(
	explicit_path: Option<&Path>,
	state_store: Option<&StateStore>,
) -> Result<Option<PathBuf>> {
	if let Some(path) = explicit_path {
		return Ok(Some(path.to_path_buf()));
	}

	let Some(state_store) = state_store else {
		return Ok(None);
	};

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)
}

fn discover_repo_root_from_current_dir() -> Result<Option<PathBuf>> {
	let mut candidate = env::current_dir()?;

	loop {
		if candidate.join("docs/index.md").is_file() && candidate.join("Cargo.toml").is_file() {
			return Ok(Some(candidate));
		}
		if !candidate.pop() {
			return Ok(None);
		}
	}
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
	serde_json::json!({
		"jsonrpc": "2.0",
		"id": id,
		"error": {
			"code": code,
			"message": message
		}
	})
}

fn push_file_resource(
	resources: &mut Vec<McpResource>,
	path: PathBuf,
	uri: &str,
	name: &str,
	description: &str,
) {
	if path.is_file() {
		resources.push(McpResource::markdown(uri, name, description));
	}
}

fn read_sorted_dir(path: &Path) -> Result<Vec<PathBuf>, McpError> {
	let entries = match fs::read_dir(path) {
		Ok(entries) => entries,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
		Err(error) => return Err(McpError::internal(error)),
	};
	let mut paths = entries
		.map(|entry| entry.map(|entry| entry.path()).map_err(McpError::internal))
		.collect::<Result<Vec<_>, _>>()?;

	paths.sort();

	Ok(paths)
}

fn markdown_stem(path: &Path) -> Option<String> {
	if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
		return None;
	}

	path.file_stem().and_then(|stem| stem.to_str()).map(str::to_owned)
}

fn json_stem(path: &Path) -> Option<String> {
	if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
		return None;
	}

	path.file_stem().and_then(|stem| stem.to_str()).map(str::to_owned)
}

fn read_file_resource(
	uri: &str,
	path: PathBuf,
	mime_type: &str,
) -> Result<ResourceContent, McpError> {
	let text = fs::read_to_string(path).map_err(|error| match error.kind() {
		ErrorKind::NotFound => McpError::resource_not_found(),
		_ => McpError::internal(error),
	})?;

	Ok(ResourceContent { uri: uri.to_owned(), mime_type: mime_type.to_owned(), text })
}

fn docs_lane_allowed(lane: &str) -> bool {
	matches!(lane, "spec" | "runbook" | "reference" | "decisions")
}

fn safe_resource_stem(value: &str) -> bool {
	!value.is_empty()
		&& value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn safe_research_artifact(value: &str) -> bool {
	!value.is_empty()
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
		&& !value.contains("..")
}

fn safe_runtime_identifier(value: &str) -> bool {
	!value.is_empty()
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
		&& !value.contains("..")
}

#[cfg(test)]
mod tests {
	use std::{fs, io::Cursor, path::Path};

	use serde_json::Value;
	use tempfile::TempDir;

	use crate::{
		loop_contract::DecisionContract,
		mcp::{self, McpContext},
		state::StateStore,
	};

	#[test]
	fn initialize_exposes_read_only_resource_capability() {
		let repo = test_repo();
		let responses =
			run_stdio(repo.path(), r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
		let response = response_at(&responses, 0);
		let result = response.get("result").and_then(Value::as_object).expect("result object");
		let capabilities =
			result.get("capabilities").and_then(Value::as_object).expect("capabilities object");

		assert!(capabilities.contains_key("resources"));
		assert!(!capabilities.contains_key("tools"));
	}

	#[test]
	fn resources_list_includes_docs_decisions_and_research_json() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
		);
		let resources =
			response_at(&responses, 0)["result"]["resources"].as_array().expect("resources array");
		let uris = resources
			.iter()
			.filter_map(|resource| resource.get("uri").and_then(Value::as_str))
			.collect::<Vec<_>>();

		assert!(uris.contains(&"decodex://docs/index"));
		assert!(uris.contains(&"decodex://docs/spec/runtime"));
		assert!(uris.contains(&"decodex://docs/decisions/mcp-gateway"));
		assert!(uris.contains(&"decodex://research/sample-report"));
	}

	#[test]
	fn resources_list_includes_runtime_decision_contracts() {
		let repo = test_repo();
		let state_store = StateStore::open_in_memory().expect("state store should open");

		state_store
			.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
			.expect("decision contract should persist");

		let responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(state_store),
			},
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
		);
		let resources =
			response_at(&responses, 0)["result"]["resources"].as_array().expect("resources array");
		let uris = resources
			.iter()
			.filter_map(|resource| resource.get("uri").and_then(Value::as_str))
			.collect::<Vec<_>>();

		assert!(uris.contains(&"decodex://decision-contracts/research-x-loop-contract"));
	}

	#[test]
	fn resources_read_returns_runtime_decision_contract() {
		let repo = test_repo();
		let state_store = StateStore::open_in_memory().expect("state store should open");

		state_store
			.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
			.expect("decision contract should persist");

		let responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(state_store),
			},
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://decision-contracts/research-x-loop-contract"}}"#,
		);
		let contents =
			response_at(&responses, 0)["result"]["contents"].as_array().expect("contents array");
		let text = contents[0]["text"].as_str().expect("text content");
		let content: Value = serde_json::from_str(text).expect("decision contract should be json");

		assert_eq!(content["project_id"], "decodex");
		assert_eq!(content["decision_contract"]["contract_id"], "research-x-loop-contract");
	}

	#[test]
	fn resources_read_returns_checked_in_doc_text() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://docs/spec/runtime"}}"#,
		);
		let contents =
			response_at(&responses, 0)["result"]["contents"].as_array().expect("contents array");
		let text = contents[0]["text"].as_str().expect("text content");

		assert_eq!(text, "# Runtime\n\nSpec body.\n");
	}

	#[test]
	fn resources_read_rejects_invalid_resource_uri() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"file:///etc/passwd"}}"#,
		);
		let error = response_at(&responses, 0).get("error").expect("error response");

		assert_eq!(error["code"], -32_602);
	}

	#[test]
	fn stdio_output_contains_only_json_rpc_responses() {
		let repo = test_repo();
		let input = [
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}"#,
		]
		.join("\n");
		let output = run_stdio_raw(repo.path(), &input);
		let lines = output.lines().collect::<Vec<_>>();

		assert_eq!(lines.len(), 2);

		for line in lines {
			let value = serde_json::from_str::<Value>(line).expect("stdout line should be JSON");

			assert_eq!(value["jsonrpc"], "2.0");
		}
	}

	fn run_stdio(repo_root: &Path, input: &str) -> Vec<Value> {
		run_stdio_raw(repo_root, input)
			.lines()
			.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
			.collect()
	}

	fn run_stdio_with_context(context: McpContext, input: &str) -> Vec<Value> {
		run_stdio_raw_with_context(context, input)
			.lines()
			.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
			.collect()
	}

	fn run_stdio_raw(repo_root: &Path, input: &str) -> String {
		let context = McpContext {
			repo_root: repo_root.to_path_buf(),
			config_path: None,
			project_id: None,
			state_store: None,
		};

		run_stdio_raw_with_context(context, input)
	}

	fn run_stdio_raw_with_context(context: McpContext, input: &str) -> String {
		let mut output = Vec::new();

		mcp::serve_stdio_with_context(Cursor::new(format!("{input}\n")), &mut output, context)
			.expect("stdio server should run");

		String::from_utf8(output).expect("stdout should be utf-8")
	}

	fn response_at(responses: &[Value], index: usize) -> &Value {
		responses.get(index).expect("response should exist")
	}

	fn test_repo() -> TempDir {
		let repo = TempDir::new().expect("temp repo should exist");

		write_file(repo.path().join("Cargo.toml"), "[workspace]\n");
		write_file(repo.path().join("docs/index.md"), "# Docs\n");
		write_file(repo.path().join("docs/policy.md"), "# Policy\n");
		write_file(repo.path().join("docs/spec/runtime.md"), "# Runtime\n\nSpec body.\n");
		write_file(repo.path().join("docs/decisions/mcp-gateway.md"), "# MCP\n");
		write_file(repo.path().join("docs/research/sample-report.json"), "{}\n");

		repo
	}

	fn write_file(path: std::path::PathBuf, contents: &str) {
		let parent = path.parent().expect("test path should have parent");

		fs::create_dir_all(parent).expect("parent directory should exist");
		fs::write(path, contents).expect("test file should write");
	}

	fn latent_decision_contract_fixture() -> DecisionContract {
		serde_json::from_str(include_str!(concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/fixtures/decision_contract/research_x_latent_contract.json"
		)))
		.expect("research X latent contract fixture should deserialize")
	}
}
