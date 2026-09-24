//! Display standalone tool input without promoting it to user authority.
use serde_json::Value;

pub(super) fn text(item: &Value) -> Option<String> {
	let name = item["name"].as_str().filter(|name| !name.is_empty())?;
	let title = if item["namespace"].is_null() {
		name.to_owned()
	} else {
		let namespace = item["namespace"].as_str()?;
		if namespace.is_empty() { name.to_owned() } else { format!("{namespace}/{name}") }
	};
	let output = match &item["output"] {
		Value::String(text) => text.clone(),
		Value::Array(parts) => parts
			.iter()
			.filter(|part| part["type"] == "input_text")
			.map(|part| part["text"].as_str())
			.collect::<Option<Vec<_>>>()?
			.join("\n"),
		_ => return None,
	};
	Some(format!("{title}\n{output}"))
}

#[cfg(test)]
mod tests {
	use decodex_protocol::ChiefTimelineContent;
	use serde_json::json;

	#[test]
	fn standalone_outputs_keep_authority_bounds_and_structured_media_indices() {
		let mut row = json!({"type":"item","position":1,"turnId":"turn","item":{
			"type":"functionCallOutput","id":"output","name":"work_instruction",
			"namespace":"decodex","output":"Inspect the delegated task."
		}});
		let ChiefTimelineContent::Item { kind, text, .. } = super::super::ordinary(&row).unwrap()
		else {
			panic!("item")
		};
		assert_eq!(kind, "functionCallOutput");
		assert_eq!(text, "decodex/work_instruction\nInspect the delegated task.");
		for namespace in [json!(null), json!("")] {
			let mut unnamed = row.clone();
			unnamed["item"]["namespace"] = namespace;
			assert!(
				matches!(super::super::ordinary(&unnamed).unwrap(), ChiefTimelineContent::Item {text,..} if text.starts_with("work_instruction\n"))
			);
		}
		row["item"]["output"] = json!([
			{"type":"input_text","text":"Visible"},
			{"type":"input_image","image_url":"data:image/png;base64,PRIVATE_IMAGE"},
			{"type":"input_audio","audio_url":"https://example.invalid/private-audio"},
			{"type":"encrypted_content","encrypted_content":"PRIVATE_ENCRYPTED"}
		]);
		let projected = super::super::ordinary(&row).unwrap();
		let ChiefTimelineContent::Item { text, attachments, .. } = &projected else {
			panic!("item")
		};
		assert_eq!(text, "decodex/work_instruction\nVisible");
		assert_eq!(attachments.iter().map(|a| a.index).collect::<Vec<_>>(), vec![1, 2, 3]);
		assert_eq!(attachments[0].kind, "inputImage");
		assert_eq!(attachments[1].kind, "inputAudio");
		assert!(!serde_json::to_string(&projected).unwrap().contains("PRIVATE_"));
		assert!(!serde_json::to_string(&projected).unwrap().contains("private-audio"));
		for output in ["界".repeat(4000), "Bearer abcdefgh".into()] {
			row["item"]["output"] = json!(output);
			assert!(
				matches!(super::super::ordinary(&row).unwrap(), ChiefTimelineContent::Item {text,truncated:true,..} if text.len()<=8192)
			);
		}
		row["item"]["output"] = json!([{"type":"input_text","text":false}]);
		assert!(super::super::ordinary(&row).is_none());
	}
}
