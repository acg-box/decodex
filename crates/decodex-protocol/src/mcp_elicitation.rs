//! Bounded MCP form controls. Defaults are suggestions, never submitted answers.
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// An advertised choice retains its wire value separately from its display label.
#[derive(Clone, Debug, PartialEq)]
pub struct McpFormChoice {
	/// Human-readable option label.
	pub label: String,
	/// Exact submitted value.
	pub value: Value,
}
/// One primitive form field.
#[derive(Clone, Debug)]
pub struct McpFormField {
	/// Exact property name.
	pub id: String,
	/// Human-readable name.
	pub title: String,
	/// Provider help text.
	pub description: Option<String>,
	/// Whether an explicit value is required.
	pub required: bool,
	/// Primitive JSON type.
	pub kind: String,
	/// Advertised choices; empty for free text and numbers.
	pub choices: Vec<McpFormChoice>,
	schema: Value,
}

fn choices(schema: &Value) -> Result<Vec<McpFormChoice>, String> {
	if let Some(values) = schema["enum"].as_array() {
		return values
			.iter()
			.enumerate()
			.map(|(index, value)| {
				let text = value.as_str().ok_or("Unsupported enum value")?;
				Ok(McpFormChoice {
					label: schema["enumNames"][index].as_str().unwrap_or(text).into(),
					value: value.clone(),
				})
			})
			.collect();
	}
	if let Some(values) = schema["oneOf"].as_array().or_else(|| schema["anyOf"].as_array()) {
		return values
			.iter()
			.map(|value| {
				if value.as_object().is_none_or(|object| {
					object.keys().any(|key| !["const", "title"].contains(&key.as_str()))
				}) {
					return Err("Unsupported choice constraints".into());
				}
				let text = value["const"].as_str().ok_or("Unsupported choice")?;
				Ok(McpFormChoice {
					label: value["title"].as_str().unwrap_or(text).into(),
					value: json!(text),
				})
			})
			.collect();
	}
	Ok(Vec::new())
}

/// Validate the request mode before interpreting its form schema.
/// Only standard MCP retains the legacy null-schema confirmation convention.
pub fn mcp_request_fields(request: &Value) -> Result<Vec<McpFormField>, String> {
	let schema = request.get("requestedSchema").ok_or("Missing form schema")?;
	match request["mode"].as_str() {
		Some("form") => mcp_form_fields(schema),
		Some("openai/form" | "openaiForm") if !schema.is_null() => mcp_form_fields(schema),
		_ => Err("This form schema is not supported.".into()),
	}
}

/// Project the standard primitive schema shared by MCP and supported OpenAI forms.
pub fn mcp_form_fields(schema: &Value) -> Result<Vec<McpFormField>, String> {
	if schema.is_null() {
		return Ok(Vec::new());
	}
	if schema.to_string().len() > crate::MAX_HISTORY_INLINE_BYTES || schema["type"] != "object" {
		return Err("This form schema is not supported.".into());
	}
	if schema.as_object().is_some_and(|object| {
		object.keys().any(|key| {
			![
				"$schema",
				"type",
				"properties",
				"required",
				"title",
				"description",
				"additionalProperties",
			]
			.contains(&key.as_str())
		})
	}) {
		return Err("This form uses unsupported schema constraints.".into());
	}
	let properties = schema["properties"].as_object().ok_or("Missing form fields")?;
	if properties.len() > 32 {
		return Err("This form has too many fields.".into());
	}
	let required = match schema.get("required") {
		None => Vec::new(),
		Some(value) => value
			.as_array()
			.ok_or("Invalid required fields")?
			.iter()
			.map(|value| {
				value
					.as_str()
					.filter(|id| properties.contains_key(*id))
					.ok_or("Invalid required field")
			})
			.collect::<Result<Vec<_>, _>>()?,
	};
	properties
		.iter()
		.map(|(id, property)| {
			if property.as_object().is_none_or(|object| {
				object.keys().any(|key| {
					![
						"type",
						"title",
						"description",
						"default",
						"format",
						"minLength",
						"maxLength",
						"minimum",
						"maximum",
						"enum",
						"enumNames",
						"oneOf",
						"anyOf",
						"items",
						"minItems",
						"maxItems",
					]
					.contains(&key.as_str())
				})
			}) {
				return Err("This field uses unsupported schema constraints.".into());
			}
			let kind = property["type"].as_str().ok_or("Missing field type")?;
			if !["string", "boolean", "number", "integer", "array"].contains(&kind)
				|| id.is_empty()
				|| id.len() > 512
			{
				return Err("Unsupported form field".into());
			}
			if kind != "string"
				&& kind != "array"
				&& ["enum", "oneOf", "anyOf"].iter().any(|key| property.get(key).is_some())
			{
				return Err("Unsupported field alternatives".into());
			}
			if kind == "array"
				&& property["items"].as_object().is_none_or(|object| {
					object
						.keys()
						.any(|key| !["type", "enum", "oneOf", "anyOf"].contains(&key.as_str()))
				}) {
				return Err("Unsupported item constraints".into());
			}
			let options = if kind == "boolean" {
				vec![
					McpFormChoice { label: "True".into(), value: json!(true) },
					McpFormChoice { label: "False".into(), value: json!(false) },
				]
			} else {
				choices(if kind == "array" { &property["items"] } else { property })?
			};
			if (kind == "string"
				&& (property.get("enum").is_some()
					|| property.get("oneOf").is_some()
					|| property.get("anyOf").is_some())
				&& options.is_empty())
				|| options.len() > 64
				|| (kind == "array"
					&& (property["items"]["type"] != "string" || options.is_empty()))
			{
				return Err("Unsupported selection field".into());
			}
			Ok(McpFormField {
				id: id.clone(),
				title: property["title"].as_str().unwrap_or(id).into(),
				description: property["description"].as_str().map(str::to_owned),
				required: required.contains(&id.as_str()),
				kind: kind.into(),
				choices: options,
				schema: property.clone(),
			})
		})
		.collect()
}

