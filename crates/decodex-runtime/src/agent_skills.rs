//! Project enabled native skills for explicit use; never install or enable candidates.
use std::{future::Future, path::Path, time::Duration};

use serde_json::{Value, json};
use tokio::time;

use crate::agent_usage_estimate::Source;
use decodex_protocol::{
	AgentSkillDto, AgentSkillsPage, AgentSkillsResult, AgentSkillsTarget,
	ConversationWorkingDirectory, EntityId, WireText,
};

pub(crate) fn project(value: &Value, cwd: &str, filter: &str) -> Option<AgentSkillsPage> {
	let entries = value["data"].as_array()?;
	let entry = entries.iter().find(|entry| entry["cwd"].as_str() == Some(cwd))?;
	let errors = u32::try_from(entry["errors"].as_array()?.len()).ok()?;
	let query = filter.trim().to_lowercase();
	let mut skills = Vec::new();

	for item in entry["skills"].as_array()? {
		if item["enabled"] != true {
			continue;
		}

		let name = item["name"].as_str()?;
		let path = item["path"].as_str()?;
		let description = item
			.pointer("/interface/shortDescription")
			.and_then(Value::as_str)
			.or_else(|| item["shortDescription"].as_str())
			.or_else(|| item["description"].as_str())?;

		if name.trim().is_empty()
			|| name.chars().any(char::is_control)
			|| !Path::new(path).is_absolute()
		{
			continue;
		}
		if !query.is_empty()
			&& !name.to_lowercase().contains(&query)
			&& !description.to_lowercase().contains(&query)
		{
			continue;
		}

		skills.push(AgentSkillDto {
			name: WireText::new(name).ok()?,
			path: ConversationWorkingDirectory::new(path).ok()?,
			description: WireText::new(
				description.chars().filter(|c| !c.is_control()).take(300).collect::<String>(),
			)
			.ok()?,
		});
	}

	skills.sort_by(|a, b| {
		(a.name.as_str(), a.path.as_str()).cmp(&(b.name.as_str(), b.path.as_str()))
	});
	skills.dedup_by(|a, b| a.name == b.name && a.path == b.path);

	let mut truncated = skills.len() > 50;

	skills.truncate(50);

	while serde_json::to_vec(&skills).ok()?.len() > 100 * 1_024 {
		skills.pop()?;

		truncated = true;
	}

	Some(AgentSkillsPage { skills, truncated, errors })
}

pub(crate) async fn read<F, Fut>(source: F, filter: &str) -> AgentSkillsResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else { return AgentSkillsResult::Unavailable };
	let observed = time::timeout(Duration::from_secs(20), async {
		let native = before.client.thread_read(json!({"threadId":before.key.thread})).await.ok()?;

		if native["thread"]["id"] != before.key.thread {
			return None;
		}

		let cwd = native["thread"]["cwd"].as_str()?;
		let value = before
			.client
			.request("skills/list", json!({"cwds":[cwd],"forceReload":true}))
			.await
			.ok()?;

		project(&value, cwd, filter)
	})
	.await
	.ok()
	.flatten();

	if source().await.is_none_or(|after| {
		after.key != before.key
			|| after.client.connection_identity() != before.client.connection_identity()
	}) {
		return AgentSkillsResult::Unavailable;
	}

	match observed {
		Some(page) => AgentSkillsResult::Available {
			target: AgentSkillsTarget::Existing {
				work_id: EntityId::new(before.key.work).expect("validated work identity"),
			},
			page,
		},
		None => AgentSkillsResult::Unavailable,
	}
}

#[cfg(test)]
mod tests {
	use crate::agent_skills::{self};
	#[test]
	fn skills_filter_full_inventory_before_bounding_and_keep_exact_paths() {
		let mut skills:Vec<_>=(0..60).map(|index|agent_skills::json!({"name":format!("skill-{index:02}"),"path":format!("/skills (local)/{index}/SKILL.md"),"description":"Fixture skill","enabled":true})).collect();

		skills.push(agent_skills::json!({"name":"disabled","path":"/skills/disabled/SKILL.md","description":"Fixture","enabled":false}));

		let response = agent_skills::json!({"data":[{"cwd":"/project","skills":skills,"errors":[{"message":"not projected"}]}]});
		let page = agent_skills::project(&response, "/project", "").unwrap();

		assert_eq!(page.skills.len(), 50);
		assert!(page.truncated);
		assert_eq!(page.errors, 1);

		let page = agent_skills::project(&response, "/project", "SKILL-59").unwrap();

		assert_eq!(page.skills.len(), 1);
		assert!(!page.truncated);
		assert_eq!(page.skills[0].path.as_str(), "/skills (local)/59/SKILL.md");
		assert!(agent_skills::project(&response, "/other", "").is_none());
		assert!(
			agent_skills::project(&response, "/project", "disabled").unwrap().skills.is_empty()
		);
	}
}
