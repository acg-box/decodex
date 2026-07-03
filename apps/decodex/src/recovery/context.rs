//! Recovery runtime context loading and tracker-backoff helpers.

use std::{
	env,
	path::{Path, PathBuf},
};

use color_eyre::Report;
use time::OffsetDateTime;

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	runtime,
	state::{ConnectorBackoffInput, StateStore},
	tracker::linear::LinearClient,
	workflow::WorkflowDocument,
};

pub(super) const LINEAR_RATE_LIMIT_BACKOFF_WARNING: &str = "tracker_rate_limited";

const LINEAR_RATE_LIMIT_BACKOFF_SECS: i64 = 15 * 60;
const LINEAR_TRANSIENT_TIMEOUT_BACKOFF_WARNING: &str = "tracker_transient_timeout";
const LINEAR_TRANSIENT_TIMEOUT_BACKOFF_SECS: i64 = 60;

pub(super) struct RecoveryContext {
	pub(super) config: ServiceConfig,
	pub(super) workflow: WorkflowDocument,
	pub(super) state_store: StateStore,
	pub(super) tracker: LinearClient,
	pub(super) runtime_mutation_policy: RecoveryRuntimeMutationPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RecoveryRuntimeMutationPolicy {
	AllowRuntimeWrites,
	ReadOnly,
}
impl RecoveryRuntimeMutationPolicy {
	pub(super) const fn allows_runtime_writes(self) -> bool {
		matches!(self, Self::AllowRuntimeWrites)
	}
}

pub(super) fn load_recovery_context_read_only(
	config_path: Option<&Path>,
) -> Result<RecoveryContext> {
	load_recovery_context_with_policy(config_path, RecoveryRuntimeMutationPolicy::ReadOnly)
}

pub(super) fn load_recovery_context_for_dry_run(
	config_path: Option<&Path>,
	dry_run: bool,
) -> Result<RecoveryContext> {
	let runtime_mutation_policy = if dry_run {
		RecoveryRuntimeMutationPolicy::ReadOnly
	} else {
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites
	};

	load_recovery_context_with_policy(config_path, runtime_mutation_policy)
}

pub(super) fn active_recovery_tracker_backoff_message(
	context: &RecoveryContext,
) -> Result<Option<String>> {
	let Some(backoff) =
		context.state_store.connector_backoff(context.config.service_id(), "linear")?
	else {
		return Ok(None);
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	if backoff.reset_unix_epoch() <= now_unix_epoch {
		if context.runtime_mutation_policy.allows_runtime_writes() {
			context.state_store.clear_connector_backoff(context.config.service_id(), "linear")?;
		}

		return Ok(None);
	}

	Ok(Some(recovery_tracker_backoff_message(
		context.config.service_id(),
		backoff.sync_phase(),
		backoff.reset_unix_epoch(),
		backoff.reset_unix_epoch().saturating_sub(now_unix_epoch),
	)))
}

pub(super) fn remember_recovery_tracker_backoff_message(
	context: &RecoveryContext,
	error: &Report,
	sync_phase: &str,
) -> Option<String> {
	let message = format!("{error:#}");
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let (quota_class, reset_unix_epoch, reset_source, warning) = if message
		.contains("Linear connector is rate limited")
	{
		let (reset_unix_epoch, reset_source) =
			match parse_recovery_rate_limit_reset_unix_epoch(&message) {
				Some(reset) if reset > now_unix_epoch => (reset, "linear"),
				_ =>
					(now_unix_epoch.saturating_add(LINEAR_RATE_LIMIT_BACKOFF_SECS), "local_default"),
			};

		(
			"linear_graphql_rate_limit",
			reset_unix_epoch,
			reset_source,
			LINEAR_RATE_LIMIT_BACKOFF_WARNING,
		)
	} else if message.contains("Linear connector timed out") {
		(
			"linear_graphql_timeout",
			now_unix_epoch.saturating_add(LINEAR_TRANSIENT_TIMEOUT_BACKOFF_SECS),
			"local_transient_timeout",
			LINEAR_TRANSIENT_TIMEOUT_BACKOFF_WARNING,
		)
	} else {
		return None;
	};

	if !context.runtime_mutation_policy.allows_runtime_writes() {
		return Some(recovery_tracker_backoff_message(
			context.config.service_id(),
			sync_phase,
			reset_unix_epoch,
			reset_unix_epoch.saturating_sub(now_unix_epoch),
		));
	}

	if let Err(store_error) = context.state_store.upsert_connector_backoff(ConnectorBackoffInput {
		project_id: context.config.service_id(),
		connector: "linear",
		sync_phase,
		quota_class,
		reset_unix_epoch,
		reset_source,
		warning,
	}) {
		let _ = store_error;

		tracing::warn!(
			project_id = context.config.service_id(),
			"Failed to persist recovery tracker backoff; sensitive runtime details were withheld."
		);
	}

	Some(recovery_tracker_backoff_message(
		context.config.service_id(),
		sync_phase,
		reset_unix_epoch,
		reset_unix_epoch.saturating_sub(now_unix_epoch),
	))
}

fn load_recovery_context_with_policy(
	config_path: Option<&Path>,
	runtime_mutation_policy: RecoveryRuntimeMutationPolicy,
) -> Result<RecoveryContext> {
	let state_store = runtime::open_runtime_store()?;
	let config_path = resolve_recovery_config_path(config_path, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	if runtime_mutation_policy.allows_runtime_writes() {
		runtime::register_project_config(&state_store, &config_path, true)?;
	}

	state_store.observe_dispatch_slot_root(config.service_id(), config.worktree_root())?;

	Ok(RecoveryContext { config, workflow, state_store, tracker, runtime_mutation_policy })
}

fn parse_recovery_rate_limit_reset_unix_epoch(message: &str) -> Option<i64> {
	let reset = message.split("rate limited until `").nth(1)?.split('`').next()?;

	reset.parse().ok()
}

fn recovery_tracker_backoff_message(
	service_id: &str,
	sync_phase: &str,
	reset_unix_epoch: i64,
	retry_after_seconds: i64,
) -> String {
	format!(
		"Linear connector is in backoff for project `{service_id}`; recovery skipped tracker reads for `{sync_phase}` until unix_epoch={reset_unix_epoch} (retry_after_seconds={retry_after_seconds})."
	)
}

fn resolve_recovery_config_path(
	config_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<PathBuf> {
	if let Some(config_path) = config_path {
		return ServiceConfig::resolve_project_config_path(config_path);
	}

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)?.ok_or_else(|| {
		eyre::eyre!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		)
	})
}
