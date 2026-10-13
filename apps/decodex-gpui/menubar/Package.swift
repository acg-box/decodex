// swift-tools-version: 6.4

import PackageDescription

let package = Package(
	name: "DecodexMenuBar",
	platforms: [.macOS(.v27)],
	products: [
		.library(name: "DecodexMenuBar", type: .dynamic, targets: ["DecodexApp", "DecodexTransport"]),
	],
	dependencies: [
		.package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.10.0"),
	],
	targets: [
		.target(name: "DecodexApp", dependencies: [.product(name: "Sparkle", package: "Sparkle")]),
        .target(name: "DecodexTransport"),
		.testTarget(name: "DecodexAppTests", dependencies: ["DecodexApp", "DecodexTransport"]),
	],
)
