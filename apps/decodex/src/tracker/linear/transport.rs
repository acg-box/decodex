use color_eyre::Report;
use reqwest::Error;
use serde_json::Value;

use crate::{prelude::eyre, tracker::linear::schema::GraphqlError};

pub(super) fn linear_transport_error(error: Error) -> Report {
	if error.is_timeout() {
		eyre::eyre!("Linear connector timed out during GraphQL request: {error}")
	} else {
		Report::new(error)
	}
}

pub(super) fn rate_limited_error_message(errors: &[GraphqlError]) -> Option<String> {
	errors.iter().find_map(|error| {
		let extensions = error.extensions.as_ref()?;
		let code = extensions.get("code").and_then(Value::as_str)?;

		if code != "RATELIMITED" {
			return None;
		}

		let user_message = extensions
			.get("userPresentableMessage")
			.and_then(Value::as_str)
			.unwrap_or(error.message.as_str());
		let reset = extensions.get("reset").and_then(Value::as_i64);

		Some(match reset {
			Some(reset) => {
				format!("Linear connector is rate limited until `{reset}`: {user_message}")
			},
			None => format!("Linear connector is rate limited: {user_message}"),
		})
	})
}
