use super::{
	ChildAgentActivitySummary, Connection, ExecutionProgramRuntimeRecord, Result, StateData,
	Transaction, derived_program_intake_plan_records, derived_program_issue_mapping_records, eyre,
	params, sqlite_bool_value,
};

pub(super) fn persist_projects(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for project in state.projects.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO projects (
					service_id, config_path, repo_root, worktree_root, workflow_path,
					tracker_api_key_env_var, github_token_env_var, enabled, config_fingerprint,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				project.service_id(),
				project.config_path().to_string_lossy().as_ref(),
				project.repo_root().to_string_lossy().as_ref(),
				project.worktree_root().to_string_lossy().as_ref(),
				project.workflow_path().to_string_lossy().as_ref(),
				project.tracker_api_key_env_var(),
				project.github_token_env_var(),
				if project.enabled() { 1_i64 } else { 0_i64 },
				project.config_fingerprint(),
				project.updated_at(),
				project.updated_at_unix(),
			],
		)?;
	}

	Ok(())
}

pub(super) fn update_run_attempt_project(
	transaction: &Transaction<'_>,
	project_id: &str,
	issue_id: &str,
	run_id: Option<&str>,
) -> Result<()> {
	match run_id {
		Some(run_id) => {
			transaction.execute(
				"UPDATE run_attempts SET project_id = ?1 WHERE issue_id = ?2 AND run_id = ?3",
				params![project_id, issue_id, run_id],
			)?;
		},
		None => {
			transaction.execute(
				"UPDATE run_attempts SET project_id = ?1 WHERE issue_id = ?2",
				params![project_id, issue_id],
			)?;
		},
	}

	Ok(())
}

pub(super) fn persist_leases(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for lease in state.leases.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO leases (issue_id, project_id, run_id, issue_state) \
				 VALUES (?1, ?2, ?3, ?4)",
			params![lease.issue_id(), lease.project_id(), lease.run_id(), lease.issue_state()],
		)?;
	}

	Ok(())
}