/// Convert explicit control values to typed form content, preserving false and empty arrays.
pub fn mcp_form_content(
	fields: &[McpFormField],
	answers: &BTreeMap<String, Value>,
) -> Result<Value, String> {
	let mut content = serde_json::Map::new();
	for field in fields {
		let Some(value) = answers.get(&field.id) else {
			if field.required {
				return Err(format!("{} is required.", field.title));
			}
			continue;
		};
		let valid = match field.kind.as_str() {
			"string" => value.is_string(),
			"boolean" => value.is_boolean(),
			"number" => value.is_number(),
			"integer" => value.as_i64().is_some() || value.as_u64().is_some(),
			"array" => value.is_array(),
			_ => false,
		};
		if !valid {
			return Err(format!("{} has an invalid value.", field.title));
		}
		if !field.choices.is_empty() {
			let selected: Vec<&Value> = if field.kind == "array" {
				value.as_array().expect("array value validated above").iter().collect()
			} else {
				vec![value]
			};
			if selected
				.iter()
				.any(|value| !field.choices.iter().any(|choice| &choice.value == *value))
			{
				return Err(format!("Choose an advertised value for {}.", field.title));
			}
			if field.kind == "array"
				&& selected
					.iter()
					.enumerate()
					.any(|(index, value)| selected[..index].contains(value))
			{
				return Err(format!("Remove duplicate choices for {}.", field.title));
			}
		}
		let count = value
			.as_str()
			.map(|text| text.chars().count() as u64)
			.or_else(|| value.as_array().map(|items| items.len() as u64));
		if let Some(count) = count {
			let (min, max) = if field.kind == "array" {
				("minItems", "maxItems")
			} else {
				("minLength", "maxLength")
			};
			if field.schema[min].as_u64().is_some_and(|min| count < min)
				|| field.schema[max].as_u64().is_some_and(|max| count > max)
			{
				return Err(format!("{} does not meet the required length.", field.title));
			}
		}
		if let Some(number) = value.as_f64()
			&& (field.schema["minimum"].as_f64().is_some_and(|min| number < min)
				|| field.schema["maximum"].as_f64().is_some_and(|max| number > max))
		{
			return Err(format!("{} is outside the allowed range.", field.title));
		}
		content.insert(field.id.clone(), value.clone());
	}
	if answers.keys().any(|id| !fields.iter().any(|field| &field.id == id)) {
		return Err("Unexpected form field".into());
	}
	let content = Value::Object(content);
	if content.to_string().len() > crate::MAX_HISTORY_INLINE_BYTES.saturating_sub(128) {
		return Err("Response is too large; shorten the answers.".into());
	}
	Ok(content)
}

