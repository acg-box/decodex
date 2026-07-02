#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::agent::app_server) struct AppServerSchemaProbeEvidence {
	cache_path: String,
	marker_count: usize,
	required_markers: Vec<&'static str>,
}
impl AppServerSchemaProbeEvidence {
	pub(in crate::agent::app_server::schema_probe) fn checked(
		cache_path: String,
		required_markers: &[&'static str],
	) -> Self {
		Self {
			cache_path,
			marker_count: required_markers.len(),
			required_markers: required_markers.to_vec(),
		}
	}
}
