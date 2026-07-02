use std::{collections::BTreeMap, path::Path};

use crate::{
	agent::app_server::schema_probe::{
		constants::APP_SERVER_SCHEMA_REQUIRED_MARKERS, dynamic_tools, markers, method_unions,
	},
	prelude::{Result, eyre},
};

pub(in crate::agent::app_server) fn validate_generated_app_server_schema(
	out_dir: &Path,
) -> Result<()> {
	let mut marker_presence = APP_SERVER_SCHEMA_REQUIRED_MARKERS
		.iter()
		.map(|marker| (*marker, false))
		.collect::<BTreeMap<_, _>>();
	let schema_file_count = markers::collect_schema_markers(out_dir, &mut marker_presence)?;

	if schema_file_count == 0 {
		eyre::bail!(
			"Generated app-server schema directory `{}` contained no JSON files.",
			out_dir.display()
		);
	}

	let missing_markers = marker_presence
		.iter()
		.filter_map(|(marker, present)| (!*present).then_some(*marker))
		.collect::<Vec<_>>();

	if !missing_markers.is_empty() {
		eyre::bail!(
			"Generated app-server schema was missing required Decodex markers: {}",
			missing_markers.join(", ")
		);
	}

	dynamic_tools::validate_generated_dynamic_tool_schema(out_dir)?;
	method_unions::validate_generated_app_server_method_unions(out_dir)?;

	Ok(())
}
