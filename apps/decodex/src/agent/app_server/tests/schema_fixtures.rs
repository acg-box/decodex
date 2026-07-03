use std::{fs, path::Path};

use crate::agent::app_server::{
	APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS, APP_SERVER_REQUIRED_CLIENT_REQUESTS,
	APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS, APP_SERVER_REQUIRED_SERVER_REQUESTS,
};

pub(super) fn write_app_server_method_union_fixtures(root: &Path, omitted: Option<(&str, &str)>) {
	for (title, required_methods) in [
		("ClientRequest", APP_SERVER_REQUIRED_CLIENT_REQUESTS),
		("ServerRequest", APP_SERVER_REQUIRED_SERVER_REQUESTS),
		("ClientNotification", APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS),
		("ServerNotification", APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS),
	] {
		let branches = required_methods
			.iter()
			.filter(|(method, _schema)| omitted != Some((title, *method)))
			.map(|(method, schema)| {
				let mut properties = serde_json::json!({
					"method": {
						"type": "string",
						"enum": [method]
					}
				});

				if !schema.is_empty() {
					properties["params"] = serde_json::json!({
						"$ref": format!("#/definitions/{schema}")
					});
				}

				serde_json::json!({
					"title": format!("{method}Fixture"),
					"type": "object",
					"properties": properties
				})
			})
			.collect::<Vec<_>>();

		fs::write(
			root.join(format!("{title}.json")),
			serde_json::json!({
				"title": title,
				"oneOf": branches
			})
			.to_string(),
		)
		.expect("schema union fixture should write");
	}
}
