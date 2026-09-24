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
