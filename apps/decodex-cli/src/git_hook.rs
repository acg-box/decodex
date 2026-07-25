use std::{
	fs,
	io::{self, BufRead},
	path::PathBuf,
	process::Command,
};

use clap::{Args, Subcommand};
use serde::Deserialize;

const COMMIT_MESSAGE_SCHEMA: &str = "decodex/commit/2";

/// Local Git hook entrypoint installed by the operator Git configuration.
#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct GitHookCommand {
	/// Exact hook operation requested by Git.
	#[command(subcommand)]
	pub(crate) command: GitHookSubcommand,
}

/// Supported local Git hook operations.
#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum GitHookSubcommand {
	/// Validate the commit message file passed by Git.
	CommitMsg {
		/// Commit message file path supplied by Git.
		#[arg(value_name = "MESSAGE_FILE")]
		message_file: PathBuf,
	},
	/// Validate commits supplied on standard input by Git before a push.
	PrePush {
		/// Remote name supplied by Git.
		#[arg(value_name = "REMOTE_NAME")]
		remote_name: String,
		/// Remote URL supplied by Git.
		#[arg(value_name = "REMOTE_URL")]
		remote_url: String,
	},
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitRecord {
	schema: String,
	change: String,
	authority: String,
	impact: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrePushUpdate {
	local_ref: String,
	local_oid: String,
	remote_ref: String,
	remote_oid: String,
}

pub(crate) fn execute(command: &GitHookCommand) -> Result<(), String> {
	match &command.command {
		GitHookSubcommand::CommitMsg { message_file } => validate_commit_message_file(message_file),
		GitHookSubcommand::PrePush { remote_name, remote_url } => {
			let updates = read_pre_push_updates(io::stdin().lock())?;

			validate_pre_push_updates(remote_name, remote_url, &updates)
		},
	}
}

fn validate_commit_message_file(message_file: &PathBuf) -> Result<(), String> {
	let raw = fs::read_to_string(message_file).map_err(|error| {
		format!("failed to read commit message file `{}`: {error}", message_file.display())
	})?;
	let subject = extract_commit_subject(&raw)?;

	validate_subject(subject)
}

fn extract_commit_subject(raw: &str) -> Result<&str, String> {
	let mut content_lines =
		raw.lines().filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
	let subject =
		content_lines.next().ok_or_else(|| String::from("commit message must not be empty"))?;

	if content_lines.next().is_some() {
		return Err(String::from("Decodex commit messages must contain one non-comment line"));
	}

	Ok(subject)
}

fn validate_subject(subject: &str) -> Result<(), String> {
	let subject = normalize_single_line("commit_message", subject)?;
	let record: CommitRecord = serde_json::from_str(subject)
		.map_err(|error| format!("invalid Decodex commit message subject: {error}"))?;

	if record.schema != COMMIT_MESSAGE_SCHEMA {
		return Err(format!(
			"`commit_message.schema` must be `{COMMIT_MESSAGE_SCHEMA}`, not `{}`",
			record.schema
		));
	}

	normalize_single_line("change", &record.change)?;
	validate_authority(&record.authority)?;

	if !matches!(record.impact.as_str(), "compatible" | "breaking") {
		return Err(format!(
			"`commit_message.impact` must be `compatible` or `breaking`, not `{}`",
			record.impact
		));
	}

	Ok(())
}

fn normalize_single_line<'a>(field: &str, value: &'a str) -> Result<&'a str, String> {
	let trimmed = value.trim();

	if trimmed.is_empty() {
		return Err(format!("`{field}` must not be empty"));
	}
	if trimmed != value {
		return Err(format!("`{field}` must not include surrounding whitespace"));
	}
	if trimmed.contains('\n') || trimmed.contains('\r') {
		return Err(format!("`{field}` must stay on one line"));
	}

	Ok(trimmed)
}

