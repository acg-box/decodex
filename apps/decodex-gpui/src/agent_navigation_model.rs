//! Local navigation preferences. Service and native conversation identities remain authoritative.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Section {
	Pin,
	Assistant,
	Projects,
}
impl Section {
	pub const ALL: [Self; 3] = [Self::Pin, Self::Assistant, Self::Projects];

	pub fn key(self) -> &'static str {
		match self {
			Self::Pin => "pin",
			Self::Assistant => "assistant",
			Self::Projects => "projects",
		}
	}

	pub fn label(self) -> &'static str {
		match self {
			Self::Pin => "Pin",
			Self::Assistant => "Assistant",
			Self::Projects => "Projects",
		}
	}
}
#[derive(Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Sort {
	#[default]
	Updated,
	Alphabetical,
}
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub(super) struct Preference {
	pub sort: Sort,
	pub auto_scroll: bool,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
	#[serde(deserialize_with = "read_sections")]
	pub sections: BTreeMap<Section, Preference>,
	pub pinned: BTreeSet<String>,
	pub project_drafts: BTreeMap<String, BTreeMap<String, String>>,
}

fn read_sections<'de, D>(reader: D) -> Result<BTreeMap<Section, Preference>, D::Error>
where
	D: serde::Deserializer<'de>,
{
	let saved = BTreeMap::<String, Preference>::deserialize(reader)?;
	Ok(saved
		.into_iter()
		.filter_map(|(key, value)| {
			Section::ALL
				.into_iter()
				.find(|section| section.key() == key)
				.map(|section| (section, value))
		})
		.collect())
}
impl Preferences {
	pub fn load() -> Self {
		crate::ui_preferences::string("DecodexNavigation", None)
			.and_then(|value| serde_json::from_str(&value).ok())
			.unwrap_or_default()
	}

	pub fn save(&self) {
		if let Ok(value) = serde_json::to_string(self) {
			crate::ui_preferences::string("DecodexNavigation", Some(&value));
		}
	}

	pub fn section(&self, section: Section) -> Preference {
		self.sections.get(&section).copied().unwrap_or(Preference {
			sort: if section == Section::Projects { Sort::Alphabetical } else { Sort::Updated },
			auto_scroll: false,
		})
	}
}
#[derive(Clone)]
pub(in crate::shell::agent_surface) struct Item {
	pub id: String,
	pub title: String,
	pub owner: String,
	pub project: Option<String>,
	pub updated: i64,
	pub draft: bool,
	pub assistant: bool,
	pub archived: bool,
	pub state: String,
}
pub(super) fn sort(items: &mut [Item], by: Sort) {
	items.sort_by(|a, b| {
		b.draft.cmp(&a.draft).then_with(|| match by {
			Sort::Updated => b
				.updated
				.cmp(&a.updated)
				.then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
				.then(a.id.cmp(&b.id)),
			Sort::Alphabetical => a
				.title
				.to_lowercase()
				.cmp(&b.title.to_lowercase())
				.then(b.updated.cmp(&a.updated))
				.then(a.id.cmp(&b.id)),
		})
	});
}
pub(super) fn matches(query: &str, text: &str) -> bool {
	let text = text.to_lowercase();
	query.split_whitespace().all(|word| text.contains(word))
}
#[cfg(test)]
mod tests {
	#[test]
	fn preferences_round_trip_each_section_without_changing_pins() {
		let mut value = super::Preferences::default();
		value.pinned.insert("topic".into());
		value.sections.insert(
			super::Section::Pin,
			super::Preference { sort: super::Sort::Alphabetical, auto_scroll: true },
		);
		let saved = serde_json::to_string(&value).unwrap();
		let loaded: super::Preferences = serde_json::from_str(&saved).unwrap();
		assert!(loaded.pinned.contains("topic"));
		assert!(loaded.section(super::Section::Pin).auto_scroll);
		assert!(!loaded.section(super::Section::Assistant).auto_scroll);
	}

	#[test]
	fn retired_recent_preferences_do_not_erase_pins_or_other_sections() {
		let saved = r#"{"sections":{"recent":{"sort":"alphabetical","auto_scroll":true},"pin":{"sort":"alphabetical","auto_scroll":true},"projects":{"sort":"updated","auto_scroll":false}},"pinned":["topic"],"project_drafts":{"profile":{"project":"draft"}}}"#;
		let loaded: super::Preferences = serde_json::from_str(saved).unwrap();
		assert!(loaded.pinned.contains("topic"));
		assert!(loaded.section(super::Section::Pin).auto_scroll);
		assert!(matches!(loaded.section(super::Section::Projects).sort, super::Sort::Updated));
		assert!(matches!(loaded.section(super::Section::Assistant).sort, super::Sort::Updated));
		assert_eq!(loaded.project_drafts["profile"]["project"], "draft");
		assert!(!serde_json::to_string(&loaded).unwrap().contains("recent"));
	}
}
