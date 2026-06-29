use std::{
	collections::BTreeSet,
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	str,
};

use reqwest::Url;
use serde::Serialize;
use serde_json::{self, Value};

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::AutonomyProposal,
	autonomy_signal::{AutonomySignal, AutonomySignalPrivacy},
	orchestrator,
	state::{AutonomyProposalRecord, AutonomySignalRecord, StateStore},
};

use super::{
	DEFAULT_MCP_STATUS_LIMIT, McpContext, McpError, McpServer, ReadResourceParams,
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

fn mcp_autonomy_project_resource(
	state_store: &StateStore,
	project_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let objectives = state_store
		.recent_autonomy_objectives_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;
	let signals = state_store
		.recent_autonomy_signals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;
	let proposals = state_store
		.recent_autonomy_proposals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_summary/1",
		"project_id": project_id,
		"read_only": true,
		"authority_boundary": mcp_autonomy_authority_boundary(),
		"objectives": objectives
			.iter()
			.map(|record| mcp_autonomy_objective_summary(record.objective(), Some(record.updated_at())))
			.collect::<Vec<_>>(),
		"signals": signals
			.iter()
			.map(|record| mcp_autonomy_signal_summary(record.signal(), Some(record.updated_at())))
			.collect::<Vec<_>>(),
		"proposals": proposals
			.iter()
			.map(|record| mcp_autonomy_proposal_summary(record.proposal(), Some(record.updated_at())))
			.collect::<Vec<_>>(),
		"evidence": mcp_autonomy_evidence_summary(&signals, &proposals)
	}))
}

fn mcp_autonomy_current_objective_resource(
	state_store: &StateStore,
	project_id: &str,
	objective_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let Some(record) = state_store
		.current_accepted_autonomy_objective(project_id, objective_id)
		.map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_resource/1",
		"project_id": project_id,
		"read_only": true,
		"authority_boundary": mcp_autonomy_authority_boundary(),
		"objective": mcp_autonomy_objective_summary(record.objective(), Some(record.updated_at()))
	}))
}

fn mcp_autonomy_objective_version_resource(
	state_store: &StateStore,
	project_id: &str,
	objective_id: &str,
	version: &str,
) -> crate::prelude::Result<Value, McpError> {
	let version = version.parse::<u64>().map_err(|_| McpError::resource_not_found())?;
	let Some(record) = state_store
		.autonomy_objective(project_id, objective_id, version)
		.map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_resource/1",
		"project_id": project_id,
		"read_only": true,
		"authority_boundary": mcp_autonomy_authority_boundary(),
		"objective": mcp_autonomy_objective_summary(record.objective(), Some(record.updated_at()))
	}))
}

fn mcp_autonomy_signals_resource(
	state_store: &StateStore,
	project_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let signals = state_store
		.recent_autonomy_signals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_signals_resource/1",
		"project_id": project_id,
		"read_only": true,
		"signals": signals
			.iter()
			.map(|record| mcp_autonomy_signal_summary(record.signal(), Some(record.updated_at())))
			.collect::<Vec<_>>()
	}))
}

fn mcp_autonomy_signal_resource(
	state_store: &StateStore,
	project_id: &str,
	signal_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let Some(record) =
		state_store.autonomy_signal(project_id, signal_id).map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_signal_resource/1",
		"project_id": project_id,
		"read_only": true,
		"signal": mcp_autonomy_signal_summary(record.signal(), Some(record.updated_at()))
	}))
}

fn mcp_autonomy_proposals_resource(
	state_store: &StateStore,
	project_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let proposals = state_store
		.recent_autonomy_proposals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_proposals_resource/1",
		"project_id": project_id,
		"read_only": true,
		"proposals": proposals
			.iter()
			.map(|record| mcp_autonomy_proposal_summary(record.proposal(), Some(record.updated_at())))
			.collect::<Vec<_>>()
	}))
}

fn mcp_autonomy_proposal_resource(
	state_store: &StateStore,
	project_id: &str,
	proposal_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let Some(record) =
		state_store.autonomy_proposal(project_id, proposal_id).map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_proposal_resource/1",
		"project_id": project_id,
		"read_only": true,
		"proposal": mcp_autonomy_proposal_summary(record.proposal(), Some(record.updated_at()))
	}))
}

