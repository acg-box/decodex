//! Bounded Decodex daemon clients plus local commit and landing authority.

use std::{
	fmt::{Debug, Formatter},
	path::PathBuf,
};

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use tokio as _;

use decodex_protocol::{
	AppServerCapability, ClientFailure, ClientProfile, DoctorClient, DoctorComponent, DoctorIssue,
	DoctorReport, DoctorStatus, ProfileKind, ServerId,
};

mod account;
mod fast_mode;
mod git_hook;
mod local_git;
mod reset_card;

const OUTPUT_SCHEMA: &str = "decodex/cli-diagnostics/1";
const LOCAL_OUTPUT_SCHEMA: &str = "decodex/local-git/1";

/// Decodex daemon and local Git command client.
#[derive(Parser)]
#[command(name = "decodex", version, about)]
pub struct Cli {
	/// Select a declared profile instead of the configured active profile.
	#[arg(long, global = true, value_name = "NAME")]
	profile: Option<String>,
	/// Read the typed configuration from this Decodex-owned root.
	#[arg(long, global = true, value_name = "PATH")]
	root: Option<PathBuf>,
	/// Require this stable server identity for the selected daemon profile.
	#[arg(long, global = true, value_name = "UUID")]
	expected_server_id: Option<String>,
	/// Select human-readable or stable structured output.
	#[arg(long, global = true, value_enum, default_value_t = OutputFormat::Human)]
	output: OutputFormat,
	/// Selected daemon or local Git operation.
	#[command(subcommand)]
	command: Command,
}
impl Debug for Cli {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("Cli")
			.field("profile_selected", &self.profile.is_some())
			.field("root_selected", &self.root.is_some())
			.field("server_identity_selected", &self.expected_server_id.is_some())
			.field("output", &self.output)
			.field("command", &self.command)
			.finish()
	}
}

/// Fully rendered bounded command result.
pub struct CommandOutput {
	text: String,
	exit_code: u8,
	error_stream: bool,
}
impl CommandOutput {
	/// Rendered output without a trailing newline.
	pub fn text(&self) -> &str {
		&self.text
	}

	/// Stable process exit code defined by the selected command contract.
	pub const fn exit_code(&self) -> u8 {
		self.exit_code
	}

	/// Human failures use stderr; JSON documents always use stdout.
	pub const fn is_error_stream(&self) -> bool {
		self.error_stream
	}
}

#[derive(Serialize)]
struct ProfileDocument {
	kind: ProfileKind,
}

#[derive(Serialize)]
struct ReportDocument<'a> {
	schema: &'static str,
	command: DiagnosticCommand,
	outcome: &'static str,
	profile: ProfileDocument,
	status: OverallStatus,
	ready: usize,
	unavailable: usize,
	unknown: usize,
	report: &'a DoctorReport,
}

#[derive(Serialize)]
struct FailureDocument {
	schema: &'static str,
	command: DiagnosticCommand,
	outcome: &'static str,
	failure: ClientFailure,
}

