use agent::{
	AppServerSteerChannelRequest, AppServerSteerChannelRequestInput, AppServerSteerQueueReport,
};
use state::{RUN_CONTROL_ACTION_ACCEPTED, RunControlActionReceipt, RunControlActionRequest};

struct LaneSteerTarget {
	run: ProjectRunStatus,
	issue_identifier: Option<String>,
}

struct LaneSteerQueueReportInput<'a> {
	target: &'a LaneSteerTarget,
	request_id: &'a str,
	audit_record_id: i64,
	project_id: &'a str,
	expected_turn_id: &'a str,
	message_byte_count: usize,
	message_line_count: usize,
	queue: AppServerSteerQueueReport,
}

pub(crate) fn steer_lane(request: LaneSteerRequest<'_>) -> Result<LaneSteerReport> {
	validate_lane_steer_request(&request)?;

	let state_store = runtime::open_runtime_store()?;
	let config_path = resolve_lane_steer_config_path(&state_store, request.config_path, request.project_id)?;
	let config = ServiceConfig::from_path(&config_path)?;

	if let Some(project_id) = request.project_id
		&& project_id != config.service_id()
	{
		eyre::bail!(
			"Lane steer project `{project_id}` did not match config service id `{}`.",
			config.service_id()
		);
	}

	runtime::register_project_config(&state_store, &config_path, true)?;

	let target = resolve_lane_steer_target(&state_store, &config, request.issue, request.run_id)?;
	let request_id = new_lane_steer_request_id();
	let message_byte_count = request.message.len();
	let message_line_count = lane_steer_message_line_count(request.message);
	let metadata = serde_json::json!({
		"request_id": request_id,
		"expected_turn_id": request.expected_turn_id,
		"message_byte_count": message_byte_count,
		"message_line_count": message_line_count,
	});
	let timeout_ms = i64::try_from(request.wait_timeout.as_millis()).unwrap_or(i64::MAX);
	let receipt = state_store.resolve_run_control_action(RunControlActionRequest {
		project_id: config.service_id(),
		issue_id: target.run.issue_id(),
		run_id: target.run.run_id(),
		attempt_number: target.run.attempt_number(),
		thread_id: target.run.thread_id(),
		turn_id: Some(request.expected_turn_id),
		source: request.source,
		action: "steer",
		timeout_ms: Some(timeout_ms),
		metadata: Some(&metadata),
	})?;

	if receipt.outcome() != RUN_CONTROL_ACTION_ACCEPTED {
		return Ok(lane_steer_report_from_rejected_receipt(
			&target,
			&request_id,
			&receipt,
			request.expected_turn_id,
			message_byte_count,
			message_line_count,
		));
	}

	let Some(channel) = receipt.channel() else {
		eyre::bail!("Lane steer was accepted without an active control channel.");
	};
	let Some(thread_id) = target.run.thread_id() else {
		eyre::bail!("Lane steer was accepted before the active app-server thread id was known.");
	};
	let steer_request = AppServerSteerChannelRequest::new(AppServerSteerChannelRequestInput {
		request_id: request_id.clone(),
		audit_record_id: receipt.audit_record_id(),
		project_id: receipt.project_id().to_owned(),
		issue_id: receipt.issue_id().to_owned(),
		run_id: receipt.run_id().to_owned(),
		attempt_number: receipt.attempt_number(),
		thread_id: thread_id.to_owned(),
		expected_turn_id: request.expected_turn_id.to_owned(),
		source: request.source.to_owned(),
		message: request.message.to_owned(),
	});
	let queue = agent::enqueue_app_server_steer_request(channel, &steer_request, request.wait_timeout)?;

	Ok(lane_steer_report_from_queue_result(LaneSteerQueueReportInput {
		target: &target,
		request_id: &request_id,
		audit_record_id: receipt.audit_record_id(),
		project_id: receipt.project_id(),
		expected_turn_id: request.expected_turn_id,
		message_byte_count,
		message_line_count,
		queue,
	}))
}

fn validate_lane_steer_request(request: &LaneSteerRequest<'_>) -> Result<()> {
	if request.issue.trim().is_empty() {
		eyre::bail!("Lane steer issue must not be empty.");
	}
	if request.run_id.trim().is_empty() {
		eyre::bail!("Lane steer run id must not be empty.");
	}
	if request.expected_turn_id.trim().is_empty() {
		eyre::bail!("Lane steer expected turn id must not be empty.");
	}
	if request.message.trim().is_empty() {
		eyre::bail!("Lane steer message must not be empty.");
	}
	if request.source.trim().is_empty() {
		eyre::bail!("Lane steer source must not be empty.");
	}

	Ok(())
}

fn resolve_lane_steer_config_path(
	state_store: &StateStore,
	config_path: Option<&Path>,
	project_id: Option<&str>,
) -> Result<PathBuf> {
	if let Some(project_id) = project_id {
		if let Some(config_path) = config_path {
			return ServiceConfig::resolve_project_config_path(config_path);
		}

		return state_store
			.list_projects()?
			.into_iter()
			.find(|project| project.service_id() == project_id)
			.map(|project| project.config_path().to_path_buf())
			.ok_or_else(|| {
				eyre::eyre!(
					"Decodex project `{project_id}` is not registered. Pass --config or run `decodex project add`."
				)
			});
	}

	resolve_config_path(config_path, state_store)?.ok_or_else(|| {
		eyre::eyre!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		)
	})
}

