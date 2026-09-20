//! Describe native non-text content without copying bytes, paths or signed URLs into history.
use decodex_protocol::{
	ChiefTimelineAttachment as Attachment, ChiefTimelineAttachmentSource as Source,
};
use serde_json::Value;

pub(super) fn project(item: &Value) -> (Vec<Attachment>, bool) {
	match item["type"].as_str() {
		Some("userMessage") => parts(&item["content"], "text", describe),
		Some("dynamicToolCall") if !item["contentItems"].is_null() =>
			parts(&item["contentItems"], "inputText", dynamic),
		Some("mcpToolCall") if !item["result"].is_null() =>
			parts(&item["result"]["content"], "text", mcp),
		Some("imageView") =>
			one(make(0, "imageView", "Image from execution environment", Source::Unknown)),
		Some("imageGeneration") => {
			if item["result"].as_str().is_some_and(|result| !result.is_empty()) {
				one(make(0, "imageGeneration", "Generated image", Source::Inline))
			} else if item["savedPath"].as_str().is_some_and(|path| !path.is_empty()) {
				one(make(0, "imageGeneration", "Generated image", Source::Unknown))
			} else {
				(vec![], false)
			}
		},
		_ => (vec![], false),
	}
}

fn parts(
	value: &Value,
	text_kind: &str,
	describe: fn(u32, &Value) -> (Attachment, bool),
) -> (Vec<Attachment>, bool) {
	let Some(parts) = value.as_array() else {
		return (vec![], true);
	};
	let mut attachments = Vec::new();
	let mut omitted = false;
	for (index, part) in parts.iter().enumerate().filter(|(_, part)| part["type"] != text_kind) {
		if attachments.len() == 16 {
			omitted = true;
			break;
		}
		let Ok(index) = u32::try_from(index) else {
			omitted = true;
			break;
		};
		let (attachment, shortened) = describe(index, part);
		attachments.push(attachment);
		omitted |= shortened;
	}
	(attachments, omitted)
}

fn uri_source(url: Option<&str>) -> Source {
	match url {
		Some(url) if url.starts_with("data:") => Source::Inline,
		Some(url) if url.starts_with("https://") || url.starts_with("http://") => Source::Remote,
		_ => Source::Unknown,
	}
}

fn dynamic(index: u32, part: &Value) -> (Attachment, bool) {
	match part["type"].as_str() {
		Some("inputImage") =>
			make(index, "inputImage", "Image", uri_source(part["imageUrl"].as_str())),
		Some("inputAudio") =>
			make(index, "inputAudio", "Audio", uri_source(part["audioUrl"].as_str())),
		_ => make(index, "unknown", "Unsupported attachment", Source::Unknown),
	}
}

fn mcp(index: u32, part: &Value) -> (Attachment, bool) {
	match part["type"].as_str() {
		Some(kind @ ("image" | "audio")) => {
			let present = part["data"].as_str().is_some_and(|data| !data.is_empty())
				&& part["mimeType"].as_str().is_some_and(|mime| !mime.is_empty());
			make(
				index,
				kind,
				if kind == "image" { "Image" } else { "Audio" },
				if present { Source::Inline } else { Source::Unknown },
			)
		},
		Some("resource_link") => make(
			index,
			"resource_link",
			part["name"].as_str().unwrap_or("Resource"),
			if part["uri"].as_str().is_some_and(|uri| !uri.is_empty()) {
				Source::Reference
			} else {
				Source::Unknown
			},
		),
		Some("resource") => {
			let resource = &part["resource"];
			let present = resource["uri"].as_str().is_some_and(|uri| !uri.is_empty())
				&& (resource["text"].is_string() || resource["blob"].is_string());
			make(
				index,
				"resource",
				"Embedded resource",
				if present { Source::Inline } else { Source::Unknown },
			)
		},
		_ => make(index, "unknown", "Unsupported attachment", Source::Unknown),
	}
}

fn one((attachment, omitted): (Attachment, bool)) -> (Vec<Attachment>, bool) {
	(vec![attachment], omitted)
}

fn describe(index: u32, part: &Value) -> (Attachment, bool) {
	match part["type"].as_str() {
		Some("localImage") => local(index, "localImage", part["path"].as_str(), "Image"),
		Some("localAudio") => local(index, "localAudio", part["path"].as_str(), "Audio"),
		Some("image")
			if part["url"].as_str().is_none()
				&& part["fileId"].as_str().is_some_and(|id| !id.is_empty()) =>
			make(index, "image", "Stored image", Source::Stored),
		Some(kind @ ("image" | "audio")) => {
			let source = uri_source(part["url"].as_str());
			make(index, kind, if kind == "image" { "Image" } else { "Audio" }, source)
		},
		Some(kind @ ("skill" | "mention")) =>
			make(index, kind, part["name"].as_str().unwrap_or(kind), Source::Reference),
		_ => make(index, "unknown", "Unsupported attachment", Source::Unknown),
	}
}

fn local(index: u32, kind: &str, path: Option<&str>, fallback: &str) -> (Attachment, bool) {
	let label = path
		.filter(|path| !path.is_empty())
		.and_then(|path| std::path::Path::new(path).file_name().and_then(|name| name.to_str()));
	make(
		index,
		kind,
		label.unwrap_or(fallback),
		if label.is_some() { Source::Local } else { Source::Unknown },
	)
}

