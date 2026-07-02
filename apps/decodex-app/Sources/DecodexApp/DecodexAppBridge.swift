import Foundation

struct DecodexAppBridge: Sendable {
	func runJSON<T: Decodable & Sendable>(_ request: AppBridgeRequest, as type: T.Type) async throws -> T {
		try await runStreaming(request, as: type, onOutput: nil)
	}

	func runStreaming<T: Decodable & Sendable>(
		_ request: AppBridgeRequest,
		as type: T.Type,
		onOutput: (@MainActor @Sendable (String) -> Void)?
	) async throws -> T {
		if onOutput == nil, try request.serverRoute() != nil {
			return try await DecodexServerBridge.shared.run(request, as: type)
		}
		guard request.requiresHelper else {
			throw DecodexAppBridgeError.invalidResponse(
				"operation \(request.operation) must be served by Decodex server"
			)
		}

		let helperURL = try helperExecutableURL()
		let requestData = try JSONEncoder().encode(request)

		return try await Task.detached(priority: .userInitiated) {
			let process = Process()
			let inputPipe = Pipe()
			let outputPipe = Pipe()
			let errorPipe = Pipe()
			let parser = AppBridgeEventParser<T>(onOutput: onOutput)

			process.executableURL = helperURL
			process.standardInput = inputPipe
			process.standardOutput = outputPipe
			process.standardError = errorPipe

			do {
				try process.run()
			} catch {
				throw DecodexAppBridgeError.launchFailed(error.localizedDescription)
			}

			inputPipe.fileHandleForWriting.write(requestData)
			inputPipe.fileHandleForWriting.write(Data([0x0a]))
			try inputPipe.fileHandleForWriting.close()

			while true {
				let data = outputPipe.fileHandleForReading.availableData
				if data.isEmpty {
					break
				}

				try parser.append(data)
			}

			process.waitUntilExit()

			let stderr = String(
				decoding: errorPipe.fileHandleForReading.readDataToEndOfFile(),
				as: UTF8.self
			)

			if process.terminationStatus != 0 {
				do {
					return try parser.finish()
				} catch DecodexAppBridgeError.commandFailed(let code, let message) {
					throw DecodexAppBridgeError.commandFailed(code, message)
				} catch {
					throw DecodexAppBridgeError.commandFailed(
						process.terminationStatus,
						stderr.isEmpty ? error.localizedDescription : stderr
					)
				}
			}

			return try parser.finish()
		}
		.value
	}

	private func helperExecutableURL() throws -> URL {
		if let override = ProcessInfo.processInfo.environment["DECODEX_APP_HELPER"], override.isEmpty == false {
			let overrideURL = URL(fileURLWithPath: override)
			if FileManager.default.isExecutableFile(atPath: overrideURL.path) {
				return overrideURL
			}
		}

		let bundledURL = Bundle.main.bundleURL
			.appendingPathComponent("Contents")
			.appendingPathComponent("Helpers")
			.appendingPathComponent("decodex-app-helper")
		if FileManager.default.isExecutableFile(atPath: bundledURL.path) {
			return bundledURL
		}

		throw DecodexAppBridgeError.helperMissing(
			"Bundled Decodex App helper is missing. Rebuild the app bundle with apps/decodex-app/script/build_and_run.sh."
		)
	}
}
