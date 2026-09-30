//! Link Objective-C categories required by the native WebRTC static archive.

fn main() {
	if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
		// libwebrtc uses Objective-C categories from its static archive (Apple QA1490).
		println!("cargo:rustc-link-arg=-Wl,-ObjC");
	}
}
