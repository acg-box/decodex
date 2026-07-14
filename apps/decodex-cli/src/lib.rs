//! Multi-command rendering over the shared bounded Decodex protocol client.

use std::{
	fmt::{Debug, Formatter},
	path::PathBuf,
};

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use tokio as _;

use decodex_protocol::{
	AppServerCapability, ClientFailure, ClientProfile, DoctorClient, DoctorComponent, DoctorIssue,
	DoctorReport, DoctorStatus, ProfileKind,
};

const OUTPUT_SCHEMA: &str = "decodex/cli-diagnostics/1";

/// API-only Decodex diagnostics client.
#[derive(Parser)]
#[command(name = "decodex", version, about)]
pub struct Cli {
	/// Select a declared profile instead of the configured active profile.
	#[arg(long, global = true, value_name = "NAME")]
	profile: Option<String>,
	/// Read the typed configuration from this Decodex-owned root.
	#[arg(long, global = true, value_name = "PATH")]
	root: Option<PathBuf>,
	/// Select human-readable or stable structured output.
	#[arg(long, global = true, value_enum, default_value_t = OutputFormat::Human)]
	output: OutputFormat,
	/// Read-only daemon operation.
	#[command(subcommand)]
	command: Command,
}
impl Debug for Cli {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("Cli")
			.field("profile_selected", &self.profile.is_some())
			.field("root_selected", &self.root.is_some())
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

	/// Stable process exit code: 0 ready, 1 report has non-ready checks, 2 client failure.
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
	command: Command,
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
	command: Command,
	outcome: &'static str,
	failure: ClientFailure,
}

