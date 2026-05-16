// swift-tools-version: 6.0

import PackageDescription

let package = Package(
	name: "DecodexApp",
	platforms: [.macOS(.v14)],
	products: [
		.executable(name: "DecodexApp", targets: ["DecodexApp"]),
	],
	targets: [
		.executableTarget(name: "DecodexApp"),
	],
)