fn validate_authority(value: &str) -> Result<(), String> {
	let value = normalize_single_line("authority", value)?;

	if matches!(value, "manual" | "baseline") || looks_like_issue_identifier(value) {
		return Ok(());
	}

	Err(String::from("`authority` must look like an issue identifier or be `manual` or `baseline`"))
}

fn looks_like_issue_identifier(value: &str) -> bool {
	let Some((prefix, number)) = value.rsplit_once('-') else {
		return false;
	};

	!prefix.is_empty()
		&& !number.is_empty()
		&& prefix.chars().all(|character| character.is_ascii_alphanumeric())
		&& number.chars().all(|character| character.is_ascii_digit())
}

fn read_pre_push_updates(reader: impl BufRead) -> Result<Vec<PrePushUpdate>, String> {
	let mut updates = Vec::new();

	for line in reader.lines() {
		let line = line.map_err(|error| format!("failed to read pre-push input: {error}"))?;

		if line.trim().is_empty() {
			continue;
		}

		let fields = line.split_whitespace().collect::<Vec<_>>();

		if fields.len() != 4 {
			return Err(format!("invalid pre-push input line `{line}`"));
		}
		if !is_object_id(fields[1]) || !is_object_id(fields[3]) {
			return Err(format!("invalid object ID in pre-push input line `{line}`"));
		}

		updates.push(PrePushUpdate {
			local_ref: fields[0].to_owned(),
			local_oid: fields[1].to_owned(),
			remote_ref: fields[2].to_owned(),
			remote_oid: fields[3].to_owned(),
		});
	}

	Ok(updates)
}

fn validate_pre_push_updates(
	remote_name: &str,
	remote_url: &str,
	updates: &[PrePushUpdate],
) -> Result<(), String> {
	let remote_exclusions = live_remote_commit_exclusion_oids(remote_name, remote_url);

	for update in updates {
		if is_zero_oid(&update.local_oid) {
			continue;
		}

		for oid in new_commit_oids(update, &remote_exclusions)? {
			let message = git_stdout(&["show", "-s", "--format=%B", &oid])?;
			let message = message.lines().collect::<Vec<_>>().join("\n");
			let subject = extract_commit_subject(&message).map_err(|error| {
				format!(
					"commit `{oid}` on `{}` has an invalid Decodex commit message body: {error}",
					update.local_ref
				)
			})?;

			validate_subject(subject).map_err(|error| {
				format!(
					"commit `{oid}` on `{}` has an invalid Decodex commit message subject: {error}",
					update.local_ref
				)
			})?;
		}
	}

	Ok(())
}

fn new_commit_oids(
	update: &PrePushUpdate,
	remote_exclusions: &[String],
) -> Result<Vec<String>, String> {
	let revision = if is_zero_oid(&update.remote_oid) {
		update.local_oid.clone()
	} else {
		format!("{}..{}", update.remote_oid, update.local_oid)
	};
	let mut args = vec![String::from("rev-list"), revision];

	if !remote_exclusions.is_empty() {
		args.push(String::from("--not"));
		args.extend(remote_exclusions.iter().cloned());
	}

	git_lines(&args).map_err(|error| {
		format!(
			"failed to list commits for push update `{} -> {}`: {error}",
			update.local_ref, update.remote_ref
		)
	})
}

fn live_remote_commit_exclusion_oids(remote_name: &str, remote_url: &str) -> Vec<String> {
	let remote = if remote_url.is_empty() { remote_name } else { remote_url };

	if remote.is_empty() {
		return Vec::new();
	}

	let Ok(lines) = git_lines(&[String::from("ls-remote"), String::from("--"), remote.to_owned()])
	else {
		return Vec::new();
	};
	let mut oids = lines
		.into_iter()
		.filter_map(|line| line.split_once('\t').map(|(oid, _)| oid.to_owned()))
		.filter(|oid| is_object_id(oid))
		.filter_map(|oid| local_commit_oid(&oid).ok().flatten())
		.collect::<Vec<_>>();

	oids.sort();
	oids.dedup();
	oids
}

