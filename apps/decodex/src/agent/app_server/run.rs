use crate::{
	active_run_env::ActiveRunCommitContext,
	agent::app_server::{
		markers, preflight,
		protocol::AppServerClient,
		runtime_types::{AppServerRunRequest, AppServerRunResult, RunRecorder},
		session, turn_loop,
	},
	prelude::Result,
	state::{RUN_CONTROL_CHANNEL_STATUS_COMPLETED, RUN_CONTROL_CHANNEL_STATUS_FAILED, StateStore},
};

pub(crate) fn execute_app_server_run(
	request: &AppServerRunRequest<'_>,
	state_store: &StateStore,
) -> Result<AppServerRunResult> {
	state_store.record_lane_run_attempt(
		&request.project_id,
		&request.run_id,
		&request.issue_id,
		request.attempt_number,
		"starting",
	)?;

	if let Some(marker_path) = request.activity_marker_path.as_ref() {
		markers::write_activity_marker_best_effort(
			marker_path,
			&request.run_id,
			request.attempt_number,
		);
	}

	let control_channel = markers::publish_run_control_channel_for_request(request, state_store)?;
	let result = self::execute_app_server_run_inner(request, state_store);

	match &result {
		Ok(_result) =>
			if control_channel.is_some() {
				state_store.retire_run_control_channel_for_attempt(
					&request.run_id,
					request.attempt_number,
					RUN_CONTROL_CHANNEL_STATUS_COMPLETED,
				)?;
			},
		Err(_error) => {
			state_store.record_lane_run_attempt(
				&request.project_id,
				&request.run_id,
				&request.issue_id,
				request.attempt_number,
				"failed",
			)?;

			if control_channel.is_some() {
				state_store.retire_run_control_channel_for_attempt(
					&request.run_id,
					request.attempt_number,
					RUN_CONTROL_CHANNEL_STATUS_FAILED,
				)?;
			}

			if let Some(marker_path) = request.activity_marker_path.as_ref() {
				markers::write_activity_marker_best_effort(
					marker_path,
					&request.run_id,
					request.attempt_number,
				);
			}
		},
	}

	result
}

fn execute_app_server_run_inner(
	request: &AppServerRunRequest<'_>,
	state_store: &StateStore,
) -> Result<AppServerRunResult> {
	let mut recorder = RunRecorder::new_with_context(
		state_store,
		&request.project_id,
		&request.issue_id,
		&request.run_id,
		request.attempt_number,
		request.activity_marker_path.as_ref(),
	);
	let process_env =
		request.process_env.clone().with_active_run_commit_context(ActiveRunCommitContext::new(
			request.project_id.clone(),
			request.run_id.clone(),
			request.issue_id.clone(),
			request.issue_identifier.clone(),
		));
	let expected_codex_home = process_env.resolve_codex_home_env()?;
	let mut client = AppServerClient::spawn(&request.listen, &process_env)?;
	let initialize_response = session::initialize_client_for_run(
		&mut client,
		&mut recorder,
		request.dynamic_tool_handler,
		&expected_codex_home,
	)?;

	client.mark_initialized()?;

	markers::write_capability_preflight_marker_best_effort(request);

	let capability_preflight =
		preflight::run_app_server_capability_preflight(&mut client, &mut recorder, &request.cwd)?;

	markers::write_activity_marker_best_effort_for_request(request);

	if let Some(health_check) = request.command_exec_health_check.as_ref() {
		preflight::run_command_exec_health_check(
			&mut client,
			&mut recorder,
			request,
			health_check,
		)?;
	}

	turn_loop::flush_pending_messages(&mut client, &mut recorder, None)?;
	session::login_codex_account_for_run(&mut client, &mut recorder, request)?;
	turn_loop::flush_pending_messages(&mut client, &mut recorder, None)?;

	let thread_response =
		session::start_or_resume_thread_session(&mut client, &mut recorder, request)?;
	let thread_id = thread_response.thread.id.clone();
	let effective_thread_config = thread_response.effective_config();

	session::record_thread_session_start(
		state_store,
		request,
		&mut recorder,
		&thread_id,
		&effective_thread_config,
	)?;
	turn_loop::flush_pending_messages(&mut client, &mut recorder, Some(&thread_id))?;

	state_store.record_lane_run_attempt(
		&request.project_id,
		&request.run_id,
		&request.issue_id,
		request.attempt_number,
		"running",
	)?;
	recorder.mark_activity()?;

	let turn_result =
		turn_loop::execute_turn_loop(&mut client, &mut recorder, request, state_store, &thread_id)?;

	state_store.record_lane_run_attempt(
		&request.project_id,
		&request.run_id,
		&request.issue_id,
		request.attempt_number,
		"succeeded",
	)?;
	recorder.mark_activity()?;

	Ok(AppServerRunResult {
		user_agent: initialize_response.user_agent,
		capability_preflight,
		thread_id,
		turn_id: turn_result.turn_id,
		turn_count: turn_result.turn_count,
		event_count: state_store.event_count(&request.run_id)?,
		final_output: turn_result.final_output,
		continuation_pending: turn_result.continuation_pending,
		phase_goal_status: turn_result.phase_goal_status,
	})
}