fn mcp_autonomy_evidence_resource(
	state_store: &StateStore,
	project_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let signals = state_store
		.recent_autonomy_signals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;
	let proposals = state_store
		.recent_autonomy_proposals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_evidence_resource/1",
		"project_id": project_id,
		"read_only": true,
		"evidence": mcp_autonomy_evidence_summary(&signals, &proposals)
	}))
}

pub(super) fn mcp_autonomy_objective_summary(
	objective: &AutonomyObjectiveContract,
	updated_at: Option<&str>,
) -> Value {
	serde_json::json!({
		"objective_id": objective.id(),
		"objective_version": objective.version(),
		"state": objective.state().as_str(),
		"summary": objective.summary(),
		"goals": objective.goals(),
		"non_goals": objective.non_goals(),
		"metrics": objective.metrics(),
		"allowed_surfaces": objective.allowed_surfaces(),
		"allowed_signal_kinds": objective.allowed_signal_kinds(),
		"validation_gates": objective.validation_gates(),
		"review_policy": objective.review_policy(),
		"acceptance_present": objective.acceptance().is_some(),
		"updated_at": updated_at
	})
}

pub(super) fn mcp_autonomy_signal_summary(
	signal: &AutonomySignal,
	updated_at: Option<&str>,
) -> Value {
	let (source_refs, primary_source_refs, source_ref_count, primary_source_ref_count) =
		mcp_autonomy_signal_ref_summary(signal);

	serde_json::json!({
		"signal_id": signal.id(),
		"objective_id": signal.objective_id(),
		"objective_version": signal.objective_version(),
		"kind": signal.kind().as_str(),
		"source_type": signal.source_type().as_str(),
		"source_refs": source_refs,
		"source_ref_count": source_ref_count,
		"primary_source_refs": primary_source_refs,
		"primary_source_ref_count": primary_source_ref_count,
		"freshness": signal.freshness().as_str(),
		"summary": signal.summary(),
		"evidence_class": signal.evidence_class().as_str(),
		"confidence": signal.confidence().as_str(),
		"redaction_level": signal.privacy().as_str(),
		"gaps": signal.gaps(),
		"contradictions": signal.contradictions(),
		"review_evidence_present": signal.review_evidence().is_some(),
		"updated_at": updated_at
	})
}

fn mcp_autonomy_signal_ref_summary(signal: &AutonomySignal) -> (Value, Value, usize, usize) {
	let source_ref_count = signal.source_refs().len();
	let primary_source_ref_count = signal.primary_source_refs().len();

	if signal.privacy() == AutonomySignalPrivacy::LocalPrivate {
		return (
			serde_json::json!([]),
			serde_json::json!([]),
			source_ref_count,
			primary_source_ref_count,
		);
	}

	(
		serde_json::json!(signal.source_refs()),
		serde_json::json!(signal.primary_source_refs()),
		source_ref_count,
		primary_source_ref_count,
	)
}

pub(super) fn mcp_autonomy_proposal_summary(
	proposal: &AutonomyProposal,
	updated_at: Option<&str>,
) -> Value {
	serde_json::json!({
		"proposal_id": proposal.id(),
		"objective_id": proposal.objective_id(),
		"objective_version": proposal.objective_version(),
		"state": proposal.state().as_str(),
		"summary": proposal.summary(),
		"source_family": proposal.source_family(),
		"intended_surface": proposal.intended_surface(),
		"affected_identifiers": proposal.affected_identifiers(),
		"source_signal_ids": proposal.source_signal_ids(),
		"allowed_surfaces": proposal.allowed_surfaces(),
		"validation_gates": proposal.validation_gates(),
		"refusal_reasons": proposal
			.refusal_reasons()
			.iter()
			.map(|refusal| refusal.reason().as_str())
			.collect::<Vec<_>>(),
		"refusals": proposal
			.refusal_reasons()
			.iter()
			.map(|refusal| {
				serde_json::json!({
					"reason": refusal.reason().as_str(),
					"detail": refusal.detail(),
					"evidence_refs": refusal.evidence_refs()
				})
			})
			.collect::<Vec<_>>(),
		"gaps": proposal.gaps(),
		"contradictions": proposal.contradictions(),
		"challenge_evidence_count": proposal.challenge_evidence().len(),
		"updated_at": updated_at
	})
}

