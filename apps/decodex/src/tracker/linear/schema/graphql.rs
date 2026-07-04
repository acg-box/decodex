use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
pub(in crate::tracker::linear) struct GraphqlRequest<'a, V> {
	pub(in crate::tracker::linear) query: &'a str,
	pub(in crate::tracker::linear) variables: V,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct GraphqlResponse<T> {
	pub(in crate::tracker::linear) data: Option<T>,
	pub(in crate::tracker::linear) errors: Option<Vec<GraphqlError>>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct GraphqlError {
	pub(in crate::tracker::linear) message: String,
	pub(in crate::tracker::linear) extensions: Option<Value>,
}