/// Supported daemon and local Git operations.
#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum Command {
	/// Summarize daemon readiness while retaining every typed check.
	Status,
	/// Render the complete authoritative diagnostic report.
	Doctor,
	/// Observe and consume reset cards through the common daemon authority.
	#[command(subcommand)]
	ResetCard(reset_card::ResetCardCommand),
	/// Manage daemon-owned accounts through the same-UID V2.1 protocol.
	#[command(subcommand)]
	Account(account::AccountCommand),
	/// Read or update the current user's local Codex Fast mode setting.
	#[command(subcommand)]
	FastMode(fast_mode::FastModeCommand),
	/// Enforce the local Git commit and push policy without contacting the Decodex server.
	GitHook(git_hook::GitHookCommand),
	/// Create one signed local commit without contacting the Decodex server.
	Commit(local_git::CommitCommand),
	/// Land one reviewed pull request without contacting the Decodex server.
	Land(local_git::LandCommand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticCommand {
	Status,
	Doctor,
}

/// CLI output encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
	/// Stable operator-readable text.
	Human,
	/// Versioned JSON for structured consumers.
	Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum OverallStatus {
	Ready,
	Unavailable,
	Unknown,
}

/// Execute one daemon-client or local Git command.
pub async fn execute(cli: Cli) -> CommandOutput {
	let Cli { profile, root, expected_server_id, output, command } = cli;

	let command = match command {
		Command::Account(command) => {
			return account::execute(
				command,
				output,
				root.as_deref(),
				profile.as_deref(),
				expected_server_id.as_deref(),
			)
			.await;
		},
		Command::ResetCard(command) => {
			return reset_card::execute(
				command,
				output,
				root.as_deref(),
				profile.as_deref(),
				expected_server_id.as_deref(),
			)
			.await;
		},
		Command::FastMode(command) => {
			return fast_mode::execute(command, output);
		},
		Command::GitHook(command) => {
			return render_git_hook_result(output, git_hook::execute(&command));
		},
		Command::Commit(command) => {
			return render_local_result("commit", output, local_git::execute_commit(&command));
		},
		Command::Land(command) => {
			return render_local_result("land", output, local_git::execute_land(&command));
		},
		Command::Status => DiagnosticCommand::Status,
		Command::Doctor => DiagnosticCommand::Doctor,
	};
	let profile =
		load_client_profile(root.as_deref(), profile.as_deref(), expected_server_id.as_deref());
	let profile = match profile {
		Ok(profile) => profile,
		Err(failure) => return render_failure(command, output, failure),
	};
	let client = DoctorClient::new(profile);
	let report = match client.query().await {
		Ok(report) => report,
		Err(failure) => return render_failure(command, output, failure),
	};

	render_report(command, output, client.profile().kind(), &report)
}

fn load_client_profile(
	root: Option<&std::path::Path>,
	selected_profile: Option<&str>,
	expected_server_id: Option<&str>,
) -> Result<ClientProfile, ClientFailure> {
	let profile = match root {
		Some(root) => ClientProfile::load(root, selected_profile),
		None => ClientProfile::load_default(selected_profile),
	}?;
	let Some(expected_server_id) = expected_server_id else {
		return Ok(profile);
	};
	if !is_canonical_uuid(expected_server_id) {
		return Err(ClientFailure::ConfigurationMalformed);
	}
	let expected_server_id =
		ServerId::new(expected_server_id).map_err(|_| ClientFailure::ConfigurationMalformed)?;

	Ok(profile.with_expected_server_id(expected_server_id))
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
		})
}

fn render_report(
	command: DiagnosticCommand,
	format: OutputFormat,
	profile_kind: ProfileKind,
	report: &DoctorReport,
) -> CommandOutput {
	if !report.has_current_component_set() {
		return render_failure(command, format, ClientFailure::ProtocolMalformed);
	}

	let counts = counts(report);
	let overall = overall(report);
	let text = match format {
		OutputFormat::Json => serde_json::to_string(&ReportDocument {
			schema: OUTPUT_SCHEMA,
			command,
			outcome: "report",
			profile: ProfileDocument { kind: profile_kind },
			status: overall,
			ready: counts.0,
			unavailable: counts.1,
			unknown: counts.2,
			report,
		})
		.expect("bounded typed diagnostic serialization cannot fail"),
		OutputFormat::Human => render_human(command, profile_kind, report, overall, counts),
	};

	CommandOutput {
		text,
		exit_code: u8::from(overall != OverallStatus::Ready),
		error_stream: false,
	}
}

fn render_git_hook_result(format: OutputFormat, result: Result<(), String>) -> CommandOutput {
	match (format, result) {
		(OutputFormat::Human, Ok(())) =>
			CommandOutput { text: String::new(), exit_code: 0, error_stream: false },
		(OutputFormat::Human, Err(error)) => CommandOutput {
			text: format!("decodex git-hook failed: {error}"),
			exit_code: 2,
			error_stream: true,
		},
		(OutputFormat::Json, Ok(())) => CommandOutput {
			text: serde_json::to_string(&serde_json::json!({
				"schema": LOCAL_OUTPUT_SCHEMA,
				"command": "git_hook",
				"outcome": "success",
			}))
			.expect("bounded local hook result serialization cannot fail"),
			exit_code: 0,
			error_stream: false,
		},
		(OutputFormat::Json, Err(error)) => CommandOutput {
			text: serde_json::to_string(&serde_json::json!({
				"schema": LOCAL_OUTPUT_SCHEMA,
				"command": "git_hook",
				"outcome": "failure",
				"error": error,
			}))
			.expect("bounded local hook failure serialization cannot fail"),
			exit_code: 2,
			error_stream: false,
		},
	}
}

