//! Local navigation preferences. Service and native conversation identities remain authoritative.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Section {
	Pin,
	Recent,
	Assistant,
	Projects,
}
impl Section {
	pub fn key(self) -> &'static str {
		match self {
			Self::Pin => "pin",
			Self::Recent => "recent",
			Self::Assistant => "assistant",
			Self::Projects => "projects",
		}
	}

	pub fn label(self) -> &'static str {
		match self {
			Self::Pin => "Pin",
			Self::Recent => "Recent",
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
	pub sections: BTreeMap<Section, Preference>,
	pub pinned: BTreeSet<String>,
	pub project_drafts: BTreeMap<String, BTreeMap<String, String>>,
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
pub(super) fn relative_time(micros: i64, now: i64) -> String {
	if micros <= 0 {
		return "—".into();
	}
	let seconds = now.saturating_sub(micros).max(0) / 1_000_000;
	for (unit, suffix) in [
		(31_536_000, "y"),
		(2_592_000, "m"),
		(604_800, "w"),
		(86_400, "d"),
		(3_600, "h"),
		(60, "min"),
	] {
		if seconds >= unit {
			return format!("{}{suffix}", seconds / unit);
		}
	}
	"now".into()
}
pub(super) fn now() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |d| d.as_micros().min(i64::MAX as u128) as i64)
}
pub(super) fn matches(query: &str, text: &str) -> bool {
	let text = text.to_lowercase();
	query.split_whitespace().all(|word| text.contains(word))
}
#[cfg(test)]
mod tests {
	#[test]
	fn relative_units_keep_minutes_and_months_distinct() {
		let now = 40_000_000_000_000;
		for (seconds, expected) in [
			(0, "now"),
			(60, "1min"),
			(3_600, "1h"),
			(86_400, "1d"),
			(604_800, "1w"),
			(2_592_000, "1m"),
			(31_536_000, "1y"),
		] {
			assert_eq!(super::relative_time(now - seconds * 1_000_000, now), expected);
		}
		assert_eq!(super::relative_time(0, now), "—");
	}
	#[test]
	fn preferences_round_trip_each_section_without_changing_pins() {
		let mut value = super::Preferences::default();
		value.pinned.insert("topic".into());
		value.sections.insert(
			super::Section::Recent,
			super::Preference { sort: super::Sort::Alphabetical, auto_scroll: true },
		);
		let saved = serde_json::to_string(&value).unwrap();
		let loaded: super::Preferences = serde_json::from_str(&saved).unwrap();
		assert!(loaded.pinned.contains("topic"));
		assert!(loaded.section(super::Section::Recent).auto_scroll);
		assert!(!loaded.section(super::Section::Assistant).auto_scroll);
	}
}
