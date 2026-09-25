//! Chief work client over the same-UID service protocol.

use std::{
	fmt::Write as _,
	io::Read as _,
	path::{Path, PathBuf},
};

use clap::Subcommand;
use decodex_protocol::{
	ChiefActionDto, ChiefClient, ChiefCommandResponse, ChiefSandboxDto, ChiefSnapshotResult,
	ChiefStartDto, ConversationModel, ConversationReasoningEffort, ConversationWorkingDirectory,
	EntityId, HistoryText, IdempotencyKey, MAX_CHIEF_DEPENDENCIES, MAX_CHIEF_PENDING_EVENTS,
	MAX_CHIEF_SNAPSHOT_BYTES, MAX_CHIEF_WORK_ITEMS, MAX_HISTORY_INLINE_BYTES, WireText,
};

use crate::{CommandOutput, OutputFormat, load_client_profile};

/// Explicit Chief operations. Execution stays inside the service.
#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum ChiefCommand {
	/// Cancel one pending model-capacity retry shown in history or status.
	CancelRetry {
		#[arg(long)]
		work_id: String,
		#[arg(long)]
		event_id: i64,
		#[arg(long)]
		idempotency_key: Option<String>,
	},
	/// Show the complete bounded work graph and undisposed event metadata.
	Status,
	/// Inspect one unresolved request before answering it.
	Request {
		#[arg(long)]
		event_id: i64,
	},
	/// Submit an explicit JSON response to one exact pending request.
	Answer {
		#[arg(long)]
		work_id: String,
		#[arg(long)]
		event_id: i64,
		#[arg(long)]
		idempotency_key: Option<String>,
		response_json: String,
	},
	/// Start or reconnect the personal Chief with an explicit model and execution directory.
	Start {
		#[arg(long)]
		model: String,
		#[arg(long)]
		/// Omit to inherit native reasoning configuration.
		effort: Option<String>,
		#[arg(long)]
		cwd: String,
		#[arg(long)]
		root_id: String,
		#[arg(long)]
		account_id: Option<String>,
		#[arg(long, conflicts_with = "full_access")]
		read_only: bool,
		/// Permit full host access without approval prompts for this Chief.
		#[arg(long, conflicts_with = "read_only")]
		full_access: bool,
		#[arg(long)]
		idempotency_key: Option<String>,
		prompt: String,
	},
	/// Send input to the existing Chief.
	Send {
		#[arg(long)]
		root_id: String,
		#[arg(long)]
		idempotency_key: Option<String>,
		text: String,
	},
	/// Interrupt one exact acknowledged turn.
	Interrupt {
		#[arg(long)]
		work_id: String,
		#[arg(long)]
		turn_id: String,
		#[arg(long)]
		idempotency_key: Option<String>,
	},
	/// Deliver an automation result with its source-owned event identity.
	Ingest {
		#[arg(long)]
		work_id: String,
		#[arg(long)]
		source_event_id: String,
		#[arg(long)]
		idempotency_key: Option<String>,
		#[arg(required_unless_present = "file", conflicts_with = "file")]
		payload: Option<String>,
		#[arg(long, conflicts_with = "payload")]
		file: Option<PathBuf>,
	},
}