fn mcp_autonomy_evidence_summary(
	signals: &[AutonomySignalRecord],
	proposals: &[AutonomyProposalRecord],
) -> Value {
	serde_json::json!({
		"signal_count": signals.len(),
		"proposal_count": proposals.len(),
		"signal_refs": signals
			.iter()
			.map(|record| {
				serde_json::json!({
					"signal_id": record.signal_id(),
					"kind": record.kind().as_str(),
					"freshness": record.freshness().as_str(),
					"evidence_class": record.evidence_class().as_str(),
					"confidence": record.confidence().as_str(),
					"redaction_level": record.privacy().as_str()
				})
			})
			.collect::<Vec<_>>(),
		"proposal_refs": proposals
			.iter()
			.map(|record| {
				serde_json::json!({
					"proposal_id": record.proposal_id(),
					"state": record.state().as_str(),
					"objective_id": record.objective_id(),
					"objective_version": record.objective_version()
				})
			})
			.collect::<Vec<_>>(),
		"authority_effect": "evidence_summary_only_no_execution_authority"
	})
}

fn mcp_autonomy_authority_boundary() -> Value {
	serde_json::json!({
		"mcp_authentication": "access_boundary_only",
		"capability_profile": "tool_visibility_boundary_only",
		"acceptance_authority": "explicit_human_or_trusted_accepted_project_policy_required",
		"execution_authority": "Decision Contract promotion and Program Intake remain separate"
	})
}

pub(super) fn mcp_status_live_resource(snapshot: Value) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.status_live/1",
		"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
		"status_source": snapshot.get("status_source").cloned().unwrap_or(Value::Null),
		"run_limit": snapshot.get("run_limit").cloned().unwrap_or(Value::Null),
		"current_lanes": mcp_run_activity_summaries(snapshot.get("current_lanes")),
		"recent_runs": mcp_run_activity_summaries(snapshot.get("recent_runs")),
		"post_review_lanes": mcp_public_post_review_lanes(snapshot.get("post_review_lanes"))
	})
}

pub(super) fn mcp_activity_tail_resource(snapshot: Value) -> Value {
	let limit = snapshot
		.get("run_limit")
		.and_then(Value::as_u64)
		.and_then(|limit| usize::try_from(limit).ok())
		.unwrap_or(DEFAULT_MCP_STATUS_LIMIT);
	let mut activity = Vec::new();

	for run in mcp_all_runs(&snapshot).into_iter().take(limit) {
		activity.push(mcp_run_activity_summary(run));
	}

	serde_json::json!({
		"schema": "decodex.mcp.activity_tail/1",
		"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
		"activity": activity
	})
}

fn mcp_public_lane_control_readback_resource(readback: Value) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.lane_control_readback/1",
		"project_id": readback.get("project_id").cloned().unwrap_or(Value::Null),
		"read_only": readback.get("read_only").cloned().unwrap_or(Value::Null),
		"mutating_tools": readback.get("mutating_tools").cloned().unwrap_or_else(|| serde_json::json!([])),
		"current_lanes": mcp_run_activity_summaries(readback.get("current_lanes")),
		"recent_runs": mcp_run_activity_summaries(readback.get("recent_runs")),
		"post_review_lanes": mcp_public_post_review_lanes(readback.get("post_review_lanes"))
	})
}

pub(super) fn mcp_public_lane_inspect_resource(report: Value) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.lane_inspect/1",
		"projectId": report.get("projectId").cloned().unwrap_or(Value::Null),
		"issue": report.get("issue").cloned().unwrap_or(Value::Null),
		"runId": report.get("runId").cloned().unwrap_or(Value::Null),
		"matchedRunCount": report.get("matchedRunCount").cloned().unwrap_or(Value::Null),
		"runs": mcp_public_lane_inspect_runs(report.get("runs"))
	})
}

fn mcp_public_lane_inspect_runs(runs: Option<&Value>) -> Vec<Value> {
	runs.and_then(Value::as_array).into_iter().flatten().map(mcp_public_lane_inspect_run).collect()
}

