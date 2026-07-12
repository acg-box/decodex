use serde::Deserialize;

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct WorkspaceIdentityData {
	pub(in crate::tracker::linear) viewer: ImmutableIdentity,
	pub(in crate::tracker::linear) organization: ImmutableIdentity,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct ImmutableIdentity {
	pub(in crate::tracker::linear) id: String,
}

#[cfg(test)]
mod tests {
	use super::WorkspaceIdentityData;

	#[test]
	fn workspace_identity_uses_only_immutable_provider_ids() {
		let data = serde_json::from_str::<WorkspaceIdentityData>(
			r#"{"viewer":{"id":"account-1"},"organization":{"id":"workspace-1"}}"#,
		)
		.expect("identity response");

		assert_eq!(data.viewer.id, "account-1");
		assert_eq!(data.organization.id, "workspace-1");
	}
}