pub async fn execute(
	command: ChiefCommand,
	output: OutputFormat,
	root: Option<&Path>,
	profile: Option<&str>,
	expected_server_id: Option<&str>,
) -> CommandOutput {
	if let ChiefCommand::Request { event_id } = command {
		return execute_request(event_id, output, root, profile, expected_server_id).await;
	}
	if !matches!(command, ChiefCommand::Status) {
		return execute_mutation(command, output, root, profile, expected_server_id).await;
	}
	let result = match load_client_profile(root, profile, expected_server_id) {
		Ok(profile) => ChiefClient::new(profile).query().await,
		Err(error) => Err(error),
	};
	let (document, text, exit_code) = match result {
		Ok(result) => {
			let mut text = String::new();
			let exit_code = match &result {
				ChiefSnapshotResult::Available(snapshot) => {
					let _ = writeln!(
						text,
						"Chief work: {} items, {} dependencies, {} pending events",
						snapshot.work_items.len(),
						snapshot.dependencies.len(),
						snapshot.pending_events.len()
					);
					if snapshot.work_items.is_empty() {
						text.push_str("No Chief work has been recorded.\n");
					}
					for item in &snapshot.work_items {
						let _ = writeln!(
							text,
							"{} | {:?} | {:?} | {:?} | {}",
							safe(&item.id),
							item.kind,
							item.status,
							item.dispatch_state,
							safe(&item.title)
						);
						if let Some(parent) = &item.parent_goal_id {
							let _ = writeln!(text, "  parent: {}", safe(parent));
						}
						if let Some(thread) = &item.codex_thread_id {
							let _ = writeln!(text, "  thread: {}", safe(thread));
						}
						if let Some(turn) = &item.active_turn_id {
							let _ = writeln!(text, "  turn: {}", safe(turn));
						}
						if let Some(time) = item.next_check_at_micros {
							let _ = writeln!(text, "  next check (Unix microseconds): {time}");
						}
					}
					for edge in &snapshot.dependencies {
						let _ = writeln!(
							text,
							"{} depends on {}",
							safe(&edge.work_item_id),
							safe(&edge.depends_on_id)
						);
					}
					for event in &snapshot.pending_events {
						let _ = writeln!(
							text,
							"pending {} | {} | {} | {} | delivery claimed: {}",
							event.id,
							safe(&event.work_item_id),
							safe(&event.event_kind),
							safe(&event.source_event_id),
							event.delivery_claimed
						);
					}
					0
				},
				ChiefSnapshotResult::Unavailable => {
					text.push_str("Chief work store is unavailable.");
					1
				},
				ChiefSnapshotResult::CapacityExceeded {
					work_items,
					dependencies,
					pending_events,
				} => {
					let _ = write!(
						text,
						"Chief snapshot exceeds the complete-response bounds: {work_items} work items, {dependencies} dependencies, {pending_events} pending events. Limits: {MAX_CHIEF_WORK_ITEMS} items, {MAX_CHIEF_DEPENDENCIES} dependencies, {MAX_CHIEF_PENDING_EVENTS} events, {MAX_CHIEF_SNAPSHOT_BYTES} encoded bytes. No partial graph was returned."
					);
					1
				},
			};
			(
				serde_json::json!({"schema":"decodex/chief-cli/1","command":"chief status","result":result}),
				text,
				exit_code,
			)
		},
		Err(failure) => (
			serde_json::json!({"schema":"decodex/chief-cli/1","command":"chief status","failure":failure}),
			format!("Chief query failed: {failure}"),
			1,
		),
	};
	CommandOutput {
		text: if output == OutputFormat::Json {
			serde_json::to_string_pretty(&document).expect("typed Chief output")
		} else {
			text.trim_end().to_owned()
		},
		exit_code,
		error_stream: output == OutputFormat::Human && exit_code != 0,
	}
}

fn safe(value: &str) -> String {
	value.chars().flat_map(char::escape_default).collect()
}

async fn execute_request(
	event_id: i64,
	output: OutputFormat,
	root: Option<&Path>,
	profile: Option<&str>,
	expected_server_id: Option<&str>,
) -> CommandOutput {
	let result = match load_client_profile(root, profile, expected_server_id) {
		Ok(profile) => ChiefClient::new(profile).request(event_id).await,
		Err(error) => Err(error),
	};
	let (document, text, exit_code) = match result {
		Ok(result) => match &result {
			decodex_protocol::ChiefRequestResult::Available {
				event_id,
				work_id,
				method,
				request_json,
			} => (
				serde_json::json!(result),
				format!(
					"Request {event_id} | {} | {}\n{}\n",
					safe(work_id),
					safe(method),
					safe(request_json.as_str())
				),
				0,
			),
			decodex_protocol::ChiefRequestResult::Unavailable =>
				(serde_json::json!(result), "Request unavailable or no longer pending.\n".into(), 1),
			// The high-level client assembles transport pages before returning.
			decodex_protocol::ChiefRequestResult::Page { .. } => (
				serde_json::json!({"error": "Incomplete request response"}),
				"Request unavailable: incomplete request response.\n".into(),
				1,
			),
		},
		Err(error) => (
			serde_json::json!({"error":format!("{error:?}")}),
			format!("Request unavailable: {error:?}\n"),
			1,
		),
	};
	CommandOutput {
		text: match output {
			OutputFormat::Json => format!("{document}\n"),
			OutputFormat::Human => text,
		},
		error_stream: false,
		exit_code,
	}
}

fn wire_identity(value: String) -> Result<WireText, &'static str> {
	if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
		return Err("invalid turn or source event identity");
	}
	WireText::new(value).map_err(|_| "invalid turn or source event identity")
}

