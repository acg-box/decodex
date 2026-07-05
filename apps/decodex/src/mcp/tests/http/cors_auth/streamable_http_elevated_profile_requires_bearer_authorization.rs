use crate::mcp::{McpCapabilityProfile, McpHttpAuthorization, http};

#[test]
fn streamable_http_elevated_profile_requires_bearer_authorization() {
	assert!(
		http::validate_mcp_http_capability_profile(
			McpCapabilityProfile::Observe,
			&McpHttpAuthorization::disabled()
		)
		.is_ok()
	);

	for profile in
		[McpCapabilityProfile::Plan, McpCapabilityProfile::Operate, McpCapabilityProfile::Admin]
	{
		assert!(
			http::validate_mcp_http_capability_profile(profile, &McpHttpAuthorization::disabled())
				.is_err()
		);
		assert!(
			http::validate_mcp_http_capability_profile(
				profile,
				&McpHttpAuthorization::from_token_for_test("secret-token")
			)
			.is_ok()
		);
	}
}
