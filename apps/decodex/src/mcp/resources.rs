use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	str,
};

use reqwest::Url;
use serde::Serialize;
use serde_json::{self, Value};

use crate::orchestrator;

use super::{
	DEFAULT_MCP_STATUS_LIMIT, McpContext, McpError, McpServer, ReadResourceParams,
	autonomy_resources::{
		mcp_autonomy_current_objective_resource, mcp_autonomy_evidence_resource,
		mcp_autonomy_objective_version_resource, mcp_autonomy_project_resource,
		mcp_autonomy_proposal_resource, mcp_autonomy_proposals_resource,
		mcp_autonomy_signal_resource, mcp_autonomy_signals_resource,
	},
	observability::{
		mcp_activity_tail_resource, mcp_pr_review_state_resource,
		mcp_public_lane_control_readback_resource, mcp_public_lane_inspect_resource,
		mcp_run_resource, mcp_status_live_resource, sanitize_mcp_observability_value,
	},
	safe_autonomy_record_identifier, safe_runtime_identifier,
};

const DOCS_HOST: &str = "docs";
const RESEARCH_HOST: &str = "research";
const DECISION_CONTRACTS_HOST: &str = "decision-contracts";
const PROJECTS_HOST: &str = "projects";

impl McpServer {
	pub(super) fn list_resources(&self) -> crate::prelude::Result<Value, McpError> {
		let mut resources = self.context.docs_resources()?;

		resources.extend(self.context.decision_contract_resources()?);

		if let Some(project_id) = self.context.project_id() {
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/status"),
				format!("Project {project_id} status"),
				"Read-only local runtime status snapshot.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/status_live"),
				format!("Project {project_id} live status"),
				"Remote-safe status, activity, progress, and lane-control summary.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/activity_tail"),
				format!("Project {project_id} activity tail"),
				"Remote-safe current/recent run activity summary.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/lane-control"),
				format!("Project {project_id} lane-control readback"),
				"Read-only lane-control state for current and recent local lanes.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/pr_review_state"),
				format!("Project {project_id} PR/review state"),
				"Remote-safe PR and review-state readback.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/autonomy"),
				format!("Project {project_id} autonomy summaries"),
				"Read-only autonomy objective, signal, proposal, and evidence summaries.",
			));
		}

		Ok(serde_json::json!({ "resources": resources }))
	}

	pub(super) fn list_resource_templates(&self) -> Value {
		let mut resource_templates = docs_resource_templates();

		resource_templates.extend(runtime_resource_templates());

		serde_json::json!({
			"resourceTemplates": resource_templates
		})
	}

	pub(super) fn read_resource(
		&self,
		params: Option<Value>,
	) -> crate::prelude::Result<Value, McpError> {
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

impl McpContext {
	fn docs_resources(&self) -> crate::prelude::Result<Vec<McpResource>, McpError> {
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

		for lane in ["spec", "runbook", "reference", "decisions", "research"] {
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
			let Some(stem) = markdown_stem(&entry) else {
				continue;
			};

			resources.push(McpResource::markdown(
				format!("decodex://research/{stem}"),
				format!("docs/research/{stem}.md"),
				"Checked-in Markdown Research Contract concept.",
			));
		}

		Ok(resources)
	}

	fn decision_contract_resources(&self) -> crate::prelude::Result<Vec<McpResource>, McpError> {
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

	fn read_resource(&self, uri: &str) -> crate::prelude::Result<ResourceContent, McpError> {
		let resource_uri = ResourceUri::parse(uri)?;

		match resource_uri.host.as_str() {
			DOCS_HOST => self.read_docs_resource(&resource_uri),
			RESEARCH_HOST => self.read_research_resource(&resource_uri),
			DECISION_CONTRACTS_HOST => self.read_decision_contract_resource(&resource_uri),
			PROJECTS_HOST => self.read_project_resource(&resource_uri),
			_ => Err(McpError::resource_not_found()),
		}
	}

	fn read_docs_resource(
		&self,
		uri: &ResourceUri,
	) -> crate::prelude::Result<ResourceContent, McpError> {
		let path = match uri.segments.as_slice() {
			[segment] if segment == "index" => self.repo_root.join("docs/index.md"),
			[segment] if segment == "policy" => self.repo_root.join("docs/policy.md"),
			[lane, topic] if docs_lane_allowed(lane) && safe_resource_stem(topic) =>
				self.repo_root.join("docs").join(lane).join(format!("{topic}.md")),
			_ => return Err(McpError::resource_not_found()),
		};

		read_file_resource(&uri.raw, path, "text/markdown")
	}

	fn read_research_resource(
		&self,
		uri: &ResourceUri,
	) -> crate::prelude::Result<ResourceContent, McpError> {
		let [concept] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if !safe_resource_stem(concept) {
			return Err(McpError::resource_not_found());
		}

		read_file_resource(
			&uri.raw,
			self.repo_root.join("docs/research").join(format!("{concept}.md")),
			"text/markdown",
		)
	}

	fn read_decision_contract_resource(
		&self,
		uri: &ResourceUri,
	) -> crate::prelude::Result<ResourceContent, McpError> {
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
		let mut value = serde_json::json!({
			"schema": "decodex.mcp.decision_contract_resource/1",
			"project_id": record.project_id(),
			"source_issue_id": record.source_issue_id(),
			"status": record.status(),
			"created_at": record.created_at(),
			"updated_at": record.updated_at(),
			"decision_contract": record.contract()
		});

		sanitize_mcp_observability_value(&mut value);

		ResourceContent::json(&uri.raw, value)
	}

	fn read_project_resource(
		&self,
		uri: &ResourceUri,
	) -> crate::prelude::Result<ResourceContent, McpError> {
		let [project_id, resource_kind, rest @ ..] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if Some(project_id.as_str()) != self.project_id.as_deref() {
			return Err(McpError::resource_not_found());
		}
		if resource_kind == "autonomy" {
			let value = self.read_autonomy_project_resource(project_id, rest)?;

			return ResourceContent::mcp_observability_json(&uri.raw, value);
		}

		let Some(config_path) = self.config_path.as_deref() else {
			return Err(McpError::resource_not_found());
		};
		let value = match (resource_kind.as_str(), rest) {
			("status", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal),
			("status_live", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map(mcp_status_live_resource)
					.map_err(McpError::internal),
			("activity_tail", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map(mcp_activity_tail_resource)
					.map_err(McpError::internal),
			("lane-control", []) => orchestrator::build_mcp_lane_control_resource(
				Some(config_path),
				None,
				None,
				DEFAULT_MCP_STATUS_LIMIT,
			)
			.map(mcp_public_lane_control_readback_resource)
			.map_err(McpError::internal),
			("lane-control", [issue]) if safe_runtime_identifier(issue) =>
				orchestrator::build_mcp_lane_control_resource(
					Some(config_path),
					Some(issue),
					None,
					DEFAULT_MCP_STATUS_LIMIT,
				)
				.map(mcp_public_lane_inspect_resource)
				.map_err(McpError::internal),
			("lane_inspect", [issue]) if safe_runtime_identifier(issue) =>
				orchestrator::build_mcp_lane_control_resource(
					Some(config_path),
					Some(issue),
					None,
					DEFAULT_MCP_STATUS_LIMIT,
				)
				.map(mcp_public_lane_inspect_resource)
				.map_err(McpError::internal),
			("runs", [run_id, resource])
				if resource == "events" && safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| mcp_run_resource(&snapshot, run_id, "events")),
			("runs", [run_id, resource])
				if resource == "protocol_activity" && safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| mcp_run_resource(&snapshot, run_id, "protocol_activity")),
			("runs", [run_id, resource])
				if resource == "child_agent_activity" && safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| {
						mcp_run_resource(&snapshot, run_id, "child_agent_activity")
					}),
			("runs", [run_id, resource])
				if resource == "progress_diagnostics" && safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| {
						mcp_run_resource(&snapshot, run_id, "progress_diagnostics")
					}),
			("pr_review_state", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map(mcp_pr_review_state_resource)
					.map_err(McpError::internal),
			_ => return Err(McpError::resource_not_found()),
		}?;

		ResourceContent::mcp_observability_json(&uri.raw, value)
	}

	fn read_autonomy_project_resource(
		&self,
		project_id: &str,
		rest: &[String],
	) -> crate::prelude::Result<Value, McpError> {
		let Some(state_store) = self.state_store.as_ref() else {
			return Err(McpError::resource_not_found());
		};

		match rest {
			[] => mcp_autonomy_project_resource(state_store, project_id),
			[resource] if resource == "signals" =>
				mcp_autonomy_signals_resource(state_store, project_id),
			[resource, signal_id]
				if resource == "signals" && safe_autonomy_record_identifier(signal_id) =>
				mcp_autonomy_signal_resource(state_store, project_id, signal_id),
			[resource] if resource == "proposals" =>
				mcp_autonomy_proposals_resource(state_store, project_id),
			[resource, proposal_id]
				if resource == "proposals" && safe_autonomy_record_identifier(proposal_id) =>
				mcp_autonomy_proposal_resource(state_store, project_id, proposal_id),
			[resource] if resource == "evidence" =>
				mcp_autonomy_evidence_resource(state_store, project_id),
			[resource, objective_id, selector]
				if resource == "objectives"
					&& safe_runtime_identifier(objective_id)
					&& selector == "current" =>
				mcp_autonomy_current_objective_resource(state_store, project_id, objective_id),
			[resource, objective_id, version]
				if resource == "objectives" && safe_runtime_identifier(objective_id) =>
				mcp_autonomy_objective_version_resource(
					state_store,
					project_id,
					objective_id,
					version,
				),
			_ => Err(McpError::resource_not_found()),
		}
	}
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
pub(super) struct ResourceContent {
	pub(super) uri: String,
	pub(super) mime_type: String,
	pub(super) text: String,
}
impl ResourceContent {
	fn json(uri: &str, value: Value) -> crate::prelude::Result<Self, McpError> {
		let text = serde_json::to_string_pretty(&value).map_err(McpError::internal)?;

		Ok(Self { uri: uri.to_owned(), mime_type: String::from("application/json"), text })
	}

	pub(super) fn mcp_observability_json(
		uri: &str,
		mut value: Value,
	) -> crate::prelude::Result<Self, McpError> {
		sanitize_mcp_observability_value(&mut value);

		Self::json(uri, value)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResourceUri {
	raw: String,
	host: String,
	segments: Vec<String>,
}
impl ResourceUri {
	fn parse(uri: &str) -> crate::prelude::Result<Self, McpError> {
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

fn docs_resource_templates() -> Vec<Value> {
	resource_template_values(&[
		(
			"decodex://docs/spec/{topic}",
			"Decodex specs",
			"Checked-in normative Decodex specification concepts.",
			"text/markdown",
		),
		(
			"decodex://docs/runbook/{topic}",
			"Decodex runbooks",
			"Checked-in Decodex operator procedures.",
			"text/markdown",
		),
		(
			"decodex://docs/reference/{topic}",
			"Decodex references",
			"Checked-in Decodex implementation and current-state references.",
			"text/markdown",
		),
		(
			"decodex://docs/decisions/{topic}",
			"Decodex decisions",
			"Checked-in Decodex design-rationale concepts.",
			"text/markdown",
		),
		(
			"decodex://research/{concept}",
			"Decodex research concepts",
			"Checked-in Markdown Research Contract concepts.",
			"text/markdown",
		),
	])
}

fn runtime_resource_templates() -> Vec<Value> {
	resource_template_values(&[
		(
			"decodex://decision-contracts/{contract_id}",
			"Runtime Decision Contracts",
			"Local runtime Decision Contract readback by contract id.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/status",
			"Project status",
			"Local runtime project status readback.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/status_live",
			"Project live status",
			"Remote-safe current operation, phase, event counts, progress diagnostics, and validation status.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/activity_tail",
			"Project activity tail",
			"Remote-safe activity readback for current and recent runs.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/lane_inspect/{issue}",
			"Lane inspect readback",
			"Read-only lane inspect alias for remote-safe current-lane state.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/lane-control/{issue}",
			"Lane-control readback",
			"Inspect one local lane before requesting guarded lane-control actions.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/events",
			"Run event readback",
			"Remote-safe event counts for a run visible in the current/recent status snapshot.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/protocol_activity",
			"Run protocol activity",
			"Remote-safe protocol activity for a run visible in the current/recent status snapshot, without hidden reasoning or raw payloads.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/child_agent_activity",
			"Run child-agent activity",
			"Remote-safe child-agent activity for a run visible in the current/recent status snapshot.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/progress_diagnostics",
			"Run progress diagnostics",
			"Remote-safe progress and suspected-stall diagnostics for a run visible in the current/recent status snapshot.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/pr_review_state",
			"PR/review state",
			"Remote-safe PR and review-state readback.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy",
			"Autonomy summaries",
			"Read-only project autonomy objective, signal, proposal, and evidence summaries.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/objectives/{objective_id}/current",
			"Current autonomy objective",
			"Read-only current accepted Objective Contract summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/objectives/{objective_id}/{version}",
			"Autonomy objective version",
			"Read-only Objective Contract version summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/signals",
			"Autonomy signals",
			"Read-only recent autonomy signal summaries.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/signals/{signal_id}",
			"Autonomy signal",
			"Read-only autonomy signal summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/proposals",
			"Autonomy proposals",
			"Read-only recent autonomy proposal summaries.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/proposals/{proposal_id}",
			"Autonomy proposal",
			"Read-only autonomy proposal summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/evidence",
			"Autonomy evidence summaries",
			"Read-only evidence summary counts and refs derived from recent signals and proposals.",
			"application/json",
		),
	])
}

fn resource_template_values(templates: &[(&str, &str, &str, &str)]) -> Vec<Value> {
	templates
		.iter()
		.map(|(uri_template, name, description, mime_type)| {
			serde_json::json!({
				"uriTemplate": uri_template,
				"name": name,
				"description": description,
				"mimeType": mime_type
			})
		})
		.collect()
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

fn read_sorted_dir(path: &Path) -> crate::prelude::Result<Vec<PathBuf>, McpError> {
	let entries = match fs::read_dir(path) {
		Ok(entries) => entries,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
		Err(error) => return Err(McpError::internal(error)),
	};
	let mut paths = entries
		.map(|entry| entry.map(|entry| entry.path()).map_err(McpError::internal))
		.collect::<crate::prelude::Result<Vec<_>, _>>()?;

	paths.sort();

	Ok(paths)
}

fn markdown_stem(path: &Path) -> Option<String> {
	if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
		return None;
	}

	path.file_stem().and_then(|stem| stem.to_str()).map(str::to_owned)
}

fn read_file_resource(
	uri: &str,
	path: PathBuf,
	mime_type: &str,
) -> crate::prelude::Result<ResourceContent, McpError> {
	let text = fs::read_to_string(path).map_err(|error| match error.kind() {
		ErrorKind::NotFound => McpError::resource_not_found(),
		_ => McpError::internal(error),
	})?;

	Ok(ResourceContent { uri: uri.to_owned(), mime_type: mime_type.to_owned(), text })
}

fn docs_lane_allowed(lane: &str) -> bool {
	matches!(lane, "spec" | "runbook" | "reference" | "decisions" | "research")
}

fn safe_resource_stem(value: &str) -> bool {
	!value.is_empty()
		&& value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