fn render_local_result(
	command: &'static str,
	format: OutputFormat,
	result: Result<String, String>,
) -> CommandOutput {
	match (format, result) {
		(OutputFormat::Human, Ok(text)) =>
			CommandOutput { text, exit_code: 0, error_stream: false },
		(OutputFormat::Human, Err(error)) => CommandOutput {
			text: format!("decodex {command} failed: {error}"),
			exit_code: 2,
			error_stream: true,
		},
		(OutputFormat::Json, Ok(result)) => CommandOutput {
			text: serde_json::to_string(&serde_json::json!({
				"schema": LOCAL_OUTPUT_SCHEMA,
				"command": command,
				"outcome": "success",
				"result": result,
			}))
			.expect("bounded local result serialization cannot fail"),
			exit_code: 0,
			error_stream: false,
		},
		(OutputFormat::Json, Err(error)) => CommandOutput {
			text: serde_json::to_string(&serde_json::json!({
				"schema": LOCAL_OUTPUT_SCHEMA,
				"command": command,
				"outcome": "failure",
				"error": error,
			}))
			.expect("bounded local failure serialization cannot fail"),
			exit_code: 2,
			error_stream: false,
		},
	}
}

fn render_failure(
	command: DiagnosticCommand,
	format: OutputFormat,
	failure: ClientFailure,
) -> CommandOutput {
	let (text, error_stream) = match format {
		OutputFormat::Json => (
			serde_json::to_string(&FailureDocument {
				schema: OUTPUT_SCHEMA,
				command,
				outcome: "failure",
				failure,
			})
			.expect("closed failure serialization cannot fail"),
			false,
		),
		OutputFormat::Human =>
			(format!("decodex {} failed: {failure}", command_name(command)), true),
	};

	CommandOutput { text, exit_code: 2, error_stream }
}

fn render_human(
	command: DiagnosticCommand,
	profile_kind: ProfileKind,
	report: &DoctorReport,
	overall: OverallStatus,
	counts: (usize, usize, usize),
) -> String {
	let mut output = format!(
		"decodex {}: {}\nprofile: {}\nserver: {}\nprotocol: {}.{}\nchecks: {} ready, {} unavailable, {} unknown",
		command_name(command),
		overall_name(overall),
		profile_kind_name(profile_kind),
		report.server_id().as_str(),
		report.version().major,
		report.version().minor,
		counts.0,
		counts.1,
		counts.2,
	);

	match command {
		DiagnosticCommand::Doctor =>
			for check in report.checks() {
				output.push_str(&format!(
					"\n{}: {}",
					component_name(check.component),
					status_name(check.status),
				));
			},
		DiagnosticCommand::Status => {
			output.push_str("\nstates:");

			for check in report.checks() {
				output.push_str(&format!(
					" {}={};",
					component_name(check.component),
					status_name(check.status),
				));
			}
		},
	}

	output
}

fn overall(report: &DoctorReport) -> OverallStatus {
	if report.checks().iter().any(|check| matches!(check.status, DoctorStatus::Unavailable(_))) {
		OverallStatus::Unavailable
	} else if report.checks().iter().any(|check| matches!(check.status, DoctorStatus::Unknown(_))) {
		OverallStatus::Unknown
	} else {
		OverallStatus::Ready
	}
}

fn counts(report: &DoctorReport) -> (usize, usize, usize) {
	report.checks().iter().fold((0, 0, 0), |mut counts, check| {
		match check.status {
			DoctorStatus::Ready => counts.0 += 1,
			DoctorStatus::Unavailable(_) => counts.1 += 1,
			DoctorStatus::Unknown(_) => counts.2 += 1,
		}

		counts
	})
}

const fn command_name(command: DiagnosticCommand) -> &'static str {
	match command {
		DiagnosticCommand::Status => "status",
		DiagnosticCommand::Doctor => "doctor",
	}
}

const fn overall_name(status: OverallStatus) -> &'static str {
	match status {
		OverallStatus::Ready => "ready",
		OverallStatus::Unavailable => "unavailable",
		OverallStatus::Unknown => "unknown",
	}
}

const fn profile_kind_name(kind: ProfileKind) -> &'static str {
	match kind {
		ProfileKind::Local => "local",
		ProfileKind::Remote => "remote",
	}
}

