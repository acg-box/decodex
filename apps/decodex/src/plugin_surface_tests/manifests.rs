use crate::plugin_surface_tests::{self, DECODEX_PLUGIN_JSON, ROUTING_REF};

#[test]
fn packaged_plugin_manifest_is_decodex_runtime_only() {
	let decodex_surface = plugin_surface_tests::manifest_interface_surface(DECODEX_PLUGIN_JSON);
	let routing_surface = ROUTING_REF;

	plugin_surface_tests::assert_contains(&decodex_surface, "runtime ops");
	plugin_surface_tests::assert_contains(&decodex_surface, "planning");
	plugin_surface_tests::assert_contains(&decodex_surface, "commit");
	plugin_surface_tests::assert_contains(&decodex_surface, "landing");
	plugin_surface_tests::assert_not_contains(&decodex_surface, "bounded research");
	plugin_surface_tests::assert_not_contains(&decodex_surface, "OKF");
	plugin_surface_tests::assert_contains(routing_surface, "external installed");
	plugin_surface_tests::assert_not_contains(routing_surface, "deliberation:");
}
