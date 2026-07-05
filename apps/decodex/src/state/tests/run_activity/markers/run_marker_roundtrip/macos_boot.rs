use crate::state;

#[test]
fn macos_host_boot_id_uses_boot_session_uuid() {
	let host_boot_id = state::current_host_boot_id().expect("macOS boot session UUID should read");

	assert!(
		host_boot_id.starts_with("macos_bootsessionuuid:"),
		"macOS host boot identity should use boot-session UUID, got {host_boot_id}"
	);
	assert!(
		!host_boot_id.contains("boottime") && !host_boot_id.contains("usec"),
		"macOS host boot identity should not depend on kern.boottime timeval output"
	);
}
