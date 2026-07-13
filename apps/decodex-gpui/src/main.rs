//! Default-disabled Decodex vNext GPUI client composition root.

use decodex_protocol::ProtocolVersion;

fn main() {
	let version = ProtocolVersion::V1;

	println!(
		"Decodex GPUI v{}.{} is disabled: XY-1263 accessibility gate remains failed",
		version.major, version.minor
	);
}