fn prepare(command: ChiefCommand) -> Result<(ChiefActionDto, IdempotencyKey), &'static str> {
	let text = |value: String| {
		if value.trim().is_empty() {
			return Err("input text must not be empty");
		}
		HistoryText::new(value).map_err(|_| "input text exceeds the protocol limit")
	};
	let identity =
		|value: String| EntityId::new(value).map_err(|_| "invalid work or account identity");

	let (action, key) = match command {
		ChiefCommand::CancelRetry { work_id, event_id, idempotency_key } => {
			if event_id <= 0 {
				return Err("retry cancellation requires a positive event identity");
			}
			(
				ChiefActionDto::CancelCapacityRetry { work_id: identity(work_id)?, event_id },
				idempotency_key,
			)
		},
		ChiefCommand::Answer { work_id, event_id, idempotency_key, response_json } => {
			if event_id <= 0
				|| !serde_json::from_str::<serde_json::Value>(&response_json)
					.is_ok_and(|value| value.is_object())
			{
				return Err("answer requires a positive event identity and a JSON response object");
			}
			(
				ChiefActionDto::Respond {
					work_id: identity(work_id)?,
					event_id,
					response_json: text(response_json)?,
				},
				idempotency_key,
			)
		},
		ChiefCommand::Start {
			model,
			effort,
			cwd,
			root_id,
			account_id,
			read_only,
			full_access,
			idempotency_key,
			prompt,
		} => (
			ChiefActionDto::Start(ChiefStartDto {
				root_id: identity(root_id)?,
				prompt: text(prompt)?,
				model: ConversationModel::new(model).map_err(|_| "invalid model")?,
				effort: effort
					.map(|effort| {
						serde_json::from_value::<ConversationReasoningEffort>(serde_json::json!(
							effort
						))
						.map_err(|_| "invalid reasoning effort")
					})
					.transpose()?,
				cwd: ConversationWorkingDirectory::new(cwd)
					.map_err(|_| "execution directory must be an absolute bounded path")?,
				account_id: account_id.map(identity).transpose()?,
				sandbox: if full_access {
					ChiefSandboxDto::FullAccess
				} else if read_only {
					ChiefSandboxDto::ReadOnly
				} else {
					ChiefSandboxDto::WorkspaceWrite
				},
			}),
			idempotency_key,
		),
		ChiefCommand::Send { root_id, idempotency_key, text: input } => (
			ChiefActionDto::Send { root_id: identity(root_id)?, text: text(input)? },
			idempotency_key,
		),
		ChiefCommand::Interrupt { work_id, turn_id, idempotency_key } => (
			ChiefActionDto::Interrupt {
				work_id: identity(work_id)?,
				turn_id: wire_identity(turn_id)?,
			},
			idempotency_key,
		),
		ChiefCommand::Ingest { work_id, source_event_id, idempotency_key, payload, file } => {
			let payload = match (payload, file) {
				(Some(payload), None) => payload,
				(None, Some(path)) => {
					if !std::fs::metadata(&path)
						.map_err(|_| "cannot inspect result file")?
						.is_file()
					{
						return Err("result source must be a regular file");
					}
					let file = std::fs::File::open(path).map_err(|_| "cannot read result file")?;
					if !file.metadata().map_err(|_| "cannot inspect result file")?.is_file() {
						return Err("result source must be a regular file");
					}
					let mut payload = String::new();
					file.take((MAX_HISTORY_INLINE_BYTES + 1) as u64)
						.read_to_string(&mut payload)
						.map_err(|_| "result file must contain UTF-8 text")?;
					payload
				},
				_ => return Err("supply result text or one result file"),
			};
			(
				ChiefActionDto::AutomationResult {
					work_id: identity(work_id)?,
					source_event_id: wire_identity(source_event_id)?,
					payload: text(payload)?,
				},
				idempotency_key,
			)
		},
		ChiefCommand::Status | ChiefCommand::Request { .. } => {
			return Err("query is not a mutation");
		},
	};
	Ok((action, command_key(key)?))
}

fn command_key(key: Option<String>) -> Result<IdempotencyKey, &'static str> {
	let key = match key {
		Some(key) => key,
		None => {
			let mut bytes = [0_u8; 16];
			std::fs::File::open("/dev/urandom")
				.and_then(|mut source| source.read_exact(&mut bytes))
				.map_err(|_| "cannot generate command identity")?;
			let mut key = String::from("chief-");
			for byte in bytes {
				let _ = write!(key, "{byte:02x}");
			}
			key
		},
	};
	IdempotencyKey::new(key).map_err(|_| "invalid idempotency key")
}

