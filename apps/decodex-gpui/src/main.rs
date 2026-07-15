//! Default-disabled Decodex vNext GPUI client composition root.

use decodex_protocol::CURRENT_VERSION;

fn main() {
	let version = CURRENT_VERSION;

	println!(
		"Decodex GPUI v{}.{} is disabled: XY-1263 foundation accepted; P/K/L/S product slices remain disabled",
		version.major, version.minor
	);
}
