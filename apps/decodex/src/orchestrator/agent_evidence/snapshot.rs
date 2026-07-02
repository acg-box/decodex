use crate::orchestrator::agent_evidence::{
	self, AGENT_HANDOFF_INDEX_SCHEMA, AgentEvidenceFileWriteContext, AgentEvidenceProjectView,
	AgentEvidenceSource, AgentEvidenceSummary, AgentEvidenceWriteResult, AgentHandoffIndex,
	BLOCKERS_DIR_NAME, EVENTS_FILE_NAME, HANDOFF_INDEX_FILE_NAME, OperatorStatusSnapshot,
	RUNS_DIR_NAME, Result, agent_connector_backoff, agent_recovery_contract, run_capsule_ref,
	runtime,
};

pub(in crate::orchestrator) fn write_agent_evidence_snapshot(
	snapshot: &OperatorStatusSnapshot,
	source: AgentEvidenceSource,
) -> Result<Vec<AgentEvidenceWriteResult>> {
	let generated_at = agent_evidence::current_timestamp();
	let month_bucket = agent_evidence::current_month_bucket();
	let mut results = Vec::new();

	for project_id in agent_evidence::agent_evidence_project_ids(snapshot) {
		let service_root = runtime::agent_evidence_dir()?.join(&project_id);
		let handoff_index_path = service_root.join(HANDOFF_INDEX_FILE_NAME);
		let blockers_dir = service_root.join(BLOCKERS_DIR_NAME);
		let runs_dir = service_root.join(RUNS_DIR_NAME);
		let events_path = service_root.join(EVENTS_FILE_NAME);
		let project_view = AgentEvidenceProjectView::from_snapshot(snapshot, &project_id);
		let mut run_capsules = agent_evidence::build_run_capsules(
			&project_view,
			&generated_at,
			&runs_dir,
			&month_bucket,
		);

		run_capsules.sort_by(|left, right| {
			left.issue_identifier
				.cmp(&right.issue_identifier)
				.then_with(|| left.issue_id.cmp(&right.issue_id))
				.then_with(|| left.attempt_number.cmp(&right.attempt_number))
				.then_with(|| left.run_id.cmp(&right.run_id))
		});

		let run_refs = run_capsules.iter().map(run_capsule_ref).collect::<Vec<_>>();
		let blockers =
			agent_evidence::build_agent_blockers(&project_view, &blockers_dir, &run_refs);
		let recovery_worktrees = project_view
			.recovery_worktrees
			.iter()
			.map(|(role, worktree)| agent_evidence::agent_recovery_worktree(role, worktree))
			.collect::<Vec<_>>();
		let recovery_contracts = blockers.iter().filter_map(agent_recovery_contract).collect();
		let connector_backoffs = project_view
			.connector_backoffs
			.iter()
			.copied()
			.map(agent_connector_backoff)
			.collect::<Vec<_>>();
		let summary = AgentEvidenceSummary {
			project_count: project_view.projects.len(),
			current_lane_count: project_view.current_lanes.len(),
			recent_run_count: project_view.recent_runs.len(),
			history_lane_count: project_view.history_lanes.len(),
			queued_candidate_count: project_view.queued_candidates.len(),
			post_review_lane_count: project_view.post_review_lanes.len(),
			recovery_worktree_count: recovery_worktrees.len(),
			blocker_count: blockers.len(),
			run_capsule_count: run_refs.len(),
			connector_backoff_count: connector_backoffs.len(),
			warning_count: project_view.warnings.len(),
		};
		let github_cli_authority =
			project_view.projects.first().map(|project| project.github_cli_authority.clone());
		let index = AgentHandoffIndex {
			schema: AGENT_HANDOFF_INDEX_SCHEMA,
			project_id: project_id.clone(),
			generated_at: generated_at.clone(),
			source: source.as_str().to_owned(),
			evidence_root: service_root.display().to_string(),
			handoff_index_path: handoff_index_path.display().to_string(),
			blockers_dir: blockers_dir.display().to_string(),
			runs_dir: runs_dir.display().to_string(),
			events_path: events_path.display().to_string(),
			summary,
			github_cli_authority,
			warnings: project_view.warnings.clone(),
			connector_backoffs,
			blockers,
			run_capsules: run_refs,
			recovery_worktrees,
			recovery_contracts,
		};
		let write_context = AgentEvidenceFileWriteContext {
			project_id: &project_id,
			generated_at: &generated_at,
			source,
			handoff_index_path: &handoff_index_path,
			blockers_dir: &blockers_dir,
			events_path: &events_path,
		};

		agent_evidence::write_agent_evidence_files(&write_context, &index, &run_capsules)?;

		results.push(AgentEvidenceWriteResult {
			project_id,
			handoff_index_path: handoff_index_path.display().to_string(),
			handoff_index: index,
		});
	}

	Ok(results)
}

pub(in crate::orchestrator) fn write_agent_evidence_best_effort(
	snapshot: &OperatorStatusSnapshot,
	source: AgentEvidenceSource,
) {
	if let Err(error) = write_agent_evidence_snapshot(snapshot, source) {
		let _ = error;

		tracing::warn!(
			"Agent evidence write failed; sensitive runtime details were withheld from logs."
		);
	}
}

pub(in crate::orchestrator) fn render_agent_evidence_write_result(
	result: &AgentEvidenceWriteResult,
) -> String {
	format!(
		"agent evidence written: project={} blockers={} run_capsules={} warnings={} index={}\n",
		result.project_id,
		result.handoff_index.summary.blocker_count,
		result.handoff_index.summary.run_capsule_count,
		result.handoff_index.summary.warning_count,
		result.handoff_index_path,
	)
}
