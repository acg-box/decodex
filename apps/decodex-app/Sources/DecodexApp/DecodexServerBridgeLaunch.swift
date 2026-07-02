import Foundation

extension DecodexServerBridge {
	func startBundledServer() throws {
		if let startedProcess, startedProcess.isRunning {
			return
		}

		let process = Process()
		let nullDevice = FileHandle(forWritingAtPath: "/dev/null")

		process.executableURL = try decodexExecutableURL()
		process.arguments = Self.bundledServerArguments(listenAddress: defaultListenAddress)
		process.standardOutput = nullDevice
		process.standardError = nullDevice

		do {
			try process.run()
		} catch {
			throw DecodexAppBridgeError.launchFailed(error.localizedDescription)
		}

		startedProcess = process
	}

	private func decodexExecutableURL() throws -> URL {
		if let override = ProcessInfo.processInfo.environment["DECODEX_APP_DECODEX"], override.isEmpty == false {
			let overrideURL = URL(fileURLWithPath: override)
			if FileManager.default.isExecutableFile(atPath: overrideURL.path) {
				return overrideURL
			}
		}

		let bundledURL = Bundle.main.bundleURL
			.appendingPathComponent("Contents")
			.appendingPathComponent("Helpers")
			.appendingPathComponent("decodex")
		if FileManager.default.isExecutableFile(atPath: bundledURL.path) {
			return bundledURL
		}

		throw DecodexAppBridgeError.helperMissing(
			"Bundled decodex server is missing. Rebuild the app bundle with apps/decodex-app/script/build_and_run.sh."
		)
	}
}
