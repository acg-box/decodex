use serde_json::Value;

pub(crate) fn protocol_response_summary(value: &Value) -> String {
	match value {
		Value::Null => String::from("null"),
		Value::Bool(_) => String::from("boolean"),
		Value::Number(_) => String::from("number"),
		Value::String(_) => String::from("string"),
		Value::Array(values) => format!("array(len={})", values.len()),
		Value::Object(entries) => {
			let mut keys = entries.keys().map(String::as_str).collect::<Vec<_>>();

			keys.sort_unstable();

			format!("object(keys={})", keys.join(","))
		},
	}
}
