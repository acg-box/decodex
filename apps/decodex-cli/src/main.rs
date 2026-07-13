//! Default-unavailable Decodex vNext command-line client composition root.

use decodex_protocol::ProtocolVersion;

fn main() {
	let version = ProtocolVersion::V1;

	println!(
		"decodex v{}.{} client unavailable: API transport belongs to XY-1266/XY-1268",
		version.major, version.minor
	);
}