pub(super) fn persist_run_attempts(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for attempt in state.run_attempts.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO run_attempts (
					run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&attempt.run_id,
				attempt.project_id.as_deref(),
				&attempt.issue_id,
				attempt.attempt_number,
				&attempt.status,
				attempt.thread_id.as_deref(),
				attempt.turn_id.as_deref(),
				&attempt.updated_at,
				attempt.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_run_control_channels(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for channel in state.control_channels.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO run_control_channels (
					run_id, project_id, issue_id, attempt_number, transport, channel_path, status,
					published_at, published_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&channel.run_id,
				&channel.project_id,
				&channel.issue_id,
				channel.attempt_number,
				&channel.transport,
				channel.channel_path.to_string_lossy().as_ref(),
				&channel.status,
				&channel.published_at,
				channel.published_at_unix,
				&channel.updated_at,
				channel.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_protocol_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for (run_id, events) in &state.events {
		for event in events {
			transaction.execute(
				"INSERT OR REPLACE INTO protocol_events (
						run_id, sequence_number, event_type, payload_sha256, created_at,
						created_at_unix
					) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
				params![
					run_id,
					event.sequence_number,
					&event.event_type,
					&event.payload_sha256,
					&event.created_at,
					event.created_at_unix,
				],
			)?;
		}
	}

	Ok(())
}

pub(super) fn persist_run_activity_summaries(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for summary in state.run_activity_summaries.values() {
		let child_agent_activity_json = summary
			.child_agent_activity
			.as_ref()
			.cloned()
			.map(ChildAgentActivitySummary::sealed_durable)
			.map(|summary| serde_json::to_string(&summary))
			.transpose()?;
		let protocol_activity_json =
			summary.protocol_activity.as_ref().map(serde_json::to_string).transpose()?;

		transaction.execute(
			"INSERT OR REPLACE INTO run_activity_summaries (
					run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			params![
				&summary.run_id,
				summary.attempt_number,
				child_agent_activity_json.as_deref(),
				protocol_activity_json.as_deref(),
				&summary.updated_at,
				summary.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_worktrees(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for mapping in state.worktrees.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO worktrees (
				issue_id, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
			params![
				&mapping.issue_id,
				&mapping.project_id,
				&mapping.branch_name,
				mapping.worktree_path.to_string_lossy().as_ref(),
				&mapping.provenance_source,
				mapping.created_at_unix,
				mapping.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_linear_execution_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.linear_execution_events.values() {
		let payload_json = serde_json::to_string(&record.record)?;

		transaction.execute(
			"INSERT OR REPLACE INTO linear_execution_events (
					idempotency_key, service_id, issue_id, event_type, event_timestamp,
					event_unix, payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&record.record.idempotency_key,
				&record.record.service_id,
				&record.record.issue_id,
				&record.record.event_type,
				&record.record.event_timestamp,
				record.event_unix,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_private_execution_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in &state.private_execution_events {
		let payload_json = serde_json::to_string(&record.payload)?;

		transaction.execute(
			"INSERT OR REPLACE INTO private_execution_events (
					record_id, project_id, issue_id, run_id, attempt_number, event_type,
					payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				record.record_id,
				&record.project_id,
				&record.issue_id,
				&record.run_id,
				record.attempt_number,
				&record.event_type,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_decision_contracts(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.decision_contracts.values() {
		let payload_json = serde_json::to_string(&record.contract)?;

		transaction.execute(
			"INSERT OR REPLACE INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&record.project_id,
				record.contract.contract_id(),
				record.source_issue_id.as_deref(),
				record.status.as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_autonomy_objectives(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.autonomy_objectives.values() {
		let payload_json = serde_json::to_string(&record.objective)?;
		let version = i64::try_from(record.objective.version())
			.map_err(|_| eyre::eyre!("Autonomy objective version exceeds SQLite integer range."))?;

		transaction.execute(
			"INSERT OR REPLACE INTO autonomy_objectives (
					project_id, objective_id, version, state, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&record.project_id,
				record.objective.id(),
				version,
				record.state.as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_autonomy_signals(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.autonomy_signals.values() {
		let payload_json = serde_json::to_string(&record.signal)?;
		let version = i64::try_from(record.signal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;

		transaction.execute(
			"INSERT OR REPLACE INTO autonomy_signals (
					project_id, signal_id, objective_id, objective_version, kind, fingerprint,
					freshness, evidence_class, confidence, privacy, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
			params![
				&record.project_id,
				record.signal.id(),
				record.signal.objective_id(),
				version,
				record.signal.kind().as_str(),
				record.signal.fingerprint(),
				record.signal.freshness().as_str(),
				record.signal.evidence_class().as_str(),
				record.signal.confidence().as_str(),
				record.signal.privacy().as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_autonomy_proposals(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.autonomy_proposals.values() {
		let payload_json = serde_json::to_string(&record.proposal)?;
		let version = i64::try_from(record.proposal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy proposal objective_version exceeds SQLite integer range.")
		})?;

		transaction.execute(
			"INSERT OR REPLACE INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
			params![
				&record.project_id,
				record.proposal.id(),
				record.proposal.objective_id(),
				version,
				record.state.as_str(),
				record.proposal.fingerprint(),
				record.proposal.source_family(),
				record.proposal.intended_surface(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_execution_programs(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.execution_programs.values() {
		let payload_json = serde_json::to_string(&record.program)?;

		transaction.execute(
			"INSERT OR REPLACE INTO execution_programs (
					project_id, program_id, source_contract_id, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			params![
				&record.project_id,
				record.program.program_id(),
				record.source_contract_id.as_deref(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_program_intake_state(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.program_intake_plans.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO program_intake_plans (
					project_id, program_id, plan_id, intake_kind, source_contract_id,
					accepted_contract_fingerprint, public_summary, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&record.project_id,
				&record.program_id,
				&record.plan_id,
				&record.intake_kind,
				record.source_contract_id.as_deref(),
				&record.accepted_contract_fingerprint,
				&record.public_summary,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}
	for record in state.program_issue_mappings.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO program_issue_mappings (
					project_id, program_id, node_id, issue_id, issue_identifier, issue_state,
					queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label,
					has_generic_dispatch_briefing, created_at, created_at_unix, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
			params![
				&record.project_id,
				&record.program_id,
				&record.node_id,
				&record.issue_id,
				&record.issue_identifier,
				&record.issue_state,
				&record.queue_intent,
				sqlite_bool_value(record.has_active_label),
				sqlite_bool_value(record.has_opt_out_label),
				sqlite_bool_value(record.has_needs_attention_label),
				sqlite_bool_value(record.has_generic_dispatch_briefing),
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn insert_program_intake_state(
	connection: &Connection,
	record: &ExecutionProgramRuntimeRecord,
) -> Result<()> {
	for plan in derived_program_intake_plan_records(record) {
		connection.execute(
			"INSERT OR REPLACE INTO program_intake_plans (
					project_id, program_id, plan_id, intake_kind, source_contract_id,
					accepted_contract_fingerprint, public_summary, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&plan.project_id,
				&plan.program_id,
				&plan.plan_id,
				&plan.intake_kind,
				plan.source_contract_id.as_deref(),
				&plan.accepted_contract_fingerprint,
				&plan.public_summary,
				&plan.created_at,
				plan.created_at_unix,
				&plan.updated_at,
				plan.updated_at_unix,
			],
		)?;
	}
	for mapping in derived_program_issue_mapping_records(record) {
		connection.execute(
			"INSERT OR REPLACE INTO program_issue_mappings (
					project_id, program_id, node_id, issue_id, issue_identifier, issue_state,
					queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label,
					has_generic_dispatch_briefing, created_at, created_at_unix, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
			params![
				&mapping.project_id,
				&mapping.program_id,
				&mapping.node_id,
				&mapping.issue_id,
				&mapping.issue_identifier,
				&mapping.issue_state,
				&mapping.queue_intent,
				sqlite_bool_value(mapping.has_active_label),
				sqlite_bool_value(mapping.has_opt_out_label),
				sqlite_bool_value(mapping.has_needs_attention_label),
				sqlite_bool_value(mapping.has_generic_dispatch_briefing),
				&mapping.created_at,
				mapping.created_at_unix,
				&mapping.updated_at,
				mapping.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_review_lifecycle_records(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.review_lifecycle_records.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO review_lifecycle_records (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha,
					phase, request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, landing_state, closeout_state,
					repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix
				) VALUES (
					?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
					?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24
				)",
			params![
				record.project_id,
				record.issue_id,
				record.branch_name,
				record.run_id,
				record.attempt_number,
				record.pr_url,
				record.target_base_ref_name,
				record.pr_head_ref_name,
				record.pr_head_oid,
				record.head_sha,
				record.phase,
				record.request_comment_database_id,
				record.request_created_at_unix_epoch,
				record
					.request_description_thumbs_up_count
					.and_then(|count| i64::try_from(count).ok()),
				record.request_retry_count,
				record.external_round_count,
				record.auto_merge_enabled_at_unix_epoch,
				record.landing_state,
				record.closeout_state,
				record.repair_attempt_count,
				record.evidence_json,
				record.next_action,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_review_policy_checkpoints(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.review_policy_checkpoints.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO review_policy_checkpoints (
					project_id, issue_id, run_id, attempt_number, phase, status, head_sha,
					nonclean_rounds, details_json, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				record.project_id,
				record.issue_id,
				record.run_id,
				record.attempt_number,
				record.phase,
				record.status,
				record.head_sha,
				record.nonclean_rounds,
				record.details_json,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_evidence_artifacts(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.evidence_artifacts.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO evidence_artifacts (
					project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha,
					key_json, payload_json, source_run_id, source_attempt_number, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
			params![
				record.project_id,
				record.issue_id,
				record.artifact_kind,
				record.key_hash,
				record.phase,
				record.status,
				record.head_sha,
				record.key_json,
				record.payload_json,
				record.source_run_id,
				record.source_attempt_number,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_loop_guardrail_checkpoints(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.loop_guardrail_checkpoints.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO loop_guardrail_checkpoints (
					project_id, issue_id, reason, fingerprint, run_id, attempt_number,
					consecutive_count, details_json, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
			params![
				record.project_id,
				record.issue_id,
				record.reason,
				record.fingerprint,
				record.run_id,
				record.attempt_number,
				record.consecutive_count,
				record.details_json,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(super) fn persist_connector_backoffs(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.connector_backoffs.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO connector_backoffs (
					project_id, connector, sync_phase, quota_class, reset_unix_epoch,
					reset_source, warning, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				record.project_id,
				record.connector,
				record.sync_phase,
				record.quota_class,
				record.reset_unix_epoch,
				record.reset_source,
				record.warning,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}
