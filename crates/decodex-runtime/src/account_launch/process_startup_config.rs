//! Inspect native startup requirements without starting a conversation or changing sign-in.
use std::{
	env,
	ffi::{OsStr, OsString},
	io::{Read as _, Seek as _, SeekFrom},
	path::{Path, PathBuf},
	process::Stdio,
	time::Duration,
};

use serde_json::Value;
use zeroize::Zeroizing;

use crate::account_launch::process::{self, AttestedAppServerProfile, ClientInfo, ProcessGuard};
use decodex_protocol::{
	NativeStartupConfigLocation, NativeStartupConfigStatus, NativeStartupConfiguration,
	NativeStartupLoginMethod,
};

const MAX_OUTPUT_BYTES: u64 = 256 * 1_024;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Check configuration with the same verified component, switches and client identity as startup.
/// Use the shared native sign-in. Do not sign out, start threads, retry, build or install a
/// runtime.
pub(crate) fn inspect_native_startup_configuration(directory: &Path) -> NativeStartupConfiguration {
	let mut report = NativeStartupConfiguration {
		directory: safe_path(directory.to_str()),
		codex_sha256: None,
		status: NativeStartupConfigStatus::Unavailable,
	};

	if report.directory.is_none() {
		return report;
	}

	let Ok(profile) = AttestedAppServerProfile::attest(directory, TIMEOUT) else {
		return report;
	};

	report.codex_sha256 = Some(process::hex_digest(&profile.command.executable_digest));

	let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) else {
		return report;
	};

	report.status = read(&profile, &PathBuf::from(home).join(".codex"))
		.unwrap_or(NativeStartupConfigStatus::Unavailable);

	report
}

fn read(profile: &AttestedAppServerProfile, home: &Path) -> Option<NativeStartupConfigStatus> {
	let client =
		serde_json::to_string(&ClientInfo { name: "decodex", version: env!("CARGO_PKG_VERSION") })
			.ok()?;
	let mut output = tempfile::tempfile().ok()?;
	let mut args: Vec<OsString> = profile
		.command
		.app_server_args
		.iter()
		.map(|arg| {
			if arg == OsStr::new("--stdio") { "--print-startup-config".into() } else { arg.clone() }
		})
		.collect();

	args.extend(["--startup-client-info".into(), client.into()]);

	// The anonymous output file cannot survive a process crash. Do not limit native auth/cache
	// file writes with RLIMIT_FSIZE; bound the decoded response after the owned child exits.
	let (status, _guard) = process::run_preflight_command(
		&profile.command,
		home,
		&args,
		Stdio::from(output.try_clone().ok()?),
		TIMEOUT,
		None,
		ProcessGuard::prepare(None).ok()?,
	)
	.ok()?;

	if output.metadata().ok()?.len() > MAX_OUTPUT_BYTES {
		return None;
	}

	output.seek(SeekFrom::Start(0)).ok()?;

	let mut bytes = Zeroizing::new(Vec::new());

	output.take(MAX_OUTPUT_BYTES + 1).read_to_end(&mut bytes).ok()?;

	if bytes.len() as u64 > MAX_OUTPUT_BYTES {
		return None;
	}

	project(&serde_json::from_slice(&bytes).ok()?, status.success())
}

fn project(value: &Value, success: bool) -> Option<NativeStartupConfigStatus> {
	if let Some(error) = value.get("error") {
		if success
			|| !error.is_object()
			|| !error["code"].is_i64()
			|| !error["message"].is_string()
			|| value.get("configRequirements").is_some()
		{
			return None;
		}

		let data = &error["data"];
		let position =
			|key: &str| data[key].as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0);
		let location = safe_path(data["filePath"].as_str()).map(|file| {
			NativeStartupConfigLocation { file, line: position("line"), column: position("column") }
		});

		return Some(NativeStartupConfigStatus::Rejected {
			location,
			cloud_policy: data["reason"] == "cloudConfigBundle",
		});
	}

	if !success {
		return None;
	}

	let version = value["appServerVersion"].as_str()?;

	if version.is_empty()
		|| version.len() > 128
		|| !version.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+_".contains(&b))
	{
		return None;
	}

	let requirements = value.get("configRequirements")?.get("requirements")?;

	if !requirements.is_null() && !requirements.is_object() {
		return None;
	}

	let login_methods = match requirements.get("allowedLoginMethods") {
		None | Some(Value::Null) => None,
		Some(Value::Array(methods)) if methods.len() <= 2 => Some(
			methods
				.iter()
				.map(|method| match method.as_str()? {
					"chatgpt" => Some(NativeStartupLoginMethod::Chatgpt),
					"api" => Some(NativeStartupLoginMethod::Api),
					_ => None,
				})
				.collect::<Option<Vec<_>>>()?,
		),
		_ => return None,
	};

	Some(NativeStartupConfigStatus::Loaded {
		version: version.into(),
		login_methods,
		requirements_present: !requirements.is_null(),
	})
}

fn safe_path(value: Option<&str>) -> Option<String> {
	value
		.filter(|v| {
			!v.is_empty()
				&& v.len() <= 4_096
				&& Path::new(v).is_absolute()
				&& !v.chars().any(char::is_control)
				&& !decodex_core::contains_credential_material(v)
		})
		.map(str::to_owned)
}

#[cfg(test)]
#[path = "process_startup_config_tests.rs"]
mod tests;