async fn execute_mutation(
	command: ChiefCommand,
	output: OutputFormat,
	root: Option<&Path>,
	profile: Option<&str>,
	expected_server_id: Option<&str>,
) -> CommandOutput {
	let prepared = prepare(command);
	let (document, text, exit_code) = match prepared {
		Err(error) => (
			serde_json::json!({"schema":"decodex/chief-cli/1","error":error}),
			format!("Chief command rejected locally: {error}"),
			2,
		),
		Ok((action, key)) => {
			let result = match load_client_profile(root, profile, expected_server_id) {
				Ok(profile) => ChiefClient::new(profile).execute(action, key.clone()).await,
				Err(error) => Err(error),
			};
			let (text, exit_code) = match &result {
				Ok(ChiefCommandResponse::Accepted { work_id }) =>
					(format!("Chief command accepted for {}.", safe(work_id.as_str())), 0),
				Ok(ChiefCommandResponse::Rejected { error }) =>
					(format!("Chief command rejected: {error:?}"), 1),
				Ok(ChiefCommandResponse::PotentiallyDispatched { failure }) => (
					format!(
						"Chief command may have been accepted: {failure}. Inspect chief status before sending more work; no retry was sent."
					),
					1,
				),
				Err(failure) => (format!("Chief command was not dispatched: {failure}"), 1),
			};
			let document = match result {
				Ok(result) => {
					serde_json::json!({"schema":"decodex/chief-cli/1","idempotency_key":key,"result":result})
				},
				Err(failure) => {
					serde_json::json!({"schema":"decodex/chief-cli/1","idempotency_key":key,"failure":failure})
				},
			};
			(document, format!("{text}\nCommand identity: {}", safe(key.as_str())), exit_code)
		},
	};
	CommandOutput {
		text: if output == OutputFormat::Json {
			serde_json::to_string_pretty(&document).expect("typed Chief output")
		} else {
			text
		},
		exit_code,
		error_stream: output == OutputFormat::Human && exit_code != 0,
	}
}

#[cfg(test)]
mod tests {
	use crate::{Cli, Command, OutputFormat};
	use clap::Parser as _;

	#[test]
	fn cancel_retry_command_binds_exact_work_and_event() {
		let cli = Cli::try_parse_from([
			"decodex",
			"chief",
			"cancel-retry",
			"--work-id",
			"worker",
			"--event-id",
			"7",
		])
		.unwrap();
		let Command::Chief(command) = cli.command else {
			panic!("Chief command");
		};
		assert!(
			matches!(super::prepare(command).unwrap().0,decodex_protocol::ChiefActionDto::CancelCapacityRetry {work_id,event_id:7} if work_id.as_str()=="worker")
		);
		assert!(
			super::prepare(super::ChiefCommand::CancelRetry {
				work_id: "worker".into(),
				event_id: 0,
				idempotency_key: None
			})
			.is_err()
		);
	}

	#[test]
	fn chief_answer_binds_event_and_rejects_non_object_responses() {
		let cli = Cli::try_parse_from(["decodex", "chief", "request", "--event-id", "7"]).unwrap();
		assert!(matches!(
			cli.command,
			Command::Chief(super::ChiefCommand::Request { event_id: 7 })
		));
		for (event_id, response, valid) in [
			(7, "{\"decision\":\"decline\"}", true),
			(0, "{}", false),
			(7, "[]", false),
			(7, "invalid", false),
		] {
			let result = super::prepare(super::ChiefCommand::Answer {
				work_id: "worker".into(),
				event_id,
				idempotency_key: Some("answer-7".into()),
				response_json: response.into(),
			});
			assert_eq!(result.is_ok(), valid);
			if let Ok((action, _)) = result {
				assert!(
					matches!(action, decodex_protocol::ChiefActionDto::Respond { event_id: 7, work_id, .. } if work_id.as_str() == "worker")
				);
			}
		}
	}

	#[test]
	fn chief_status_is_a_read_only_cli_command_with_structured_output() {
		let cli = Cli::try_parse_from(["decodex", "--output", "json", "chief", "status"]).unwrap();
		assert_eq!(cli.output, OutputFormat::Json);
		assert!(matches!(cli.command, Command::Chief(super::ChiefCommand::Status)));
		assert!(Cli::try_parse_from(["decodex", "chief", "run"]).is_err());
	}

	#[test]
	fn chief_start_without_effort_inherits_native_configuration() {
		let cli = Cli::try_parse_from([
			"decodex",
			"chief",
			"start",
			"--model",
			"model",
			"--cwd",
			"/tmp",
			"--root-id",
			"personal",
			"--read-only",
			"Plan work",
		])
		.unwrap();
		let Command::Chief(command) = cli.command else { panic!("Chief command") };
		let (action, _) = super::prepare(command).unwrap();
		assert!(
			matches!(action,decodex_protocol::ChiefActionDto::Start(start) if start.effort.is_none())
		);
	}

