// swift-tools-version: 6.4

import PackageDescription

let package = Package(
	name: "DecodexMenuBar",
	platforms: [.macOS(.v27)],
	products: [
		.library(name: "DecodexMenuBar", type: .dynamic, targets: ["DecodexApp"]),
	],
	targets: [
		.target(name: "DecodexApp"),
		.testTarget(name: "DecodexAppTests", dependencies: ["DecodexApp"]),
	],
)
