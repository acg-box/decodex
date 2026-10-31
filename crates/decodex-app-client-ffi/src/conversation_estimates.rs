//! Menu-bar presentation of the existing source-bound conversation estimates.
use std::time::Duration;

use decodex_protocol::{AgentClient, AgentSnapshotResult, AgentUsageEstimateResult, ClientProfile};
use serde::Serialize;
use serde_json::Value;

use super::{RequestFailure, to_value};

#[derive(Serialize)]
struct Row {
	work_id: String,
	title: String,
	account_id: Option<String>,
	usd_micros: Option<u64>,
	native_usd_micros: Option<u64>,
	credits_micros: Option<u64>,
	observed_at_micros: Option<i64>,
}

pub(super) async fn read(profile: ClientProfile) -> Result<Value, RequestFailure> {
	let client = AgentClient::new(profile.clone());
	let AgentSnapshotResult::Available(snapshot) =
		client.query().await.map_err(RequestFailure::Client)?
	else {
		return Ok(serde_json::json!({"available":false,"rows":[],"more_available":false}));
	};
	if snapshot.runtime_source.is_none() {
		return Ok(serde_json::json!({"available":false,"rows":[],"more_available":false}));
	}
	let mut work: Vec<_> = snapshot
		.work_items
		.iter()
		.filter(|work| {
			work.codex_thread_id.is_some()
				&& snapshot.agents.iter().any(|agent| agent.id == work.id)
		})
		.collect();
	work.sort_by(|a, b| {
		b.updated_at_micros.cmp(&a.updated_at_micros).then_with(|| a.id.cmp(&b.id))
	});
	let more_available = work.len() > 8;
	let mut reads = tokio::task::JoinSet::new();
	for (index, work) in work.into_iter().take(8).enumerate() {
		let profile = profile.clone();
		let (id, title) = (work.id.clone(), work.title.clone());
		reads.spawn(async move {
			let result = tokio::time::timeout(
				Duration::from_secs(20),
				AgentClient::new(profile).usage_estimate(
					decodex_protocol::EntityId::new(&id).expect("validated snapshot work"),
				),
			)
			.await;
			let mut row = Row {
				work_id: id,
				title,
				account_id: None,
				usd_micros: None,
				native_usd_micros: None,
				credits_micros: None,
				observed_at_micros: None,
			};
			if let Ok(Ok(AgentUsageEstimateResult::Available {
				account_id,
				estimate,
				observed_at_micros,
				..
			})) = result
			{
				row.account_id = Some(account_id.as_str().to_owned());
				row.usd_micros = estimate.estimated_usage_usd_micros;
				row.native_usd_micros = estimate.native_usage_usd_micros;
				row.credits_micros = Some(estimate.estimated_usage_credits_micros);
				row.observed_at_micros = Some(observed_at_micros);
			}
			(index, row)
		});
	}
	let mut rows = Vec::new();
	while let Some(result) = reads.join_next().await {
		let Ok(row) = result else {
			return Ok(serde_json::json!({"available":false,"rows":[],"more_available":false}));
		};
		rows.push(row);
	}
	let AgentSnapshotResult::Available(after) =
		client.query().await.map_err(RequestFailure::Client)?
	else {
		return Ok(serde_json::json!({"available":false,"rows":[],"more_available":false}));
	};
	if after.runtime_source != snapshot.runtime_source
		|| rows.iter().any(|(_, row)| {
			let original = snapshot
				.work_items
				.iter()
				.find(|work| work.id == row.work_id)
				.and_then(|work| work.codex_thread_id.as_ref());
			after
				.work_items
				.iter()
				.find(|work| work.id == row.work_id)
				.and_then(|work| work.codex_thread_id.as_ref())
				!= original
		}) {
		return Ok(serde_json::json!({"available":false,"rows":[],"more_available":false}));
	}
	rows.sort_by_key(|(index, _)| *index);
	to_value(
		serde_json::json!({"available":true,"rows":rows.into_iter().map(|(_, row)| row).collect::<Vec<_>>(),"more_available":more_available}),
	)
}