	#[test]
	fn chief_mutations_parse_exact_model_turn_and_source_identity() {
		let cli = Cli::try_parse_from([
			"decodex",
			"chief",
			"start",
			"--model",
			"gpt-6-astra",
			"--effort",
			"medium",
			"--cwd",
			"/tmp",
			"--root-id",
			"personal",
			"--read-only",
			"Plan my work",
		])
		.unwrap();
		let Command::Chief(command) = cli.command else {
			panic!("Chief command");
		};
		let (action, key) = super::prepare(command).unwrap();
		assert!(key.as_str().starts_with("chief-"));
		assert!(
			matches!(action, decodex_protocol::ChiefActionDto::Start(start) if start.model.as_str() == "gpt-6-astra" && start.effort == Some(decodex_protocol::ConversationReasoningEffort::Medium) && start.sandbox == decodex_protocol::ChiefSandboxDto::ReadOnly)
		);
		for args in [
			vec!["decodex", "chief", "send", "--root-id", "personal", "Next task"],
			vec!["decodex", "chief", "interrupt", "--work-id", "worker", "--turn-id", "turn-one"],
			vec![
				"decodex",
				"chief",
				"ingest",
				"--work-id",
				"goal",
				"--source-event-id",
				"source-one",
				"Result",
			],
		] {
			let cli = Cli::try_parse_from(args).unwrap();
			let Command::Chief(command) = cli.command else {
				panic!("Chief command");
			};
			assert!(super::prepare(command).is_ok());
		}
		assert!(
			Cli::try_parse_from(["decodex", "chief", "interrupt", "--work-id", "worker"]).is_err()
		);
		assert!(
			Cli::try_parse_from(["decodex", "chief", "start", "--root-id", "personal", "Hello"])
				.is_err()
		);
	}

	#[test]
	fn command_bounds_fail_before_connect_and_preserve_explicit_key() {
		let cli = Cli::try_parse_from([
			"decodex",
			"chief",
			"send",
			"--root-id",
			"personal",
			"--idempotency-key",
			"exact-once",
			"Hello",
		])
		.unwrap();
		let Command::Chief(command) = cli.command else {
			panic!("Chief command");
		};
		assert_eq!(super::prepare(command).unwrap().1.as_str(), "exact-once");
		assert!(
			super::prepare(super::ChiefCommand::Send {
				root_id: "personal".into(),
				idempotency_key: None,
				text: "x".repeat(decodex_protocol::MAX_HISTORY_INLINE_BYTES + 1)
			})
			.is_err()
		);
		assert!(
			super::prepare(super::ChiefCommand::Send {
				root_id: "personal".into(),
				idempotency_key: None,
				text: " ".into()
			})
			.is_err()
		);
	}

	#[test]
	fn full_access_requires_an_explicit_flag_and_conflicts_with_read_only() {
		let base = [
			"decodex",
			"chief",
			"start",
			"--model",
			"gpt-6-astra",
			"--effort",
			"medium",
			"--cwd",
			"/tmp",
			"--root-id",
			"personal",
			"Hello",
		];
		let default = Cli::try_parse_from(base).unwrap();
		let Command::Chief(command) = default.command else {
			panic!("Chief command");
		};
		assert!(
			matches!(super::prepare(command).unwrap().0, decodex_protocol::ChiefActionDto::Start(start) if start.sandbox == decodex_protocol::ChiefSandboxDto::WorkspaceWrite)
		);
		let mut args = base.to_vec();
		args.push("--full-access");
		let full = Cli::try_parse_from(args.clone()).unwrap();
		let Command::Chief(command) = full.command else {
			panic!("Chief command");
		};
		assert!(
			matches!(super::prepare(command).unwrap().0, decodex_protocol::ChiefActionDto::Start(start) if start.sandbox == decodex_protocol::ChiefSandboxDto::FullAccess)
		);
		args.push("--read-only");
		assert!(Cli::try_parse_from(args).is_err());
	}

	#[test]
	fn automation_result_file_is_bounded_before_connection() {
		use std::io::Write as _;
		let mut file = tempfile::NamedTempFile::new().unwrap();
		file.write_all(&vec![b'x'; decodex_protocol::MAX_HISTORY_INLINE_BYTES + 1]).unwrap();
		let command = super::ChiefCommand::Ingest {
			work_id: "goal".into(),
			source_event_id: "source-event".into(),
			idempotency_key: Some("one-result".into()),
			payload: None,
			file: Some(file.path().to_owned()),
		};
		assert!(super::prepare(command).is_err());
	}
}
