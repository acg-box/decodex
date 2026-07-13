//! Default-unavailable Decodex vNext command-line client composition root.

use decodex_protocol::CURRENT_VERSION;

fn main() {
	let version = CURRENT_VERSION;

	println!(
		"decodex v{}.{} client unavailable: client transport belongs to XY-1268",
		version.major, version.minor
	);
}
