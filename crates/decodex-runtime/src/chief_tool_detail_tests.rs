use super::*;

fn history(item: Value) -> Value {
	json!({"thread":{"id":"thread","turns":[{"id":"turn","items":[item]}]}})
}

#[test]
fn complete_mcp_results_survive_paging_with_resources_and_failure_tail() {
	let long = "界".repeat(12000);
	let item = json!({"id":"item","type":"mcpToolCall","server":"docs","tool":"read","status":"failed","result":{"content":[
		{"type":"text","text":long},
		{"type":"resource_link","uri":"https://example.test/result","name":"report","description":"Result report"},
		{"type":"resource","resource":{"uri":"file:///report.txt","mimeType":"text/plain","text":"embedded body"}},
		{"type":"resource","resource":{"uri":"file:///report.bin","blob":"RAW_BINARY_DO_NOT_RENDER"}},
		{"type":"image","data":"RAW_IMAGE_DO_NOT_RENDER","text":"Image caption"},
		{"type":"audio","data":"RAW_AUDIO_DO_NOT_RENDER"},
		{"type":"future","text":"Future result","value":42}
	]},"error":{"message":"Trailing failure diagnostic"}});
	let text = project_text(&history(item), "thread", "turn", "item").unwrap();
	let mut combined = String::new();
	let mut cursor = None;
	loop {
		let ChiefActivityDetailResult::Available { text, next, .. } =
			page(&text, "scope", cursor.as_ref()).unwrap()
		else {
			panic!("page")
		};
		combined.push_str(&text);
		if next.is_none() {
			break;
		}
		cursor = next;
	}
	assert_eq!(combined, text);
	assert!(combined.contains(&long));
	for expected in [
		"https://example.test/result",
		"Result report",
		"embedded body",
		"file:///report.bin",
		"Returned image",
		"Image caption",
		"Returned audio",
		"Future result",
		"Tool reported failure",
	] {
		assert!(combined.contains(expected), "{expected}");
	}
	assert!(combined.ends_with("Trailing failure diagnostic"));
	assert!(!combined.contains("DO_NOT_RENDER"));
}

#[test]
fn structured_only_result_and_public_siblings_remain_visible() {
	let item = json!({"id":"item","type":"mcpToolCall","tool":"read","result":{"content":[
		{"type":"text","text":"Bearer fixture-private-access-token-123456789"},
		{"type":"text","text":"Public result"}
	],"structuredContent":{"count":3,"items":["one","two","three"]},"_meta":{"private":"META_DO_NOT_RENDER"}}});
	let text = project_text(&history(item), "thread", "turn", "item").unwrap();
	assert!(text.contains("[Sensitive content omitted]"));
	assert!(text.contains("Public result"));
	assert!(text.contains("Structured result:"));
	assert!(text.contains("three"));
	assert!(!text.contains("DO_NOT_RENDER"));
	let item = json!({"id":"item","type":"mcpToolCall","result":{"content":[],"structuredContent":{"count":0}}});
	assert!(project_text(&history(item), "thread", "turn", "item").unwrap().contains("count"));
}

#[test]
fn dynamic_media_and_malformed_blocks_do_not_disappear() {
	let item = json!({"id":"item","type":"dynamicToolCall","namespace":"fixture","tool":"read","success":false,"contentItems":[
		{"type":"inputText","text":"Text result"},
		{"type":"inputImage","imageUrl":"data:image/png;base64,RAW_DO_NOT_RENDER"},
		{"type":"inputAudio","audioUrl":"data:audio/wav;base64,RAW_DO_NOT_RENDER"},
		{"type":"text","unexpected":"Malformed text result"}
	]});
	let text = project_text(&history(item), "thread", "turn", "item").unwrap();
	for expected in [
		"fixture",
		"Text result",
		"Returned image",
		"Returned audio",
		"Malformed text result",
		"Tool reported failure",
	] {
		assert!(text.contains(expected));
	}
	assert!(!text.contains("DO_NOT_RENDER"));
}

#[test]
fn standalone_result_details_retain_long_text_without_media_or_encrypted_bodies() {
	let long = "界".repeat(12000);
	let item = json!({"id":"item","type":"functionCallOutput","name":"result","namespace":"tools","output":[
	 {"type":"input_text","text":long},
	 {"type":"input_image","image_url":"data:image/png;base64,RAW_DO_NOT_RENDER"},
	 {"type":"encrypted_content","encrypted_content":"RAW_DO_NOT_RENDER"},
	 {"type":"input_text","text":"Bearer fixture-private-access-token-123456789"},
	 {"type":"input_text","text":"Public tail"}
	]});
	let text = project_text(&history(item.clone()), "thread", "turn", "item").unwrap();
	assert!(text.starts_with("tools/result"));
	assert!(text.contains(&long));
	assert!(text.ends_with("Public tail"));
	assert!(text.contains("[Sensitive content omitted]"));
	assert!(!text.contains("DO_NOT_RENDER"));
	let mut combined = String::new();
	let mut cursor = None;
	loop {
		let ChiefActivityDetailResult::Available { text: chunk, next, .. } =
			page(&text, "scope", cursor.as_ref()).unwrap()
		else {
			panic!("page")
		};
		assert!(chunk.len() <= 8192);
		combined.push_str(&chunk);
		if next.is_none() {
			break;
		}
		cursor = next;
	}
	assert_eq!(combined, text);
	let mut scalar = item;
	scalar["output"] = json!("Plain result");
	assert!(
		project_text(&history(scalar), "thread", "turn", "item").unwrap().ends_with("Plain result")
	);
}

#[test]
fn image_path_and_app_context_are_descriptive_native_evidence() {
	let item = json!({"id":"item","type":"imageView","path":"/remote/image.png"});
	let text = project_text(&history(item), "thread", "turn", "item").unwrap();
	assert!(text.contains("/remote/image.png"));
	assert!(text.contains("does not identify the executor"));
	let item = json!({"id":"item","type":"mcpToolCall","appContext":{
  "connectorId":"app-fixture","appName":"Calendar","actionName":"Read event",
  "linkId":"link-fixture","resourceUri":"ui://event","private":"RAW_DO_NOT_RENDER"
 },"result":{"content":[{"type":"text","text":"Event found"}]}});
	let text = project_text(&history(item.clone()), "thread", "turn", "item").unwrap();
	for expected in [
		"App: Calendar",
		"Action: Read event",
		"Connector: app-fixture",
		"Link: link-fixture",
		"Resource: ui://event",
		"Event found",
	] {
		assert!(text.contains(expected), "{expected}");
	}
	assert!(!text.contains("DO_NOT_RENDER"));
	let mut partial = item;
	partial["appContext"] = json!({"appName":"Calendar","linkId":false,"resourceUri":null});
	let text = project_text(&history(partial), "thread", "turn", "item").unwrap();
	assert!(text.contains("App: Calendar"));
	assert!(!text.contains("Link:"));
	assert!(!text.contains("Resource:"));
}
