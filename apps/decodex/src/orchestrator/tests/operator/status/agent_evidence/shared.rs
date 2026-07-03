use crate::orchestrator::tests::operator::status::{Path, Value, fs};

pub(super) fn read_json_file(path: &Path) -> Value {
	let body = fs::read_to_string(path).expect("JSON file should exist");

	serde_json::from_str(&body).expect("JSON file should parse")
}
