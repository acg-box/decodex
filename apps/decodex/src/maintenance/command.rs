use std::{
	io::{self, Write as _},
	time::SystemTime,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	maintenance::{
		files::{self},
		policy::{MaintenanceMode, MaintenancePolicy, MaintenancePruneRequest, MaintenanceScope},
		reports::MaintenanceReport,
		runtime::{self},
	},
	prelude::Result,
};

pub(crate) fn run_prune_command(request: MaintenancePruneRequest) -> Result<()> {
	let report = run_prune_with_policy(request, MaintenancePolicy::default())?;

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print_prune_report(&report)?;
	}

	Ok(())
}

pub(crate) fn run_auto_safe_prune() -> Result<MaintenanceReport> {
	run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy::default(),
	)
}

pub(crate) fn run_prune_with_policy(
	request: MaintenancePruneRequest,
	policy: MaintenancePolicy,
) -> Result<MaintenanceReport> {
	let generated_at = OffsetDateTime::now_utc();
	let system_now = SystemTime::now();
	let logs = files::maintain_logs(request.mode, policy, system_now, generated_at)?;
	let agent_evidence =
		files::maintain_agent_evidence(request.mode, policy, system_now, generated_at)?;
	let git_askpass_helpers = files::maintain_git_askpass_helpers_for_scope(
		request.mode,
		request.scope,
		policy,
		system_now,
	)?;
	let backups = files::maintain_backups(request.mode, policy, system_now)?;
	let runtime = runtime::maintain_runtime(request.mode, request.scope, policy, generated_at)?;
	let wal_checkpoint = runtime::maintain_wal(request.mode, request.scope)?;

	Ok(MaintenanceReport {
		schema: "decodex.maintenance_report/1",
		mode: request.mode.as_str().to_owned(),
		scope: match request.scope {
			MaintenanceScope::Full => String::from("full"),
			MaintenanceScope::AutoSafe => String::from("auto-safe"),
		},
		generated_at: generated_at.format(&Rfc3339)?,
		logs,
		agent_evidence,
		git_askpass_helpers,
		backups,
		runtime,
		wal_checkpoint,
	})
}

fn print_prune_report(report: &MaintenanceReport) -> Result<()> {
	let mut stdout = io::stdout().lock();

	writeln!(stdout, "Decodex maintenance prune ({}, {})", report.mode, report.scope)?;
	writeln!(
		stdout,
		"logs: rotate {}/{} files ({} bytes), delete {}/{} files ({} bytes)",
		report.logs.rotated_files,
		report.logs.rotate_candidates,
		report.logs.rotate_bytes,
		report.logs.deleted_files,
		report.logs.delete_candidates,
		report.logs.delete_bytes
	)?;
	writeln!(
		stdout,
		"agent-evidence: rotate {}/{} streams ({} bytes), delete {}/{} files ({} bytes)",
		report.agent_evidence.rotated_files,
		report.agent_evidence.rotate_candidates,
		report.agent_evidence.rotate_bytes,
		report.agent_evidence.deleted_files,
		report.agent_evidence.delete_candidates,
		report.agent_evidence.delete_bytes
	)?;
	writeln!(
		stdout,
		"git-askpass: delete {}/{} files ({} bytes)",
		report.git_askpass_helpers.deleted_files,
		report.git_askpass_helpers.delete_candidates,
		report.git_askpass_helpers.delete_bytes
	)?;
	writeln!(
		stdout,
		"backups: delete {}/{} files ({} bytes)",
		report.backups.deleted_files, report.backups.delete_candidates, report.backups.delete_bytes
	)?;
	writeln!(
		stdout,
		"runtime: compact {}/{} terminal runs ({} protocol events), protected runs {}",
		report.runtime.compacted_runs,
		report.runtime.protocol_run_candidates,
		report.runtime.protocol_event_candidates,
		report.runtime.protected_run_count
	)?;

	for warning in &report.runtime.warnings {
		writeln!(stdout, "runtime warning: {} ({})", warning.warning, warning.reason)?;
	}

	match &report.wal_checkpoint {
		Some(checkpoint) => writeln!(
			stdout,
			"wal: {} checkpoint busy={} log_frames={} checkpointed_frames={}",
			checkpoint.mode, checkpoint.busy, checkpoint.log_frames, checkpoint.checkpointed_frames
		)?,
		None => writeln!(stdout, "wal: skipped")?,
	}

	Ok(())
}