/// Validate an MCP reply against its original request before consuming live response authority.
pub fn validate_mcp_response(request: &Value, response: &Value) -> Result<(), String> {
	let object = response.as_object().ok_or("Response must be an object")?;
	if object.keys().any(|key| !["action", "content", "_meta"].contains(&key.as_str())) {
		return Err("Unexpected response field".into());
	}
	let action = response["action"].as_str().ok_or("Missing elicitation action")?;
	if matches!(action, "decline" | "cancel") {
		if !response["content"].is_null() || !response["_meta"].is_null() {
			return Err("Decline and cancel cannot submit content or persistent permission".into());
		}
		return Ok(());
	}
	if action != "accept" {
		return Err("Unknown elicitation action".into());
	}
	match request["mode"].as_str() {
		Some("form" | "openai/form" | "openaiForm") => {
			let fields = mcp_request_fields(request)?;
			if fields.is_empty() {
				if !response["content"].is_null() && response["content"] != json!({}) {
					return Err("This approval has no input fields".into());
				}
			} else {
				let answers = response["content"]
					.as_object()
					.ok_or("Form content must be an object")?
					.iter()
					.map(|(key, value)| (key.clone(), value.clone()))
					.collect();
				mcp_form_content(&fields, &answers)?;
			}
			if !response["_meta"].is_null() {
				let meta = response["_meta"].as_object().ok_or("Invalid response metadata")?;
				let persist = meta
					.get("persist")
					.and_then(Value::as_str)
					.ok_or("Missing persistence choice")?;
				let offered = &request["_meta"]["persist"];
				if !fields.is_empty()
					|| request["_meta"]["codex_approval_kind"] == "tool_suggestion"
					|| meta.len() != 1
					|| !["session", "always"].contains(&persist)
					|| !(offered.as_str() == Some(persist)
						|| offered.as_array().is_some_and(|values| {
							values.iter().any(|value| value.as_str() == Some(persist))
						})) {
					return Err("This persistence scope was not offered".into());
				}
			}
		},
		Some("url") =>
			if !response["content"].is_null() || !response["_meta"].is_null() {
				return Err("URL confirmation cannot submit form content".into());
			},
		_ => return Err("This verification method cannot be accepted by this client".into()),
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn opaque_openai_schemas_never_become_empty_approvals() {
		for mode in ["form", "openai/form", "openaiForm"] {
			assert!(mcp_request_fields(&json!({"mode":mode})).is_err());
		}
		for mode in ["openai/form", "openaiForm"] {
			for schema in [Value::Null, json!(true), json!("unknown"), json!([]), json!({})] {
				let request = json!({"mode":mode,"requestedSchema":schema});
				assert!(mcp_request_fields(&request).is_err());
				assert!(
					validate_mcp_response(&request, &json!({"action":"accept","content":null}))
						.is_err()
				);
				for action in ["decline", "cancel"] {
					assert!(
						validate_mcp_response(&request, &json!({"action":action,"content":null}))
							.is_ok()
					);
				}
			}
		}
		assert!(
			mcp_request_fields(&json!({"mode":"form","requestedSchema":null})).unwrap().is_empty()
		);
	}

	#[test]
	fn openai_form_unknown_semantics_require_decline_or_cancel() {
		let request = json!({"mode":"openaiForm","requestedSchema":{
			"type":"object","properties":{"template":{"type":"string","oneOf":[{
				"const":"wire-value","title":"Display label","x-openai-preview":{"src":"data:image/png;base64,fixture"}
			}]}},"required":["template"]
		}});
		assert!(mcp_form_fields(&request["requestedSchema"]).is_err());
		assert!(
			validate_mcp_response(
				&request,
				&json!({"action":"accept","content":{"template":"wire-value"}})
			)
			.is_err()
		);
		for action in ["decline", "cancel"] {
			assert!(
				validate_mcp_response(&request, &json!({"action":action,"content":null})).is_ok()
			);
		}
	}

	#[test]
	fn response_permissions_are_limited_to_advertised_scope() {
		let request = json!({"mode":"form","requestedSchema":null,"_meta":{"persist":["session"]}});
		assert!(
			validate_mcp_response(
				&request,
				&json!({"action":"accept","content":null,"_meta":{"persist":"session"}})
			)
			.is_ok()
		);
		assert!(
			validate_mcp_response(
				&request,
				&json!({"action":"accept","content":null,"_meta":{"persist":"always"}})
			)
			.is_err()
		);
		assert!(
			validate_mcp_response(&request, &json!({"action":"cancel","content":{"hidden":true}}))
				.is_err()
		);
		assert!(
			validate_mcp_response(
				&json!({"mode":"openai/userVerification"}),
				&json!({"action":"accept","content":{}})
			)
			.is_err()
		);
		assert!(
			mcp_form_fields(
				&json!({"type":"object","properties":{"field":{"type":"string","pattern":"^allowed$"}}})
			)
			.is_err()
		);
	}

	#[test]
	fn explicit_choices_keep_types_and_never_submit_defaults() {
		let fields=mcp_form_fields(&json!({"type":"object","properties":{"agree":{"type":"boolean","default":true},"count":{"type":"integer","minimum":1},"tags":{"type":"array","items":{"type":"string","enum":["a","b"]},"minItems":1}},"required":["agree"]})).unwrap();
		assert!(mcp_form_content(&fields, &BTreeMap::new()).is_err());
		let mut answers = BTreeMap::from([
			("agree".into(), json!(false)),
			("count".into(), json!(2)),
			("tags".into(), json!(["b"])),
		]);
		assert_eq!(mcp_form_content(&fields, &answers).unwrap()["agree"], false);
		answers.insert("count".into(), json!(1.5));
		assert!(mcp_form_content(&fields, &answers).is_err());
		answers.insert("count".into(), json!(2));
		answers.insert("tags".into(), json!(["unoffered"]));
		assert!(mcp_form_content(&fields, &answers).is_err());
	}
}
