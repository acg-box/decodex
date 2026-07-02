use std::{
	collections::{self, BTreeMap},
	fs::{self, OpenOptions},
	io::Write as _,
	path::{Path, PathBuf},
	process,
};

use serde::Serialize;
use time::OffsetDateTime;

use crate::orchestrator::agent_evidence::{
	AGENT_BLOCKER_SNAPSHOT_SCHEMA, AGENT_EVIDENCE_EVENT_SCHEMA, AgentBlocker, AgentBlockerSnapshot,
	AgentEvidenceEvent, AgentEvidenceFileWriteContext, AgentEvidenceSource, AgentHandoffIndex,
	AgentRunCapsule, AgentRunCapsuleRef,
};
use crate::prelude::{Result, eyre};

pub(in crate::orchestrator) fn write_agent_evidence_files(
	context: &AgentEvidenceFileWriteContext<'_>,
	index: &AgentHandoffIndex,
	run_capsules: &[AgentRunCapsule],
) -> Result<()> {
	for capsule in run_capsules {
		let path = PathBuf::from(&capsule.path);

		write_json_atomically(&path, capsule)?;
	}

	write_blocker_snapshots(
		context.project_id,
		context.generated_at,
		context.blockers_dir,
		&index.blockers,
		&index.run_capsules,
	)?;
	write_json_atomically(context.handoff_index_path, index)?;
	append_agent_evidence_event(
		context.project_id,
		context.generated_at,
		context.source,
		context.events_path,
		index,
	)?;

	Ok(())
}

fn write_blocker_snapshots(
	project_id: &str,
	generated_at: &str,
	blockers_dir: &Path,
	blockers: &[AgentBlocker],
	run_refs: &[AgentRunCapsuleRef],
) -> Result<()> {
	fs::create_dir_all(blockers_dir)?;

	let mut blockers_by_path: BTreeMap<String, Vec<AgentBlocker>> = BTreeMap::new();

	for blocker in blockers {
		blockers_by_path
			.entry(blocker.blocker_snapshot_path.clone())
			.or_default()
			.push(blocker.clone());
	}

	let mut kept_paths = collections::BTreeSet::new();

	for (path, blockers) in blockers_by_path {
		let path = PathBuf::from(path);
		let related_run_capsules = blockers
			.iter()
			.filter_map(|blocker| blocker.run_id.as_deref())
			.filter_map(|run_id| run_refs.iter().find(|run_ref| run_ref.run_id == run_id))
			.cloned()
			.collect::<Vec<_>>();
		let snapshot = AgentBlockerSnapshot {
			schema: AGENT_BLOCKER_SNAPSHOT_SCHEMA,
			project_id: project_id.to_owned(),
			generated_at: generated_at.to_owned(),
			issue_id: blockers.iter().find_map(|blocker| blocker.issue_id.clone()),
			issue_identifier: blockers.iter().find_map(|blocker| blocker.issue_identifier.clone()),
			blockers,
			related_run_capsules,
		};

		write_json_atomically(&path, &snapshot)?;

		kept_paths.insert(path);
	}

	prune_stale_json_files(blockers_dir, &kept_paths)
}

fn append_agent_evidence_event(
	project_id: &str,
	generated_at: &str,
	source: AgentEvidenceSource,
	events_path: &Path,
	index: &AgentHandoffIndex,
) -> Result<()> {
	if let Some(parent) = events_path.parent() {
		fs::create_dir_all(parent)?;
	}

	let event = AgentEvidenceEvent {
		schema: AGENT_EVIDENCE_EVENT_SCHEMA,
		project_id: project_id.to_owned(),
		generated_at: generated_at.to_owned(),
		source: source.as_str().to_owned(),
		handoff_index_path: index.handoff_index_path.clone(),
		blocker_count: index.summary.blocker_count,
		run_capsule_count: index.summary.run_capsule_count,
		warning_count: index.summary.warning_count,
		connector_backoff_count: index.summary.connector_backoff_count,
	};
	let mut file = OpenOptions::new().create(true).append(true).open(events_path)?;

	writeln!(file, "{}", serde_json::to_string(&event)?)?;

	Ok(())
}

fn write_json_atomically<T>(path: &Path, value: &T) -> Result<()>
where
	T: Serialize,
{
	let Some(parent) = path.parent() else {
		eyre::bail!("Agent evidence path `{}` has no parent directory.", path.display());
	};

	fs::create_dir_all(parent)?;

	let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
		eyre::eyre!("Agent evidence path `{}` has no UTF-8 file name.", path.display())
	})?;
	let temp_path = parent.join(format!(
		".{file_name}.tmp-{}-{}",
		process::id(),
		OffsetDateTime::now_utc().unix_timestamp_nanos()
	));
	let body = serde_json::to_vec_pretty(value)?;

	fs::write(&temp_path, body)?;
	fs::rename(&temp_path, path)?;

	Ok(())
}

fn prune_stale_json_files(dir: &Path, keep_paths: &collections::BTreeSet<PathBuf>) -> Result<()> {
	for entry in fs::read_dir(dir)? {
		let entry = entry?;
		let path = entry.path();

		if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
			continue;
		}
		if !keep_paths.contains(&path) {
			fs::remove_file(path)?;
		}
	}

	Ok(())
}
