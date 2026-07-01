use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DynamicToolSpec {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) namespace: Option<String>,
	pub(crate) description: String,
	#[serde(rename = "deferLoading", default, skip_serializing_if = "std::ops::Not::not")]
	pub(crate) defer_loading: bool,
	#[serde(rename = "inputSchema")]
	pub(crate) input_schema: Value,
	pub(crate) name: String,
}
impl DynamicToolSpec {
	pub(crate) fn new(
		name: impl Into<String>,
		description: impl Into<String>,
		input_schema: Value,
	) -> Self {
		Self {
			namespace: None,
			description: description.into(),
			defer_loading: false,
			input_schema,
			name: name.into(),
		}
	}

	pub(crate) fn deferred(mut self) -> Self {
		self.defer_loading = true;

		self
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DynamicToolCallResponse {
	#[serde(rename = "contentItems")]
	pub(crate) content_items: Vec<DynamicToolContentItem>,
	pub(crate) success: bool,
}
impl DynamicToolCallResponse {
	pub(crate) fn success(message: String) -> Self {
		Self { content_items: vec![DynamicToolContentItem::text(message)], success: true }
	}

	pub(crate) fn failure(message: String) -> Self {
		Self { content_items: vec![DynamicToolContentItem::text(message)], success: false }
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type")]
pub(crate) enum DynamicToolContentItem {
	#[serde(rename = "inputText")]
	InputText { text: String },
}
impl DynamicToolContentItem {
	fn text(text: String) -> Self {
		Self::InputText { text }
	}
}
