//! Read native capabilities on the retained process. Never expose raw configuration.
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::{ChiefCapabilitiesResult, ChiefModelDto, ConversationModel};
use serde_json::{Value, json};

pub(crate) async fn read(client: &AppServerClient) -> ChiefCapabilitiesResult {
	tokio::time::timeout(std::time::Duration::from_secs(8), read_inner(client))
		.await
		.unwrap_or(ChiefCapabilitiesResult::Unavailable)
}

async fn read_inner(client: &AppServerClient) -> ChiefCapabilitiesResult {
	let mut models = Vec::new();
	let mut cursor = Value::Null;
	let mut seen = std::collections::HashSet::new();
	for _ in 0..8 {
		let Ok(page) = client
			.request("model/list", json!({"limit":100,"includeHidden":false,"cursor":cursor}))
			.await
		else {
			return ChiefCapabilitiesResult::Unavailable;
		};
		let Some(entries) = page["data"].as_array() else {
			return ChiefCapabilitiesResult::Unavailable;
		};
		for entry in entries {
			if entry["hidden"] == true {
				continue;
			}
			let Some(model) = project_model(entry) else {
				return ChiefCapabilitiesResult::Unavailable;
			};
			if models.iter().any(|known: &ChiefModelDto| known.model == model.model) {
				return ChiefCapabilitiesResult::Unavailable;
			}
			models.push(model);
			if models.len() > 100 {
				return ChiefCapabilitiesResult::Unavailable;
			}
		}
		cursor = page["nextCursor"].clone();
		if cursor.is_null() {
			let memory_enabled = memory_feature(client).await;
			return ChiefCapabilitiesResult::Available { models, memory_enabled };
		}
		let Some(next) = cursor.as_str() else {
			return ChiefCapabilitiesResult::Unavailable;
		};
		if next.len() > 4096 || !seen.insert(next.to_owned()) {
			return ChiefCapabilitiesResult::Unavailable;
		}
	}
	ChiefCapabilitiesResult::Unavailable
}

async fn memory_feature(client: &AppServerClient) -> Option<bool> {
	let mut cursor = Value::Null;
	let mut seen = std::collections::HashSet::new();
	for _ in 0..8 {
		let page = client
			.request("experimentalFeature/list", json!({"limit":100,"cursor":cursor}))
			.await
			.ok()?;
		if let Some(feature) =
			page["data"].as_array()?.iter().find(|feature| feature["name"] == "memories")
		{
			return feature["enabled"].as_bool();
		}
		cursor = page["nextCursor"].clone();
		let next = cursor.as_str()?;
		if next.len() > 4096 || !seen.insert(next.to_owned()) {
			return None;
		}
	}
	None
}

fn project_model(value: &Value) -> Option<ChiefModelDto> {
	let model = ConversationModel::new(value["model"].as_str()?).ok()?;
	let name = value["displayName"].as_str()?;
	if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
		return None;
	}
	let mut efforts = Vec::new();
	for level in value["supportedReasoningEfforts"].as_array()? {
		if let Ok(effort) = serde_json::from_value(level["reasoningEffort"].clone())
			&& !efforts.contains(&effort)
		{
			efforts.push(effort);
		}
	}
	let default_effort = serde_json::from_value(value["defaultReasoningEffort"].clone())
		.ok()
		.filter(|effort| efforts.contains(effort));
	let supports_fast = value["serviceTiers"]
		.as_array()
		.is_some_and(|tiers| tiers.iter().any(|tier| tier["id"] == "priority"))
		|| value["additionalSpeedTiers"]
			.as_array()
			.is_some_and(|tiers| tiers.iter().any(|tier| tier == "fast" || tier == "priority"));
	let supports_images = value["inputModalities"]
		.as_array()
		.is_none_or(|modes| modes.iter().any(|mode| mode == "image"));
	Some(ChiefModelDto {
		model,
		name: name.into(),
		efforts,
		default_effort,
		supports_fast,
		supports_images,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	#[tokio::test]
	async fn reads_all_pages_without_a_turn_and_projects_only_memory_flag() {
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		let (client_io, server_io) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(client_io);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(server_io);
			let mut lines = BufReader::new(reader).lines();
			for (index, method) in
				["model/list", "model/list", "experimentalFeature/list"].into_iter().enumerate()
			{
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				let result = if index == 2 {
					json!({"data":[{"name":"memories","enabled":true}],"private":"DO_NOT_PROJECT"})
				} else {
					assert_eq!(
						request["params"]["cursor"],
						if index == 0 { Value::Null } else { json!("page2") }
					);
					json!({"data":[{"model":format!("model-{index}"),"displayName":"Model","supportedReasoningEfforts":[{"reasoningEffort":"high"}],"defaultReasoningEffort":"high"}],"nextCursor":if index == 0 {json!("page2")} else {Value::Null}})
				};
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		let result = read(&client).await;
		server.await.unwrap();
		assert!(
			matches!(&result,ChiefCapabilitiesResult::Available { models,memory_enabled:Some(true) } if models.len()==2)
		);
		assert!(!serde_json::to_string(&result).unwrap().contains("DO_NOT_PROJECT"));
	}

	#[test]
	fn catalog_uses_advertised_values_not_model_name_guesses() {
		let value = json!({"model":"custom-model","displayName":"Custom","supportedReasoningEfforts":[{"reasoningEffort":"medium"},{"reasoningEffort":"future-level"}],"defaultReasoningEffort":"medium","inputModalities":["text"],"serviceTiers":[{"id":"priority"}]});
		let model = project_model(&value).expect("native model");
		assert_eq!(model.efforts, vec![decodex_protocol::ConversationReasoningEffort::Medium]);
		assert_eq!(model.default_effort, model.efforts.first().copied());
		assert!(model.supports_fast);
		assert!(!model.supports_images);
		let mut invalid = value;
		invalid["displayName"] = json!("invalid\nlabel");
		assert!(project_model(&invalid).is_none());
	}
}
