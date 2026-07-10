use crate::plugin_surface_tests::{self, DECODEX_PLUGIN_JSON};

#[test]
fn packaged_plugin_manifest_is_decodex_runtime_only() {
	let decodex_surface = plugin_surface_tests::manifest_interface_surface(DECODEX_PLUGIN_JSON);

	plugin_surface_tests::assert_contains(&decodex_surface, "runtime operations");
	plugin_surface_tests::assert_contains(&decodex_surface, "planning");
	plugin_surface_tests::assert_contains(&decodex_surface, "commit");
	plugin_surface_tests::assert_contains(&decodex_surface, "landing");
}
