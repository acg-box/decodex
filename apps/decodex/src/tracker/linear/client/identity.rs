use serde::Serialize;

use crate::{
	prelude::Result,
	tracker::{
		TrackerCredentialAttestation,
		linear::{LinearClient, schema::WorkspaceIdentityData},
	},
};

const WORKSPACE_IDENTITY_QUERY: &str = r#"
query DecodexWorkspaceIdentity {
  viewer { id }
  organization { id }
}
"#;

#[derive(Serialize)]
struct NoVariables {}

impl LinearClient {
	/// Introspect a host credential before any project configuration may consume it.
	pub(crate) fn introspect_workspace_identity(
		&self,
		credential_ref: &str,
	) -> Result<TrackerCredentialAttestation> {
		let data =
			self.post::<_, WorkspaceIdentityData>(WORKSPACE_IDENTITY_QUERY, &NoVariables {})?;

		TrackerCredentialAttestation::linear(
			credential_ref,
			&data.viewer.id,
			&data.organization.id,
			"linear_graphql_authenticated_identity_v1",
		)
	}
}
