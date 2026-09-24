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
	let mut catalog = ModelCatalogPages::default();
	let mut cursor = Value::Null;
	for _ in 0..8 {
		let Ok(page) = client
			.request("model/list", json!({"limit":100,"includeHidden":false,"cursor":cursor}))
			.await
		else {
			return ChiefCapabilitiesResult::Unavailable;
		};
		match catalog.push(&page) {
			Ok(Some(next)) => cursor = json!(next),
			Ok(None) =>
				return ChiefCapabilitiesResult::Available {
					models: catalog.models,
					memory_enabled: memory_feature(client).await,
				},
			Err(()) => return ChiefCapabilitiesResult::Unavailable,
		}
	}
	ChiefCapabilitiesResult::Unavailable
}

/// Shared model-page projection for retained Chief and ordinary process transports.
#[derive(Default)]
pub(crate) struct ModelCatalogPages {
	pub models: Vec<ChiefModelDto>,
	seen: std::collections::HashSet<String>,
	pages: usize,
	complete: bool,
}

impl ModelCatalogPages {
	/// Keep incomplete catalogs unavailable, including repeated cursors and identities.
	pub fn push(&mut self, page: &Value) -> Result<Option<String>, ()> {
		if self.complete || self.pages >= 8 {
			return Err(());
		}
		self.pages += 1;
		let entries = page["data"].as_array().filter(|entries| entries.len() <= 100).ok_or(())?;
		for entry in entries {
			if entry["hidden"] == true {
				continue;
			}
			let model = project_model(entry).ok_or(())?;
			if self.models.len() >= 100
				|| self.models.iter().any(|known| known.model == model.model)
			{
				return Err(());
			}
			self.models.push(model);
		}
		if page["nextCursor"].is_null() {
			self.complete = true;
			return Ok(None);
		}
		let next = page["nextCursor"]
			.as_str()
			.filter(|next| !next.is_empty() && next.len() <= 4096)
			.ok_or(())?;
		if !self.seen.insert(next.into()) {
			return Err(());
		}
		Ok(Some(next.into()))
	}
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
	let mut service_tiers = Vec::new();
	if !value["serviceTiers"].is_null() {
		let tiers = value["serviceTiers"].as_array().filter(|tiers| tiers.len() <= 32)?;
		for tier in tiers {
			let id = decodex_core::ServiceTier::new(tier["id"].as_str()?).ok()?;
			if service_tiers
				.iter()
				.any(|known: &decodex_protocol::ChiefServiceTierDto| known.id == id)
			{
				return None;
			}
			let name = tier["name"].as_str().unwrap_or(id.as_str());
			let description = tier["description"].as_str().unwrap_or("");
			if name.is_empty()
				|| name.len() > 256
				|| name.chars().any(char::is_control)
				|| description.len() > 2048
				|| description.chars().any(|c| c.is_control() && c != '\n')
			{
				return None;
			}
			service_tiers.push(decodex_protocol::ChiefServiceTierDto {
				name: name.into(),
				description: description.into(),
				id,
			});
		}
	}
	if service_tiers.is_empty()
		&& value["additionalSpeedTiers"]
			.as_array()
			.is_some_and(|tiers| tiers.iter().any(|tier| tier == "fast" || tier == "priority"))
	{
		service_tiers.push(decodex_protocol::ChiefServiceTierDto {
			id: decodex_core::ServiceTier::from_fast(true),
			name: "Fast".into(),
			description: String::new(),
		});
	}
	let default_service_tier = if value["defaultServiceTier"].is_null() {
		None
	} else {
		Some(decodex_core::ServiceTier::new(value["defaultServiceTier"].as_str()?).ok()?)
	};
	let supports_fast = service_tiers.iter().any(|tier| tier.id.as_str() == "priority");
	let supports_images = value["inputModalities"]
		.as_array()
		.is_none_or(|modes| modes.iter().any(|mode| mode == "image"));
	let notice = |text: &Value| {
		text.as_str()
			.filter(|text| !text.trim().is_empty() && text.len() <= 4096)
			.map(str::to_owned)
	};
	let upgrade_model = value
		.pointer("/upgradeInfo/model")
		.and_then(Value::as_str)
		.or_else(|| value["upgrade"].as_str())
		.and_then(|model| ConversationModel::new(model).ok());
	let upgrade = upgrade_model.map(|model| decodex_protocol::ChiefModelUpgradeDto {
		model,
		notice: notice(&value["upgradeInfo"]["upgradeCopy"]),
		retirement_at: value["upgradeInfo"]["retirementAt"].as_i64(),
	});
	Some(ChiefModelDto {
		model,
		name: name.into(),
		efforts,
		default_effort,
		supports_fast,
		service_tiers,
		default_service_tier,
		supports_images,
		availability: notice(&value["availabilityNux"]["message"]),
		upgrade,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn advertised_persistent_effort_retains_native_value() {
		let value = json!({"model":"custom","displayName":"Custom","supportedReasoningEfforts":[{"reasoningEffort":"persistent"}],"defaultReasoningEffort":"persistent"});
		let model = project_model(&value).expect("advertised model");
		assert_eq!(model.efforts[0].as_str(), "persistent");
		assert_eq!(model.default_effort, Some(model.efforts[0]));
		let mut value = value;
		value["supportedReasoningEfforts"] = json!([{"reasoningEffort":"high"}]);
		let model = project_model(&value).expect("model without persistent support");
		assert_eq!(model.efforts.len(), 1);
		assert_eq!(model.efforts[0].as_str(), "high");
		assert_eq!(model.default_effort, None, "an unadvertised default is not selected");
	}

	#[test]
	fn shared_catalog_rejects_partial_repeated_and_oversized_pages() {
		let model = json!({"model":"custom","displayName":"Custom","supportedReasoningEfforts":[],"defaultReasoningEffort":"high"});
		let mut pages = ModelCatalogPages::default();
		assert_eq!(
			pages.push(&json!({"data":[model.clone()],"nextCursor":"next"})),
			Ok(Some("next".into()))
		);
		assert!(pages.push(&json!({"data":[model.clone()],"nextCursor":null})).is_err());
		let mut pages = ModelCatalogPages::default();
		assert!(pages.push(&json!({"data":[],"nextCursor":"next"})).is_ok());
		assert!(pages.push(&json!({"data":[],"nextCursor":"next"})).is_err());
		let mut pages = ModelCatalogPages::default();
		assert!(pages.push(&json!({"data":vec![model.clone();101],"nextCursor":null})).is_err());
		let mut pages = ModelCatalogPages::default();
		let mut hidden = model.clone();
		hidden["hidden"] = json!(true);
		assert_eq!(pages.push(&json!({"data":[hidden,model],"nextCursor":null})), Ok(None));
		assert_eq!(pages.models.len(), 1);
		assert!(pages.push(&json!({"data":[],"nextCursor":null})).is_err());
	}

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
	fn catalog_exposes_bounded_upgrade_notices_without_changing_selected_model() {
		let mut value = json!({"model":"old","displayName":"Old","supportedReasoningEfforts":[{"reasoningEffort":"high"}],"defaultReasoningEffort":"high","upgradeInfo":{"model":"new","upgradeCopy":"New model available","retirementAt":1800000000},"availabilityNux":{"message":"Available for this account"}});
		let model = project_model(&value).unwrap();
		assert_eq!(model.model.as_str(), "old");
		assert_eq!(model.upgrade.as_ref().unwrap().model.as_str(), "new");
		assert_eq!(model.upgrade.unwrap().retirement_at, Some(1800000000));
		assert_eq!(model.availability.as_deref(), Some("Available for this account"));
		value["availabilityNux"]["message"] = json!("x".repeat(4097));
		value["upgradeInfo"] = Value::Null;
		value["upgrade"] = json!("fallback");
		let model = project_model(&value).unwrap();
		assert!(model.availability.is_none());
		assert_eq!(model.upgrade.unwrap().model.as_str(), "fallback");
	}

	#[test]
	fn catalog_preserves_named_service_tiers_and_default_without_selecting_them() {
		let mut value = json!({"model":"custom","displayName":"Custom","supportedReasoningEfforts":[],"defaultReasoningEffort":"high","serviceTiers":[{"id":"priority","name":"Fast","description":"Increased usage"},{"id":"ultrafast","name":"Ultrafast","description":"Latency-sensitive work"}],"defaultServiceTier":"ultrafast"});
		let projected = project_model(&value).unwrap();
		assert_eq!(projected.service_tiers.len(), 2);
		assert_eq!(projected.service_tiers[1].id.as_str(), "ultrafast");
		assert_eq!(projected.service_tiers[1].description, "Latency-sensitive work");
		assert_eq!(projected.default_service_tier.unwrap().as_str(), "ultrafast");
		value["serviceTiers"][1]["id"] = json!("future-tier");
		assert_eq!(project_model(&value).unwrap().service_tiers[1].id.as_str(), "future-tier");
		value["serviceTiers"][1]["id"] = json!("priority");
		assert!(project_model(&value).is_none(), "duplicate tier identities are ambiguous");
		value["serviceTiers"] = json!([]);
		value["defaultServiceTier"] = json!("flex");
		let projected = project_model(&value).unwrap();
		assert_eq!(projected.default_service_tier.unwrap().as_str(), "flex");
		assert!(projected.service_tiers.is_empty(), "a default is not an advertised selection");
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