fn component_name(component: DoctorComponent) -> &'static str {
	match component {
		DoctorComponent::Configuration => "configuration",
		DoctorComponent::ProductStore => "product_store",
		DoctorComponent::QuickTask => "quick_task",
		DoctorComponent::Protocol => "protocol",
		DoctorComponent::ProtocolVersion => "protocol_version",
		DoctorComponent::ServerIdentity => "server_identity",
		DoctorComponent::SharedCodexHome => "shared_codex_home",
		DoctorComponent::AppServerCapability(capability) => capability_name(capability),
		DoctorComponent::ManagedRepository => "managed_repository",
		DoctorComponent::BlobIntegrity => "blob_integrity",
		DoctorComponent::CredentialVault => "credential_vault",
		DoctorComponent::PluginReadiness => "plugin_readiness",
	}
}

const fn capability_name(capability: AppServerCapability) -> &'static str {
	match capability {
		AppServerCapability::Initialize => "app_server.initialize",
		AppServerCapability::AccountRead => "app_server.account_read",
		AppServerCapability::ThreadList => "app_server.thread_list",
		AppServerCapability::ThreadRead => "app_server.thread_read",
		AppServerCapability::ThreadArchive => "app_server.thread_archive",
		AppServerCapability::PaginatedHistory => "app_server.paginated_history",
		AppServerCapability::NativeCollaboration => "app_server.native_collaboration",
		AppServerCapability::ThreadSearch => "app_server.thread_search",
	}
}

fn status_name(status: DoctorStatus) -> String {
	match status {
		DoctorStatus::Ready => "ready".into(),
		DoctorStatus::Unavailable(issue) => format!("unavailable({})", issue_name(issue)),
		DoctorStatus::Unknown(issue) => format!("unknown({})", issue_name(issue)),
	}
}