fn resolve_lane_steer_target(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue: &str,
	run_id: &str,
) -> Result<LaneSteerTarget> {
	let (_active_runs, recent_runs) = state_store.list_project_runs(project.service_id(), usize::MAX)?;
	let matches = recent_runs
		.into_iter()
		.filter(|run| run.run_id() == run_id)
		.filter(|run| private_evidence_run_matches_issue(project, run, issue))
		.map(|run| {
			let branch_name = run.branch_name().map(str::to_owned);
			let worktree_path = run
				.worktree_path()
				.map(|path| relative_worktree_path_for_path(project, path));
			let issue_identifier = operator_run_issue_identifier_from_fields(
				run.run_id(),
				branch_name.as_deref(),
				worktree_path.as_deref(),
			);

			LaneSteerTarget { run, issue_identifier }
		})
		.collect::<Vec<_>>();

	match matches.len() {
		0 => eyre::bail!(
			"No local run matched issue `{issue}` and run id `{run_id}` in project `{}`.",
			project.service_id()
		),
		1 => {
			let mut matches = matches;

			Ok(matches.remove(0))
		},
		_ => eyre::bail!(
			"Lane steer matched multiple local runs for issue `{issue}` and run id `{run_id}`."
		),
	}
}

fn lane_steer_report_from_rejected_receipt(
	target: &LaneSteerTarget,
	request_id: &str,
	receipt: &RunControlActionReceipt,
	expected_turn_id: &str,
	message_byte_count: usize,
	message_line_count: usize,
) -> LaneSteerReport {
	LaneSteerReport {
		project_id: receipt.project_id().to_owned(),
		issue_id: receipt.issue_id().to_owned(),
		issue_identifier: target.issue_identifier.clone(),
		run_id: receipt.run_id().to_owned(),
		attempt_number: receipt.attempt_number(),
		thread_id: receipt.current_thread_id().map(str::to_owned),
		expected_turn_id: expected_turn_id.to_owned(),
		current_turn_id: receipt.current_turn_id().map(str::to_owned),
		response_turn_id: None,
		audit_record_id: receipt.audit_record_id(),
		request_id: request_id.to_owned(),
		request_path: None,
		outcome: receipt.outcome().to_owned(),
		reason: receipt.reason().to_owned(),
		failure_class: lane_steer_failure_class_for_reason(receipt.reason()).map(str::to_owned),
		delivery_status: String::from("rejected"),
		message_byte_count,
		message_line_count,
	}
}

fn lane_steer_report_from_queue_result(input: LaneSteerQueueReportInput<'_>) -> LaneSteerReport {
	let result = input.queue.result;

	LaneSteerReport {
		project_id: input.project_id.to_owned(),
		issue_id: input.target.run.issue_id().to_owned(),
		issue_identifier: input.target.issue_identifier.clone(),
		run_id: input.target.run.run_id().to_owned(),
		attempt_number: input.target.run.attempt_number(),
		thread_id: input.target.run.thread_id().map(str::to_owned),
		expected_turn_id: input.expected_turn_id.to_owned(),
		current_turn_id: result
			.as_ref()
			.and_then(|result| result.current_turn_id.clone())
			.or_else(|| input.target.run.turn_id().map(str::to_owned)),
		response_turn_id: result.as_ref().and_then(|result| result.response_turn_id.clone()),
		audit_record_id: input.audit_record_id,
		request_id: input.request_id.to_owned(),
		request_path: Some(input.queue.request_path.display().to_string()),
		outcome: result
			.as_ref()
			.map_or_else(|| RUN_CONTROL_ACTION_ACCEPTED.to_owned(), |result| result.outcome.clone()),
		reason: result
			.as_ref()
			.map_or_else(|| String::from("queued_wait_timeout"), |result| result.reason.clone()),
		failure_class: result.as_ref().and_then(|result| result.failure_class.clone()),
		delivery_status: result
			.as_ref()
			.map_or_else(|| String::from("queued"), |_| String::from("resolved")),
		message_byte_count: input.message_byte_count,
		message_line_count: input.message_line_count,
	}
}

fn lane_steer_failure_class_for_reason(reason: &str) -> Option<&'static str> {
	match reason {
		"turn_mismatch" => Some("stale_expected_turn_id"),
		"active_turn_not_steerable" => Some("active_turn_not_steerable"),
		"app_server_turn_steer_unsupported" => Some("app_server_turn_steer_unsupported"),
		_ => Some("run_control_action_failed"),
	}
}

fn new_lane_steer_request_id() -> String {
	let now = OffsetDateTime::now_utc().unix_timestamp_nanos();

	format!("steer-{now}-{}", process::id())
}

fn lane_steer_message_line_count(message: &str) -> usize {
	message.lines().count().max(usize::from(!message.is_empty()))
}
