//! Execute the Chrome boundary checks without installing Node or using a live account.

#[test]
#[cfg(target_os = "macos")]
fn chrome_extension_protocol_and_scope() {
	let output = std::process::Command::new("/usr/bin/swift")
		.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/extension_harness.swift"))
		.arg(env!("CARGO_MANIFEST_DIR"))
		.arg(decodex_pro_bridge::native_host::EXTENSION_ID)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}\n{}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);
}