fn make(index: u32, kind: &str, label: &str, source: Source) -> (Attachment, bool) {
	let sensitive = decodex_core::contains_credential_material(label);
	let shortened = label.len() > 256 || sensitive || source == Source::Unknown;
	let label = if sensitive {
		"Details omitted".into()
	} else {
		label[..label.floor_char_boundary(256.min(label.len()))]
			.chars()
			.map(|c| if c.is_control() { ' ' } else { c })
			.collect()
	};
	(Attachment { index, kind: kind.into(), label, source }, shortened)
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	#[test]
	fn native_parts_keep_source_indices_without_exposing_embedded_or_remote_data() {
		let item = json!({"type":"userMessage","content":[
			{"type":"text","text":"Question"}, {"type":"localImage","path":"/host/private/photo.png"},
			{"type":"image","url":"data:image/png;base64,PRIVATE_BYTES"}, {"type":"image","fileId":"private-file-id"},
			{"type":"audio","url":"https://example.test/audio?signature=PRIVATE_SIGNATURE"},
			{"type":"skill","name":"Read docs","path":"/private/SKILL.md"}, {"type":"mention","name":"Calendar","path":"app://private-id"}
		]});
		let (attachments, omitted) = project(&item);
		assert!(!omitted);
		assert_eq!(
			attachments.iter().map(|part| part.index).collect::<Vec<_>>(),
			vec![1, 2, 3, 4, 5, 6]
		);
		assert_eq!(attachments[0].label, "photo.png");
		assert_eq!(attachments[1].source, Source::Inline);
		assert_eq!(attachments[2].source, Source::Stored);
		assert_eq!(attachments[3].source, Source::Remote);
		let encoded = serde_json::to_string(&attachments).unwrap();
		for private in [
			"PRIVATE_BYTES",
			"PRIVATE_SIGNATURE",
			"private-file-id",
			"/host/private",
			"/private/SKILL.md",
			"app://private-id",
		] {
			assert!(!encoded.contains(private));
		}
	}
	#[test]
	fn dynamic_tool_media_preserves_native_indices_and_never_copies_payloads() {
		let (parts, omitted) = project(&json!({"type":"dynamicToolCall","contentItems":[
			{"type":"inputText","text":"Result"},
			{"type":"inputImage","imageUrl":"data:image/png;base64,PRIVATE"},
			{"type":"inputAudio","audioUrl":"https://example.test/?signature=PRIVATE"}
		]}));
		assert!(!omitted);
		assert_eq!(parts.iter().map(|part| part.index).collect::<Vec<_>>(), vec![1, 2]);
		assert_eq!((parts[0].kind.as_str(), parts[0].source), ("inputImage", Source::Inline));
		assert_eq!((parts[1].kind.as_str(), parts[1].source), ("inputAudio", Source::Remote));
		assert!(!serde_json::to_string(&parts).unwrap().contains("PRIVATE"));
		assert_eq!(
			project(&json!({"type":"dynamicToolCall","contentItems":null})),
			(vec![], false)
		);
		assert!(project(&json!({"type":"dynamicToolCall","contentItems":{}})).1);
		let (parts, omitted) = project(&json!({"type":"dynamicToolCall","contentItems":[
			{"type":"inputImage","imageUrl":null}, {"type":"futurePart"}
		]}));
		assert!(omitted && parts.iter().all(|part| part.source == Source::Unknown));
	}

	#[test]
	fn mcp_media_and_resources_are_described_without_raw_tool_output() {
		let (parts, omitted) = project(&json!({"type":"mcpToolCall","result":{"content":[
			{"type":"text","text":"Result"},
			{"type":"image","mimeType":"image/png","data":"PRIVATE"},
			{"type":"audio","mimeType":"audio/wav","data":"PRIVATE"},
			{"type":"resource","resource":{"uri":"file:///PRIVATE","text":"PRIVATE"}},
			{"type":"resource_link","uri":"https://example.test/PRIVATE","name":"Report"}
		],"structuredContent":{"private":"PRIVATE"},"_meta":{"private":"PRIVATE"}}}));
		assert!(!omitted);
		assert_eq!(parts.iter().map(|part| part.index).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
		assert!(parts[..3].iter().all(|part| part.source == Source::Inline));
		assert_eq!((parts[3].label.as_str(), parts[3].source), ("Report", Source::Reference));
		assert!(!serde_json::to_string(&parts).unwrap().contains("PRIVATE"));
		assert_eq!(project(&json!({"type":"mcpToolCall","result":null})), (vec![], false));
		let (parts, omitted) = project(&json!({"type":"mcpToolCall","result":{"content":[
			{"type":"image","data":"PRIVATE"}, {"type":"resource","resource":{}},
			{"type":"resource_link","name":"Link"}, {"type":"futureBlock"}
		]}}));
		assert!(omitted && parts.iter().all(|part| part.source == Source::Unknown));
	}

	#[test]
	fn bounds_unknown_parts_and_generated_images_are_explicit() {
		let (parts, omitted) = project(
			&json!({"type":"userMessage","content":[{"type":"futureContent","value":"private"},{"type":"localAudio","path":format!("/tmp/{}", "界".repeat(100))}]}),
		);
		assert!(omitted && parts[0].source == Source::Unknown);
		assert!(parts[1].label.len() <= 256 && parts[1].label.ends_with('界'));
		let (parts, omitted) = project(
			&json!({"type":"userMessage","content":vec![json!({"type":"image","fileId":"file"});17]}),
		);
		assert!(omitted);
		assert_eq!(parts.len(), 16);
		let (parts, _) = project(
			&json!({"type":"imageGeneration","result":"PRIVATE_BYTES","savedPath":"/tmp/result.png"}),
		);
		assert_eq!(parts[0].source, Source::Inline);
		assert_eq!(parts[0].label, "Generated image");
		let (parts, _) = project(&json!({"type":"imageGeneration","savedPath":"/tmp/result.png"}));
		assert_eq!(parts[0].source, Source::Unknown);
		let (parts, _) = project(&json!({"type":"imageGeneration","result":"PRIVATE_BYTES"}));
		assert_eq!(parts[0].source, Source::Inline);
	}
}
