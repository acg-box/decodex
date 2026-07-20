// swift-tools-version: 6.4

import PackageDescription

let package = Package(
	name: "DecodexApp",
	platforms: [.macOS(.v27)],
	products: [
		.executable(name: "DecodexApp", targets: ["DecodexApp"]),
	],
	targets: [
		.executableTarget(name: "DecodexApp"),
		.testTarget(name: "DecodexAppTests", dependencies: ["DecodexApp"]),
	],
)