fn mcp_public_lane_inspect_run(run: &Value) -> Value {
	serde_json::json!({
		"projectId": run.get("projectId").cloned().unwrap_or(Value::Null),
		"issueId": run.get("issueId").cloned().unwrap_or(Value::Null),
		"issueIdentifier": run.get("issueIdentifier").cloned().unwrap_or(Value::Null),
		"runId": run.get("runId").cloned().unwrap_or(Value::Null),
		"attemptNumber": run.get("attemptNumber").cloned().unwrap_or(Value::Null),
		"status": run.get("status").cloned().unwrap_or(Value::Null),
		"attemptStatus": run.get("attemptStatus").cloned().unwrap_or(Value::Null),
		"phase": run.get("phase").cloned().unwrap_or(Value::Null),
		"waitReason": run.get("waitReason").cloned().unwrap_or(Value::Null),
		"currentOperation": run.get("currentOperation").cloned().unwrap_or(Value::Null),
		"laneControlNextAction": run
			.get("laneControlNextAction")
			.cloned()
			.unwrap_or(Value::Null),
		"laneControlConditions": run
			.get("laneControlConditions")
			.cloned()
			.unwrap_or_else(|| serde_json::json!([])),
		"lastEventType": run.get("lastEventType").cloned().unwrap_or(Value::Null),
		"lastEventAt": run.get("lastEventAt").cloned().unwrap_or(Value::Null),
		"eventCount": run.get("eventCount").cloned().unwrap_or(Value::Null)
	})
}

pub(super) fn mcp_run_resource(
	snapshot: &Value,
	run_id: &str,
	kind: &str,
) -> Result<Value, McpError> {
	let Some(run) = mcp_find_run(snapshot, run_id) else {
		return Err(McpError::resource_not_found());
	};
	let value = match kind {
		"events" => serde_json::json!({
			"schema": "decodex.mcp.run_events/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"issue_id": run.get("issue_id").cloned().unwrap_or(Value::Null),
			"issue_identifier": run.get("issue_identifier").cloned().unwrap_or(Value::Null),
			"event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
			"last_event_type": run.get("last_event_type").cloned().unwrap_or(Value::Null),
			"last_event_at": run.get("last_event_at").cloned().unwrap_or(Value::Null),
			"last_protocol_activity_at": run
				.get("last_protocol_activity_at")
				.cloned()
				.unwrap_or(Value::Null)
		}),
		"protocol_activity" => serde_json::json!({
			"schema": "decodex.mcp.protocol_activity/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"protocol_activity": mcp_public_protocol_activity(run),
			"event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
			"last_event_type": run.get("last_event_type").cloned().unwrap_or(Value::Null)
		}),
		"child_agent_activity" => serde_json::json!({
			"schema": "decodex.mcp.child_agent_activity/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"child_agent_activity": run.get("child_agent_activity").cloned().unwrap_or(Value::Null)
		}),
		"progress_diagnostics" => serde_json::json!({
			"schema": "decodex.mcp.progress_diagnostics/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"phase": run.get("phase").cloned().unwrap_or(Value::Null),
			"run_phase": run.get("run_phase").cloned().unwrap_or(Value::Null),
			"current_operation": run.get("current_operation").cloned().unwrap_or(Value::Null),
			"last_progress_at": run.get("last_progress_at").cloned().unwrap_or(Value::Null),
			"progress_diagnostic": run.get("progress_diagnostic").cloned().unwrap_or(Value::Null),
			"suspected_stall": run.get("suspected_stall").cloned().unwrap_or(Value::Null)
		}),
		_ => unreachable!("MCP run resource kind is selected by static match arms"),
	};

	Ok(value)
}

pub(super) fn mcp_pr_review_state_resource(snapshot: Value) -> Value {
	let review_lanes = mcp_public_post_review_lanes(snapshot.get("post_review_lanes"));
	let current_lane_reviews = mcp_current_lane_runs(&snapshot)
		.into_iter()
		.filter_map(|run| {
			let review = mcp_loop_review_status(run)?;

			Some(serde_json::json!({
				"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
				"issue_id": run.get("issue_id").cloned().unwrap_or(Value::Null),
				"issue_identifier": run.get("issue_identifier").cloned().unwrap_or(Value::Null),
				"review": mcp_public_review_status(review)
			}))
		})
		.collect::<Vec<_>>();

	serde_json::json!({
		"schema": "decodex.mcp.pr_review_state/1",
		"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
		"post_review_lanes": review_lanes,
		"current_lane_reviews": current_lane_reviews
	})
}

fn mcp_run_activity_summaries(runs: Option<&Value>) -> Vec<Value> {
	runs.and_then(Value::as_array).into_iter().flatten().map(mcp_run_activity_summary).collect()
}

fn mcp_public_post_review_lanes(lanes: Option<&Value>) -> Vec<Value> {
	lanes.and_then(Value::as_array).into_iter().flatten().map(mcp_public_post_review_lane).collect()
}

