use crate::{
	orchestrator::lane_control::{
		LaneAuthorityReadbackRequest,
		reports::{AuthorityAuditReport, AuthorityTimelineEntry, AuthorityTimelineReport},
	},
	prelude::{Result, eyre},
	runtime,
};

pub(crate) fn print_lane_authority_timeline(
	request: LaneAuthorityReadbackRequest<'_>,
) -> Result<()> {
	let (report, _) = read_authority_reports(request)?;
	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		println!(
			"authority timeline: project={} issue={} events={}",
			report.project_key, report.tracker_issue_id, report.event_count
		);
		for event in report.events {
			println!(
				"- sequence={} type={} decision={} reasons={} transition={} operation={}",
				event.sequence,
				event.event_type,
				event.decision,
				event.reason_codes.join(","),
				event.transition_id,
				event.operation_id.as_deref().unwrap_or("none"),
			);
		}
	}
	Ok(())
}

pub(crate) fn print_lane_authority_audit(request: LaneAuthorityReadbackRequest<'_>) -> Result<()> {
	let (_, report) = read_authority_reports(request)?;
	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		println!(
			"authority audit: chain_valid={} generation={} total_events={} lane_events={} project={} issue={} privacy={}",
			report.chain_valid,
			report.generation,
			report.total_event_count,
			report.lane_event_count,
			report.project_key,
			report.tracker_issue_id,
			report.privacy_projection,
		);
	}
	Ok(())
}

fn read_authority_reports(
	request: LaneAuthorityReadbackRequest<'_>,
) -> Result<(AuthorityTimelineReport, AuthorityAuditReport)> {
	let state_store = runtime::open_runtime_store()?;
	let config = super::project::load_lane_control_project(request.config_path, &state_store)?;
	build_authority_reports(&state_store, &config, request.issue)
}

fn build_authority_reports(
	state_store: &crate::state::StateStore,
	config: &crate::config::ServiceConfig,
	issue: &str,
) -> Result<(AuthorityTimelineReport, AuthorityAuditReport)> {
	let tracker_issue_id = resolve_tracker_issue_id(state_store, config, issue)?;
	let events = state_store.verify_authority_events()?;
	let entries = events
		.iter()
		.filter(|event| {
			event.draft.project_key.as_deref() == Some(config.service_id())
				&& event.draft.tracker_issue_id.as_deref() == Some(tracker_issue_id.as_str())
		})
		.map(|event| AuthorityTimelineEntry {
			generation: event.generation,
			sequence: event.sequence,
			event_id: event.draft.event_id.clone(),
			event_type: event.draft.event_type.as_str().to_owned(),
			transition_id: event.draft.transition_id.clone(),
			correlation_id: event.draft.correlation_id.clone(),
			causation_id: event.draft.causation_id.clone(),
			project_key: config.service_id().to_owned(),
			tracker_issue_id: tracker_issue_id.clone(),
			binding_fingerprint: event
				.draft
				.project_binding_fingerprint
				.clone()
				.unwrap_or_default(),
			invocation_fingerprint: event.draft.invocation_identity_fingerprint.clone(),
			facts_fingerprint: event.draft.observed_facts_fingerprint.clone(),
			decision: event.draft.decision.as_str().to_owned(),
			reason_codes: event
				.draft
				.reason_codes
				.iter()
				.map(|reason| reason.as_str().to_owned())
				.collect(),
			operation_id: event.draft.operation_id.clone(),
			runtime_version: event.draft.runtime_version.clone(),
			recorded_at_unix_micros: event.draft.recorded_at_unix_micros,
		})
		.collect::<Vec<_>>();
	let generation = events.first().map_or(0, |event| event.generation);
	let timeline = AuthorityTimelineReport {
		schema: "decodex/authority-timeline/1",
		project_key: config.service_id().to_owned(),
		tracker_issue_id: tracker_issue_id.clone(),
		event_count: entries.len(),
		events: entries,
	};
	let audit = AuthorityAuditReport {
		schema: "decodex/authority-audit/1",
		chain_valid: true,
		generation,
		total_event_count: events.len(),
		first_sequence: events.first().map(|event| event.sequence),
		last_sequence: events.last().map(|event| event.sequence),
		lane_event_count: timeline.event_count,
		project_key: config.service_id().to_owned(),
		tracker_issue_id,
		privacy_projection: "typed_allowlist_v1",
	};
	Ok((timeline, audit))
}

fn resolve_tracker_issue_id(
	state_store: &crate::state::StateStore,
	config: &crate::config::ServiceConfig,
	issue: &str,
) -> Result<String> {
	let lane_id = crate::lane_authority::LaneId::new(config.service_id(), issue)?;
	if state_store.lane(&lane_id)?.is_some() {
		return Ok(issue.to_owned());
	}
	let report = super::build_lane_inspect_report(state_store, config, issue, None)?;
	report
		.runs
		.first()
		.map(|run| run.issue_id.clone())
		.ok_or_else(|| eyre::eyre!("No local Lane authority matched issue `{issue}`."))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		lane_authority::{LaneCommand, LaneId},
		orchestrator::tests,
		state::StateStore,
	};

	#[test]
	fn lane_authority_v2_c5_authority_operator_readback_projection_privacy() {
		let (temp, config, _workflow) = tests::temp_project_layout();
		let database = temp.path().join("authority.sqlite3");
		let store = StateStore::open(&database).expect("prepare store");
		store.initialize_authority_generation(1, &[4_u8; 32]).expect("generation");
		drop(store);
		let store = StateStore::open_with_invocation(
			&database,
			crate::authority_broker::test_invocation_identity(),
		)
		.expect("store");
		let id = LaneId::new(config.service_id(), "issue-1").expect("lane");
		store
			.apply_lane_command(
				id,
				"binding-fingerprint",
				LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
			)
			.expect("admit");
		let (timeline, audit) =
			build_authority_reports(&store, &config, "issue-1").expect("reports");
		assert!(audit.chain_valid);
		assert_eq!(audit.total_event_count, 1);
		assert_eq!(timeline.event_count, 1);
		let payload = serde_json::to_string(&timeline).expect("json");
		for forbidden in ["worktree_path", "receipt_ref", "provider_body", "issue_body"] {
			assert!(!payload.contains(forbidden), "leaked {forbidden}");
		}
		assert!(payload.contains("invocation_fingerprint"));
		assert!(payload.contains("facts_fingerprint"));
	}
}