fn local_commit_oid(oid: &str) -> Result<Option<String>, String> {
	let output = Command::new("git")
		.args(["rev-parse", "--verify", "--quiet", &format!("{oid}^{{commit}}")])
		.output()
		.map_err(|error| format!("failed to start Git: {error}"))?;

	if !output.status.success() {
		return Ok(None);
	}

	let oid = String::from_utf8(output.stdout)
		.map_err(|error| format!("Git emitted non-UTF-8 output: {error}"))?
		.trim()
		.to_owned();

	Ok(is_object_id(&oid).then_some(oid))
}

fn git_lines(args: &[String]) -> Result<Vec<String>, String> {
	Ok(git_stdout_owned(args)?.lines().map(ToOwned::to_owned).collect())
}

fn git_stdout(args: &[&str]) -> Result<String, String> {
	let args = args.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>();

	git_stdout_owned(&args)
}

fn git_stdout_owned(args: &[String]) -> Result<String, String> {
	let output = Command::new("git")
		.args(args)
		.output()
		.map_err(|error| format!("failed to start Git: {error}"))?;

	if !output.status.success() {
		return Err(format!(
			"`git {}` failed: {}",
			args.join(" "),
			String::from_utf8_lossy(&output.stderr).trim()
		));
	}

	String::from_utf8(output.stdout)
		.map_err(|error| format!("Git emitted non-UTF-8 output: {error}"))
}

fn is_object_id(value: &str) -> bool {
	matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_zero_oid(value: &str) -> bool {
	is_object_id(value) && value.bytes().all(|byte| byte == b'0')
}

#[cfg(test)]
mod tests {
	use std::io::Cursor;

	use super::{extract_commit_subject, read_pre_push_updates, validate_subject};

	const ZERO_OID: &str = "0000000000000000000000000000000000000000";
	const LOCAL_OID: &str = "1111111111111111111111111111111111111111";
	const REMOTE_OID: &str = "2222222222222222222222222222222222222222";

	#[test]
	fn commit_subject_contract_accepts_exact_schema_and_rejects_legacy_text() {
		validate_subject(
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"manual","impact":"compatible"}"#,
		)
		.expect("schema record should validate");

		for invalid in [
			"ship fix",
			r#"{"schema":"decodex/commit/1","change":"ship fix","authority":"manual","impact":"compatible"}"#,
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"unknown","impact":"compatible"}"#,
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"manual","impact":"unknown"}"#,
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"manual","impact":"compatible","extra":true}"#,
		] {
			validate_subject(invalid).expect_err("invalid record should fail closed");
		}
	}

	#[test]
	fn commit_subject_extraction_ignores_comments_but_rejects_bodies() {
		let subject = extract_commit_subject(
			"\n# comment\n{\"schema\":\"decodex/commit/2\",\"change\":\"ship fix\",\"authority\":\"manual\",\"impact\":\"compatible\"}\n# trailing\n",
		)
		.expect("subject should extract");

		assert!(subject.contains("\"schema\":\"decodex/commit/2\""));
		assert!(extract_commit_subject("subject\nbody\n").is_err());
	}

	#[test]
	fn pre_push_input_accepts_updates_and_deletions_but_rejects_invalid_oids() {
		let updates = read_pre_push_updates(Cursor::new(format!(
			"refs/heads/main {LOCAL_OID} refs/heads/main {REMOTE_OID}\n\
			 refs/heads/topic {ZERO_OID} refs/heads/topic {REMOTE_OID}\n"
		)))
		.expect("valid input should parse");

		assert_eq!(updates.len(), 2);
		assert_eq!(updates[0].local_ref, "refs/heads/main");
		assert_eq!(updates[1].local_oid, ZERO_OID);
		assert!(
			read_pre_push_updates(Cursor::new("refs/heads/main nope refs/heads/main also-nope\n"))
				.is_err()
		);
	}
}
