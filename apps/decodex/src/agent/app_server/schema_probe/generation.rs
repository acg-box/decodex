use std::{fs, path::PathBuf, process::Command};

use crate::{
	agent::{
		app_server::schema_probe::{
			constants::{
				APP_SERVER_SCHEMA_GENERATE_COMMAND, APP_SERVER_SCHEMA_PROBE_OUT_DIR,
				APP_SERVER_SCHEMA_REQUIRED_MARKERS,
			},
			evidence::AppServerSchemaProbeEvidence,
			output, validation,
		},
		json_rpc::{self, AppServerProcessEnv},
	},
	prelude::{Result, eyre},
};

pub(in crate::agent::app_server) fn probe_app_server_schema(
	process_env: &AppServerProcessEnv,
) -> Result<AppServerSchemaProbeEvidence> {
	let out_dir = PathBuf::from(APP_SERVER_SCHEMA_PROBE_OUT_DIR);

	if out_dir.exists() {
		fs::remove_dir_all(&out_dir)?;
	}

	if let Some(parent) = out_dir.parent() {
		fs::create_dir_all(parent)?;
	}

	let mut command = Command::new(json_rpc::app_server_command_program());

	command.args(["app-server", "generate-json-schema", "--experimental", "--out"]);
	command.arg(&out_dir);
	process_env.apply_to(&mut command)?;

	let output = command.output()?;

	if !output.status.success() {
		eyre::bail!(
			"`{APP_SERVER_SCHEMA_GENERATE_COMMAND}` failed with status {}: stdout={} stderr={}",
			output.status,
			output::command_output_excerpt(&output.stdout),
			output::command_output_excerpt(&output.stderr)
		);
	}

	validation::validate_generated_app_server_schema(&out_dir)?;

	Ok(AppServerSchemaProbeEvidence::checked(
		APP_SERVER_SCHEMA_PROBE_OUT_DIR.to_owned(),
		APP_SERVER_SCHEMA_REQUIRED_MARKERS,
	))
}