pub(super) fn mcp_public_post_review_lane(lane: &Value) -> Value {
	serde_json::json!({
		"project_id": lane.get("project_id").cloned().unwrap_or(Value::Null),
		"issue_id": lane.get("issue_id").cloned().unwrap_or(Value::Null),
		"issue_identifier": lane.get("issue_identifier").cloned().unwrap_or(Value::Null),
		"issue_state": lane.get("issue_state").cloned().unwrap_or(Value::Null),
		"classification": lane.get("classification").cloned().unwrap_or(Value::Null),
		"reason": lane.get("reason").cloned().unwrap_or(Value::Null),
		"pr_url": lane.get("pr_url").cloned().unwrap_or(Value::Null),
		"pr_state": lane.get("pr_state").cloned().unwrap_or(Value::Null),
		"review_decision": lane.get("review_decision").cloned().unwrap_or(Value::Null),
		"mergeable": lane.get("mergeable").cloned().unwrap_or(Value::Null),
		"check_state": lane.get("check_state").cloned().unwrap_or(Value::Null),
		"unresolved_review_threads": lane
			.get("unresolved_review_threads")
			.cloned()
			.unwrap_or(Value::Null),
		"shadowed_by_current_lane": lane
			.get("shadowed_by_current_lane")
			.cloned()
			.unwrap_or(Value::Null),
		"readback_warning": lane.get("readback_warning").cloned().unwrap_or(Value::Null),
		"readback_root_cause": lane.get("readback_root_cause").cloned().unwrap_or(Value::Null),
		"loop_review": lane
			.get("loop_status")
			.and_then(mcp_loop_review_status_from_loop_status)
			.map(mcp_public_review_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_all_runs(snapshot: &Value) -> Vec<&Value> {
	let mut runs = Vec::new();
	let mut seen_run_ids = BTreeSet::new();

	for key in ["current_lanes", "recent_runs"] {
		if let Some(items) = snapshot.get(key).and_then(Value::as_array) {
			for (index, run) in items.iter().enumerate() {
				let run_key = run
					.get("run_id")
					.and_then(Value::as_str)
					.map(str::to_owned)
					.unwrap_or_else(|| format!("{key}:{index}"));

				if seen_run_ids.insert(run_key) {
					runs.push(run);
				}
			}
		}
	}

	runs
}

fn mcp_current_lane_runs(snapshot: &Value) -> Vec<&Value> {
	snapshot.get("current_lanes").and_then(Value::as_array).into_iter().flatten().collect()
}

fn mcp_find_run<'a>(snapshot: &'a Value, run_id: &str) -> Option<&'a Value> {
	mcp_all_runs(snapshot)
		.into_iter()
		.find(|run| run.get("run_id").and_then(Value::as_str) == Some(run_id))
}

pub(super) fn mcp_run_activity_summary(run: &Value) -> Value {
	serde_json::json!({
		"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
		"issue_id": run.get("issue_id").cloned().unwrap_or(Value::Null),
		"issue_identifier": run.get("issue_identifier").cloned().unwrap_or(Value::Null),
		"attempt_number": run.get("attempt_number").cloned().unwrap_or(Value::Null),
		"status": run.get("status").cloned().unwrap_or(Value::Null),
		"attempt_status": run.get("attempt_status").cloned().unwrap_or(Value::Null),
		"phase": run.get("phase").cloned().unwrap_or(Value::Null),
		"run_phase": run.get("run_phase").cloned().unwrap_or(Value::Null),
		"wait_reason": run.get("wait_reason").cloned().unwrap_or(Value::Null),
		"current_operation": run.get("current_operation").cloned().unwrap_or(Value::Null),
		"lane_control_next_action": run
			.get("lane_control_next_action")
			.cloned()
			.unwrap_or(Value::Null),
		"event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
		"last_event_type": run.get("last_event_type").cloned().unwrap_or(Value::Null),
		"last_event_at": run.get("last_event_at").cloned().unwrap_or(Value::Null),
		"last_protocol_activity_at": run
			.get("last_protocol_activity_at")
			.cloned()
			.unwrap_or(Value::Null),
		"last_progress_at": run.get("last_progress_at").cloned().unwrap_or(Value::Null),
		"protocol_activity": mcp_public_protocol_activity(run),
		"child_agent_activity": run.get("child_agent_activity").cloned().unwrap_or(Value::Null),
		"progress_diagnostic": run.get("progress_diagnostic").cloned().unwrap_or(Value::Null),
		"phase_acceptance": run
			.get("phase_acceptance")
			.map(mcp_public_phase_acceptance_status)
			.unwrap_or(Value::Null),
		"autonomy": mcp_public_autonomy_status(run),
		"loop_review": run
			.get("loop_status")
			.and_then(mcp_loop_review_status_from_loop_status)
			.map(mcp_public_review_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_public_autonomy_status(run_or_lane: &Value) -> Value {
	let Some(loop_status) = run_or_lane.get("loop_status").filter(|status| status.is_object())
	else {
		return Value::Null;
	};

	serde_json::json!({
		"status": loop_status.get("autonomy").cloned().unwrap_or(Value::Null),
		"summary": loop_status.get("summary").cloned().unwrap_or(Value::Null),
		"objective": loop_status
			.get("autonomy_objective")
			.map(mcp_public_autonomy_objective)
			.unwrap_or(Value::Null),
		"signals": mcp_public_autonomy_signals(loop_status.get("autonomy_signals")),
		"proposals": mcp_public_autonomy_proposals(loop_status.get("autonomy_proposals")),
		"lineage": mcp_public_autonomy_lineage(loop_status.get("autonomy_lineage")),
		"report": loop_status
			.get("autonomy_report")
			.map(mcp_public_autonomy_report)
			.unwrap_or(Value::Null)
	})
}

fn mcp_public_autonomy_objective(objective: &Value) -> Value {
	serde_json::json!({
		"objective_id": objective.get("objective_id").cloned().unwrap_or(Value::Null),
		"objective_version": objective
			.get("objective_version")
			.cloned()
			.unwrap_or(Value::Null),
		"state": objective.get("state").cloned().unwrap_or(Value::Null),
		"source_ref": objective.get("source_ref").cloned().unwrap_or(Value::Null),
		"completeness": objective.get("completeness").cloned().unwrap_or(Value::Null),
		"known_gaps": objective.get("known_gaps").cloned().unwrap_or_else(|| serde_json::json!([]))
	})
}

fn mcp_public_autonomy_signals(signals: Option<&Value>) -> Vec<Value> {
	signals
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.map(|signal| {
			let (source_refs, primary_source_refs) = mcp_public_autonomy_signal_refs(signal);

			serde_json::json!({
				"signal_id": signal.get("signal_id").cloned().unwrap_or(Value::Null),
				"objective_id": signal.get("objective_id").cloned().unwrap_or(Value::Null),
				"objective_version": signal.get("objective_version").cloned().unwrap_or(Value::Null),
				"kind": signal.get("kind").cloned().unwrap_or(Value::Null),
				"source_type": signal.get("source_type").cloned().unwrap_or(Value::Null),
				"source_refs": source_refs,
				"source_ref_count": signal_ref_count(signal, "source_refs", "source_ref_count"),
				"primary_source_refs": primary_source_refs,
				"primary_source_ref_count": signal_ref_count(
					signal,
					"primary_source_refs",
					"primary_source_ref_count"
				),
				"freshness": signal.get("freshness").cloned().unwrap_or(Value::Null),
				"evidence_class": signal.get("evidence_class").cloned().unwrap_or(Value::Null),
				"confidence": signal.get("confidence").cloned().unwrap_or(Value::Null),
				"redaction_level": signal
					.get("redaction_level")
					.cloned()
					.unwrap_or(Value::Null),
				"completeness": signal.get("completeness").cloned().unwrap_or(Value::Null),
				"known_gaps": signal
					.get("known_gaps")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"updated_at": signal.get("updated_at").cloned().unwrap_or(Value::Null)
			})
		})
		.collect()
}

fn mcp_public_autonomy_signal_refs(signal: &Value) -> (Value, Value) {
	if signal.get("redaction_level").and_then(Value::as_str) == Some("local_private") {
		return (serde_json::json!([]), serde_json::json!([]));
	}

	(
		signal.get("source_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
		signal.get("primary_source_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
	)
}

fn signal_ref_count(signal: &Value, refs_key: &str, count_key: &str) -> u64 {
	signal.get(count_key).and_then(Value::as_u64).unwrap_or_else(|| {
		signal.get(refs_key).and_then(Value::as_array).map_or(0, |refs| refs.len() as u64)
	})
}

fn mcp_public_autonomy_proposals(proposals: Option<&Value>) -> Vec<Value> {
	proposals
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.map(|proposal| {
			serde_json::json!({
				"proposal_id": proposal.get("proposal_id").cloned().unwrap_or(Value::Null),
				"objective_id": proposal.get("objective_id").cloned().unwrap_or(Value::Null),
				"objective_version": proposal.get("objective_version").cloned().unwrap_or(Value::Null),
				"state": proposal.get("state").cloned().unwrap_or(Value::Null),
				"summary": proposal.get("summary").cloned().unwrap_or(Value::Null),
				"source_family": proposal.get("source_family").cloned().unwrap_or(Value::Null),
				"intended_surface": proposal
					.get("intended_surface")
					.cloned()
					.unwrap_or(Value::Null),
				"source_signal_ids": proposal
					.get("source_signal_ids")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"refusal_reasons": proposal
					.get("refusal_reasons")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"refusals": proposal
					.get("refusals")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"completeness": proposal.get("completeness").cloned().unwrap_or(Value::Null),
				"known_gaps": proposal
					.get("known_gaps")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"updated_at": proposal.get("updated_at").cloned().unwrap_or(Value::Null)
			})
		})
		.collect()
}

fn mcp_public_autonomy_lineage(lineage: Option<&Value>) -> Vec<Value> {
	lineage.and_then(Value::as_array).into_iter().flatten().cloned().collect()
}

fn mcp_public_autonomy_report(report: &Value) -> Value {
	serde_json::json!({
		"surface": report.get("surface").cloned().unwrap_or(Value::Null),
		"authority": report.get("authority").cloned().unwrap_or(Value::Null),
		"audit_authority": report.get("audit_authority").cloned().unwrap_or(Value::Null),
		"source_refs": report.get("source_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
		"redaction_level": report.get("redaction_level").cloned().unwrap_or(Value::Null),
		"completeness": report.get("completeness").cloned().unwrap_or(Value::Null),
		"known_gaps": report.get("known_gaps").cloned().unwrap_or_else(|| serde_json::json!([]))
	})
}

fn mcp_public_phase_acceptance_status(acceptance: &Value) -> Value {
	serde_json::json!({
		"phase": acceptance.get("phase").cloned().unwrap_or(Value::Null),
		"decision": acceptance.get("decision").cloned().unwrap_or(Value::Null),
		"reason_code": acceptance.get("reason_code").cloned().unwrap_or(Value::Null),
		"objective_covered": acceptance.get("objective_covered").cloned().unwrap_or(Value::Null),
		"effective_delta_present": acceptance
			.get("effective_delta_present")
			.cloned()
			.unwrap_or(Value::Null),
		"non_goal_passed": acceptance.get("non_goal_passed").cloned().unwrap_or(Value::Null),
		"validation_passed": acceptance.get("validation_passed").cloned().unwrap_or(Value::Null),
		"next_action": acceptance.get("next_action").cloned().unwrap_or(Value::Null)
	})
}

fn mcp_public_review_status(review: &Value) -> Value {
	serde_json::json!({
		"phase": review.get("phase").cloned().unwrap_or(Value::Null),
		"status": review.get("status").cloned().unwrap_or(Value::Null),
		"checkpoint": review
			.get("checkpoint")
			.map(mcp_public_review_checkpoint_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_loop_review_status(run_or_lane: &Value) -> Option<&Value> {
	run_or_lane.get("loop_status").and_then(mcp_loop_review_status_from_loop_status)
}

fn mcp_loop_review_status_from_loop_status(loop_status: &Value) -> Option<&Value> {
	loop_status.get("review").filter(|review| review.is_object())
}

fn mcp_public_review_checkpoint_status(checkpoint: &Value) -> Value {
	serde_json::json!({
		"round": checkpoint.get("round").cloned().unwrap_or(Value::Null),
		"nonclean_rounds": checkpoint.get("nonclean_rounds").cloned().unwrap_or(Value::Null),
		"updated_at": checkpoint.get("updated_at").cloned().unwrap_or(Value::Null)
	})
}

fn mcp_public_protocol_activity(run: &Value) -> Value {
	let mut activity = run.get("protocol_activity").cloned().unwrap_or(Value::Null);

	redact_reasoning_protocol_activity(&mut activity);

	activity
}

fn redact_reasoning_protocol_activity(value: &mut Value) {
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

pub(super) fn sanitize_mcp_observability_value(value: &mut Value) {
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

pub(super) fn mcp_sanitized_value(mut value: Value) -> Value {
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
