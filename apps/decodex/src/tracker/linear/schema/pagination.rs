use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub(in crate::tracker::linear) struct PageInfo {
	#[serde(rename = "hasNextPage")]
	pub(in crate::tracker::linear) has_next_page: bool,
	#[serde(rename = "endCursor")]
	pub(in crate::tracker::linear) end_cursor: Option<String>,
}