const fn issue_name(issue: DoctorIssue) -> &'static str {
	match issue {
		DoctorIssue::Authentication => "authentication",
		DoctorIssue::Plugin => "plugin",
		DoctorIssue::ConfigurationMissing => "configuration_missing",
		DoctorIssue::ConfigurationMalformed => "configuration_malformed",
		DoctorIssue::ConfigurationVersion => "configuration_version",
		DoctorIssue::DatabaseNotConfigured => "database_not_configured",
		DoctorIssue::DatabaseMalformedConfig => "database_malformed_config",
		DoctorIssue::DatabaseUnreachable => "database_unreachable",
		DoctorIssue::DatabaseIncompatible => "database_incompatible",
		DoctorIssue::UnsafeDatabaseAuthority => "unsafe_database_authority",
		DoctorIssue::ProtocolDisconnected => "protocol_disconnected",
		DoctorIssue::ProtocolVersionMismatch => "protocol_version_mismatch",
		DoctorIssue::ServerIdentityMismatch => "server_identity_mismatch",
		DoctorIssue::ServerIdentityUnavailable => "server_identity_unavailable",
		DoctorIssue::UnsafeHostPath => "unsafe_host_path",
		DoctorIssue::Integrity => "integrity",
		DoctorIssue::NotProbed => "not_probed",
		DoctorIssue::Disabled => "disabled",
	}
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeSet;

	use clap::{CommandFactory as _, Parser as _};

	use crate::{Cli, Command, DiagnosticCommand, OutputFormat};
	use decodex_protocol::{
		CURRENT_VERSION, ClientFailure, DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport,
		DoctorStatus, ProfileKind, ServerId,
	};

	const SERVER_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

	fn report(statuses: impl IntoIterator<Item = DoctorStatus>) -> DoctorReport {
		DoctorReport::new(
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
			CURRENT_VERSION,
			DoctorComponent::ALL
				.into_iter()
				.zip(statuses)
				.map(|(component, status)| DoctorCheck::new(component, status))
				.collect(),
		)
		.expect("test operation must succeed")
	}

	#[test]
	fn command_surface_is_multi_command_without_aliases() {
		Cli::command().debug_assert();

		let doctor = Cli::try_parse_from(["decodex", "--profile", "remote", "doctor"])
			.expect("test operation must succeed");
		let status = Cli::try_parse_from(["decodex", "status", "--output", "json"])
			.expect("test operation must succeed");
		let commit =
			Cli::try_parse_from(["decodex", "commit", "Exact candidate", "--manual-authority"])
				.expect("test operation must succeed");
		let land = Cli::try_parse_from([
			"decodex",
			"land",
			"Exact candidate",
			"--manual-authority",
			"--pr",
			"https://github.com/acg-box/decodex/pull/123",
			"--expected-base-oid",
			"1111111111111111111111111111111111111111",
			"--expected-head-oid",
			"2222222222222222222222222222222222222222",
		])
		.expect("test operation must succeed");
		let git_hook =
			Cli::try_parse_from(["decodex", "git-hook", "commit-msg", ".git/COMMIT_EDITMSG"])
				.expect("test operation must succeed");

		assert_eq!(doctor.command, Command::Doctor);
		assert_eq!(doctor.profile.as_deref(), Some("remote"));
		assert_eq!(status.command, Command::Status);
		assert_eq!(status.output, OutputFormat::Json);
		assert!(matches!(
			commit.command,
			Command::Commit(command)
				if command.manual_authority && command.summary == "Exact candidate"
		));
		assert!(matches!(
			land.command,
			Command::Land(command)
				if command.manual_authority
					&& command.pr == "https://github.com/acg-box/decodex/pull/123"
					&& command.expected_base_oid
						== "1111111111111111111111111111111111111111"
					&& command.expected_head_oid
						== "2222222222222222222222222222222222222222"
		));
		assert!(matches!(git_hook.command, Command::GitHook(_)));
		assert!(Cli::try_parse_from(["decodex", "diagnose"]).is_err());
	}

	#[test]
	fn cli_debug_never_discloses_profile_or_root_text() {
		let profile_marker = "xy1308-profile-secret-marker";
		let root_marker = "/tmp/xy1308-root-secret-marker";
		let cli = Cli::try_parse_from([
			"decodex",
			"--profile",
			profile_marker,
			"--root",
			root_marker,
			"--expected-server-id",
			"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162",
			"doctor",
		])
		.expect("test operation must succeed");
		let debug = format!("{cli:?}");

		assert!(!debug.contains(profile_marker));
		assert!(!debug.contains(root_marker));
		assert!(debug.contains("profile_selected: true"));
		assert!(debug.contains("root_selected: true"));
		assert!(debug.contains("server_identity_selected: true"));
	}

	#[test]
	fn every_component_and_issue_has_one_stable_human_name() {
		let components = DoctorComponent::ALL.map(crate::component_name);
		let issues = DoctorIssue::ALL.map(crate::issue_name);

		assert_eq!(components.into_iter().collect::<BTreeSet<_>>().len(), components.len());
		assert_eq!(issues.into_iter().collect::<BTreeSet<_>>().len(), issues.len());
	}

	#[test]
	fn doctor_human_and_json_preserve_every_component_status_and_issue() {
		let statuses = DoctorIssue::ALL
			.into_iter()
			.chain([DoctorIssue::NotProbed])
			.map(DoctorStatus::Unavailable);
		let report = report(statuses);
		let human = crate::render_report(
			DiagnosticCommand::Doctor,
			OutputFormat::Human,
			ProfileKind::Remote,
			&report,
		);
		let json = crate::render_report(
			DiagnosticCommand::Doctor,
			OutputFormat::Json,
			ProfileKind::Remote,
			&report,
		);

		for component in DoctorComponent::ALL {
			assert!(human.text().contains(crate::component_name(component)));
		}
		for issue in DoctorIssue::ALL {
			assert!(human.text().contains(crate::issue_name(issue)));
		}

		let value: serde_json::Value =
			serde_json::from_str(json.text()).expect("test operation must succeed");
		let decoded: DoctorReport =
			serde_json::from_value(value["report"].clone()).expect("test operation must succeed");

		assert_eq!(decoded, report);
		assert_eq!(human.exit_code(), 1);
		assert_eq!(json.exit_code(), 1);
	}

	#[test]
	fn status_retains_ready_unavailable_and_unknown_states() {
		let statuses = DoctorComponent::ALL.map(|component| match component {
			DoctorComponent::ProductStore =>
				DoctorStatus::Unavailable(DoctorIssue::DatabaseUnreachable),
			DoctorComponent::QuickTask => DoctorStatus::Unknown(DoctorIssue::NotProbed),
			_ => DoctorStatus::Ready,
		});

		let report = report(statuses);
		let output = crate::render_report(
			DiagnosticCommand::Status,
			OutputFormat::Human,
			ProfileKind::Local,
			&report,
		);

		assert!(output.text().contains("17 ready, 1 unavailable, 1 unknown"));
		assert!(output.text().contains("product_store=unavailable(database_unreachable)"));
		assert!(output.text().contains("quick_task=unknown(not_probed)"));
	}

	#[test]
	fn every_client_failure_has_bounded_human_and_structured_output() {
		let failures = [
			ClientFailure::ConfigurationMissing,
			ClientFailure::ConfigurationMalformed,
			ClientFailure::ConfigurationVersion,
			ClientFailure::ProfileMissing,
			ClientFailure::UnsafeHostPath,
			ClientFailure::ServerIdentityUnavailable,
			ClientFailure::RemoteMutationUnsupported,
			ClientFailure::LocalTransportDisabled,
			ClientFailure::RemoteTransportDisabled,
			ClientFailure::LocalTransportUnsupported,
			ClientFailure::UnsafeLocalEndpoint,
			ClientFailure::LocalPeerIdentityUnavailable,
			ClientFailure::LocalPeerUidMismatch,
			ClientFailure::ProtocolDisconnected,
			ClientFailure::ProtocolTimeout,
			ClientFailure::ProtocolMajorMismatch,
			ClientFailure::ProtocolMinorMismatch,
			ClientFailure::ServerIdentityMismatch,
			ClientFailure::ProtocolMalformed,
			ClientFailure::ProtocolViolation,
			ClientFailure::ProtocolBackpressure,
			ClientFailure::ApplicationAcceptanceUnknown,
		];

		for failure in failures {
			let human =
				crate::render_failure(DiagnosticCommand::Doctor, OutputFormat::Human, failure);
			let json =
				crate::render_failure(DiagnosticCommand::Doctor, OutputFormat::Json, failure);
			let value: serde_json::Value =
				serde_json::from_str(json.text()).expect("test operation must succeed");

			assert_eq!(human.exit_code(), 2);
			assert!(human.is_error_stream());
			assert_eq!(json.exit_code(), 2);
			assert!(!json.is_error_stream());
			assert_eq!(value["schema"], "decodex/cli-diagnostics/1");
			assert_eq!(value["outcome"], "failure");
			assert!(value["failure"].is_string());
			assert!(human.text().len() < 256);
			assert!(json.text().len() < 256);
		}
	}

	#[test]
	fn all_ready_report_has_success_exit() {
		let report = report(vec![DoctorStatus::Ready; DoctorComponent::ALL.len()]);
		let output = crate::render_report(
			DiagnosticCommand::Status,
			OutputFormat::Json,
			ProfileKind::Local,
			&report,
		);

		assert_eq!(output.exit_code(), 0);
	}

	#[test]
	fn incomplete_ready_reports_render_as_closed_protocol_failures() {
		let complete = report(vec![DoctorStatus::Ready; DoctorComponent::ALL.len()]);
		let cases = [
			DoctorReport::new(
				ServerId::new(SERVER_ID).expect("test operation must succeed"),
				CURRENT_VERSION,
				Vec::new(),
			)
			.expect("test operation must succeed"),
			DoctorReport::new(
				ServerId::new(SERVER_ID).expect("test operation must succeed"),
				CURRENT_VERSION,
				vec![DoctorCheck::new(DoctorComponent::Configuration, DoctorStatus::Ready)],
			)
			.expect("test operation must succeed"),
			DoctorReport::new(
				ServerId::new(SERVER_ID).expect("test operation must succeed"),
				CURRENT_VERSION,
				complete.checks()[1..].to_vec(),
			)
			.expect("test operation must succeed"),
		];

		for report in cases {
			for format in [OutputFormat::Human, OutputFormat::Json] {
				let output = crate::render_report(
					DiagnosticCommand::Status,
					format,
					ProfileKind::Local,
					&report,
				);

				assert_eq!(output.exit_code(), 2);

				if format == OutputFormat::Json {
					let value: serde_json::Value =
						serde_json::from_str(output.text()).expect("test operation must succeed");

					assert_eq!(value["failure"], "protocol_malformed");
				}
			}
		}
	}

	#[test]
	fn complete_ready_report_in_arbitrary_order_stays_successful() {
		let complete = report(vec![DoctorStatus::Ready; DoctorComponent::ALL.len()]);
		let mut checks = complete.checks().to_vec();

		checks.reverse();

		let report = DoctorReport::new(
			ServerId::new(SERVER_ID).expect("test operation must succeed"),
			CURRENT_VERSION,
			checks,
		)
		.expect("test operation must succeed");
		let output = crate::render_report(
			DiagnosticCommand::Status,
			OutputFormat::Json,
			ProfileKind::Local,
			&report,
		);

		assert_eq!(output.exit_code(), 0);
	}
}