/// Supported read-only daemon operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Subcommand)]
#[serde(rename_all = "snake_case")]
pub enum Command {
	/// Summarize daemon readiness while retaining every typed check.
	Status,
	/// Render the complete authoritative diagnostic report.
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

/// Execute one API-only command.
pub async fn execute(cli: Cli) -> CommandOutput {
	let profile = match cli.root.as_deref() {
		Some(root) => ClientProfile::load(root, cli.profile.as_deref()),
		None => ClientProfile::load_default(cli.profile.as_deref()),
	};
	let profile = match profile {
		Ok(profile) => profile,
		Err(failure) => return render_failure(cli.command, cli.output, failure),
	};
	let client = DoctorClient::new(profile);
	let report = match client.query().await {
		Ok(report) => report,
		Err(failure) => return render_failure(cli.command, cli.output, failure),
	};

	render_report(cli.command, cli.output, client.profile().kind(), &report)
}

fn render_report(
	command: Command,
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

fn render_failure(command: Command, format: OutputFormat, failure: ClientFailure) -> CommandOutput {
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
	command: Command,
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
		Command::Doctor =>
			for check in report.checks() {
				output.push_str(&format!(
					"\n{}: {}",
					component_name(check.component),
					status_name(check.status),
				));
			},
		Command::Status => {
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

const fn command_name(command: Command) -> &'static str {
	match command {
		Command::Status => "status",
		Command::Doctor => "doctor",
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
		DoctorComponent::Database => "database",
		DoctorComponent::Protocol => "protocol",
		DoctorComponent::ProtocolVersion => "protocol_version",
		DoctorComponent::ServerIdentity => "server_identity",
		DoctorComponent::SharedCodexHome => "shared_codex_home",
		DoctorComponent::AppServerCapability(capability) => capability_name(capability),
		DoctorComponent::ServerRepositories => "server_repositories",
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

	use crate::{Cli, Command, OutputFormat};
	use decodex_protocol::{
		CURRENT_VERSION, ClientFailure, DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport,
		DoctorStatus, ProfileKind, ServerId,
	};

	const SERVER_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

	fn report(statuses: impl IntoIterator<Item = DoctorStatus>) -> DoctorReport {
		DoctorReport::new(
			ServerId::new(SERVER_ID).unwrap(),
			CURRENT_VERSION,
			DoctorComponent::ALL
				.into_iter()
				.zip(statuses)
				.map(|(component, status)| DoctorCheck::new(component, status))
				.collect(),
		)
		.unwrap()
	}

	#[test]
	fn command_surface_is_multi_command_without_aliases() {
		Cli::command().debug_assert();

		let doctor = Cli::try_parse_from(["decodex", "--profile", "remote", "doctor"]).unwrap();
		let status = Cli::try_parse_from(["decodex", "status", "--output", "json"]).unwrap();

		assert_eq!(doctor.command, Command::Doctor);
		assert_eq!(doctor.profile.as_deref(), Some("remote"));
		assert_eq!(status.command, Command::Status);
		assert_eq!(status.output, OutputFormat::Json);
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
			"doctor",
		])
		.unwrap();
		let debug = format!("{cli:?}");

		assert!(!debug.contains(profile_marker));
		assert!(!debug.contains(root_marker));
		assert!(debug.contains("profile_selected: true"));
		assert!(debug.contains("root_selected: true"));
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
		let statuses = DoctorIssue::ALL.map(DoctorStatus::Unavailable);
		let report = report(statuses);
		let human = crate::render_report(
			Command::Doctor,
			OutputFormat::Human,
			ProfileKind::Remote,
			&report,
		);
		let json =
			crate::render_report(Command::Doctor, OutputFormat::Json, ProfileKind::Remote, &report);

		for component in DoctorComponent::ALL {
			assert!(human.text().contains(crate::component_name(component)));
		}
		for issue in DoctorIssue::ALL {
			assert!(human.text().contains(crate::issue_name(issue)));
		}

		let value: serde_json::Value = serde_json::from_str(json.text()).unwrap();
		let decoded: DoctorReport = serde_json::from_value(value["report"].clone()).unwrap();

		assert_eq!(decoded, report);
		assert_eq!(human.exit_code(), 1);
		assert_eq!(json.exit_code(), 1);
	}

	#[test]
	fn status_retains_ready_unavailable_and_unknown_states() {
		let mut statuses = vec![DoctorStatus::Ready; DoctorComponent::ALL.len()];

		statuses[1] = DoctorStatus::Unavailable(DoctorIssue::DatabaseUnreachable);
		statuses[2] = DoctorStatus::Unknown(DoctorIssue::NotProbed);

		let report = report(statuses);
		let output =
			crate::render_report(Command::Status, OutputFormat::Human, ProfileKind::Local, &report);

		assert!(output.text().contains("16 ready, 1 unavailable, 1 unknown"));
		assert!(output.text().contains("database=unavailable(database_unreachable)"));
		assert!(output.text().contains("protocol=unknown(not_probed)"));
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
			ClientFailure::ProtocolDisconnected,
			ClientFailure::ProtocolTimeout,
			ClientFailure::ProtocolMajorMismatch,
			ClientFailure::ProtocolMinorMismatch,
			ClientFailure::ServerIdentityMismatch,
			ClientFailure::ProtocolMalformed,
			ClientFailure::ProtocolViolation,
			ClientFailure::ProtocolBackpressure,
		];

		for failure in failures {
			let human = crate::render_failure(Command::Doctor, OutputFormat::Human, failure);
			let json = crate::render_failure(Command::Doctor, OutputFormat::Json, failure);
			let value: serde_json::Value = serde_json::from_str(json.text()).unwrap();

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
		let output =
			crate::render_report(Command::Status, OutputFormat::Json, ProfileKind::Local, &report);

		assert_eq!(output.exit_code(), 0);
	}

	#[test]
	fn incomplete_ready_reports_render_as_closed_protocol_failures() {
		let complete = report(vec![DoctorStatus::Ready; DoctorComponent::ALL.len()]);
		let cases = [
			DoctorReport::new(ServerId::new(SERVER_ID).unwrap(), CURRENT_VERSION, Vec::new())
				.unwrap(),
			DoctorReport::new(
				ServerId::new(SERVER_ID).unwrap(),
				CURRENT_VERSION,
				vec![DoctorCheck::new(DoctorComponent::Configuration, DoctorStatus::Ready)],
			)
			.unwrap(),
			DoctorReport::new(
				ServerId::new(SERVER_ID).unwrap(),
				CURRENT_VERSION,
				complete.checks()[1..].to_vec(),
			)
			.unwrap(),
		];

		for report in cases {
			for format in [OutputFormat::Human, OutputFormat::Json] {
				let output =
					crate::render_report(Command::Status, format, ProfileKind::Local, &report);

				assert_eq!(output.exit_code(), 2);

				if format == OutputFormat::Json {
					let value: serde_json::Value = serde_json::from_str(output.text()).unwrap();

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

		let report =
			DoctorReport::new(ServerId::new(SERVER_ID).unwrap(), CURRENT_VERSION, checks).unwrap();
		let output =
			crate::render_report(Command::Status, OutputFormat::Json, ProfileKind::Local, &report);

		assert_eq!(output.exit_code(), 0);
	}
}
