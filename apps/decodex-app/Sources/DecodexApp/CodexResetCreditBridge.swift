import Darwin
import Foundation

enum ResetCreditConsumeOutcome: String, Decodable, Sendable {
	case reset
	case nothingToReset
	case noCredit
	case alreadyRedeemed
}

enum CodexResetCreditBridgeError: LocalizedError {
	case launchFailed(String)
	case requestFailed(Int, String)
	case invalidResponse(String)
	case responseTimedOut
	case serverExited(Int32)
	case accountHomeMismatch
	case creditChanged

	var errorDescription: String? {
		switch self {
		case .launchFailed(let message):
			return "Could not start the Codex app server: \(message)"
		case .requestFailed(let code, let message):
			return "Codex app server request failed (\(code)): \(message)"
		case .invalidResponse(let message):
			return "Invalid Codex app server response: \(message)"
		case .responseTimedOut:
			return "The Codex app server did not respond in time."
		case .serverExited(let status):
			return "The Codex app server exited before it completed the reset request (status \(status))."
		case .accountHomeMismatch:
			return "The Codex app server did not use the isolated account session."
		case .creditChanged:
			return "The reset cards changed. Refresh and try again."
		}
	}
}

struct CodexResetCreditCredentials: Sendable {
	let expectedEmail: String?
}

struct CodexResetCreditConsumeResult: Sendable {
	let outcome: ResetCreditConsumeOutcome
	let creditID: String
}

struct CodexResetCreditUseFailure: LocalizedError, Sendable {
	let creditID: String
	let message: String

	var errorDescription: String? {
		message
	}
}

struct CodexResetCreditBridge: Sendable {
	private let environment: [String: String]
	private let responseTimeout: TimeInterval

	init(
		environment: [String: String] = ProcessInfo.processInfo.environment,
		responseTimeout: TimeInterval = 20
	) {
		self.environment = environment
		self.responseTimeout = responseTimeout
	}

	func consume(
		codexExecutableURL: URL,
		codexHomeURL: URL,
		credentials: CodexResetCreditCredentials,
		attempt: ResetCreditUseAttempt
	) async throws -> CodexResetCreditConsumeResult {
		try await Task.detached(priority: .userInitiated) {
			try consumeSynchronously(
				codexExecutableURL: codexExecutableURL,
				codexHomeURL: codexHomeURL,
				credentials: credentials,
				attempt: attempt
			)
		}
		.value
	}

	func resolveCreditID(
		for target: ResetCreditUseTarget,
		availableCount: Int,
		in credits: [CodexRateLimitResetCredit]
	) throws -> String {
		guard target.detailsComplete, target.descriptorMultiplicity == 1 else {
			throw CodexResetCreditBridgeError.creditChanged
		}

		let availableCredits = credits.filter { $0.status == "available" }
		guard availableCount == availableCredits.count else {
			throw CodexResetCreditBridgeError.creditChanged
		}

		let matchingCredits = availableCredits.filter { credit in
			credit.status == "available"
				&& credit.resetType == "codexRateLimits"
				&& credit.grantedAt == target.descriptor.grantedAtUnixEpoch
				&& credit.expiresAt == target.descriptor.expiresAtUnixEpoch
		}
		guard matchingCredits.count == 1 else {
			throw CodexResetCreditBridgeError.creditChanged
		}

		let creditID = matchingCredits[0].id
			.trimmingCharacters(in: .whitespacesAndNewlines)
		guard creditID.isEmpty == false else {
			throw CodexResetCreditBridgeError.invalidResponse(
				"the selected reset card has no identifier"
			)
		}

		return creditID
	}

	private func consumeSynchronously(
		codexExecutableURL: URL,
		codexHomeURL: URL,
		credentials: CodexResetCreditCredentials,
		attempt: ResetCreditUseAttempt
	) throws -> CodexResetCreditConsumeResult {
		let session = try makeSession(
			codexExecutableURL: codexExecutableURL,
			codexHomeURL: codexHomeURL,
			credentials: credentials
		)
		defer {
			session.stop()
		}

		try bootstrap(
			session: session,
			codexHomeURL: codexHomeURL,
			credentials: credentials
		)
		let creditID: String
		if let existingCreditID = normalizedCreditID(attempt.creditID) {
			creditID = existingCreditID
		} else {
			let rateLimits = try readRateLimits(session: session, requestID: 3)
			guard let resetCredits = rateLimits.rateLimitResetCredits,
				let credits = resetCredits.credits
			else {
				throw CodexResetCreditBridgeError.invalidResponse(
					"reset-card details are unavailable"
				)
			}

			creditID = try resolveCreditID(
				for: attempt.target,
				availableCount: resetCredits.availableCount,
				in: credits
			)
		}

		do {
			try session.sendRequest(
				id: 4,
				method: "account/rateLimitResetCredit/consume",
				params: [
					"creditId": creditID,
					"idempotencyKey": attempt.idempotencyKey,
				]
			)
			let response = try session.readResponse(
				id: 4,
				as: CodexConsumeResetCreditResponse.self
			)

			return CodexResetCreditConsumeResult(
				outcome: response.outcome,
				creditID: creditID
			)
		} catch {
			throw CodexResetCreditUseFailure(
				creditID: creditID,
				message: error.localizedDescription
			)
		}
	}

	private func normalizedCreditID(_ value: String?) -> String? {
		guard let value = value?.trimmingCharacters(in: .whitespacesAndNewlines),
			value.isEmpty == false
		else {
			return nil
		}

		return value
	}

	private func makeSession(
		codexExecutableURL: URL,
		codexHomeURL: URL,
		credentials: CodexResetCreditCredentials
	) throws -> CodexAppServerSession {
		try CodexAppServerSession(
			executableURL: codexExecutableURL,
			codexHomeURL: codexHomeURL,
			environment: environment,
			responseTimeout: responseTimeout
		)
	}

	private func bootstrap(
		session: CodexAppServerSession,
		codexHomeURL: URL,
		credentials: CodexResetCreditCredentials
	) throws {
		try session.sendRequest(
			id: 1,
			method: "initialize",
			params: [
				"clientInfo": [
					"name": "decodex-app",
					"version": "0.2.0",
				],
			]
		)
		let initialize = try session.readResponse(id: 1, as: CodexInitializeResponse.self)
		guard pathsReferToSameLocation(initialize.codexHome, codexHomeURL.path) else {
			throw CodexResetCreditBridgeError.accountHomeMismatch
		}

		try session.sendNotification(method: "initialized")
		try session.sendRequest(
			id: 2,
			method: "account/read",
			params: [
				"refreshToken": false,
			]
		)
		let account = try session.readResponse(id: 2, as: CodexAccountReadResponse.self)
		guard account.account?.type == "chatgpt" else {
			throw CodexResetCreditBridgeError.invalidResponse(
				"the isolated account does not match the selected account"
			)
		}
		if let expectedEmail = normalizedEmail(credentials.expectedEmail),
			normalizedEmail(account.account?.email) != expectedEmail
		{
			throw CodexResetCreditBridgeError.invalidResponse(
				"the isolated account does not match the selected account"
			)
		}
	}

	private func normalizedEmail(_ value: String?) -> String? {
		guard let email = value?
			.trimmingCharacters(in: .whitespacesAndNewlines)
			.lowercased(),
			email.isEmpty == false
		else {
			return nil
		}

		return email
	}

	private func readRateLimits(
		session: CodexAppServerSession,
		requestID: Int
	) throws -> CodexRateLimitsResponse {
		try session.sendRequest(
			id: requestID,
			method: "account/rateLimits/read"
		)
		return try session.readResponse(id: requestID, as: CodexRateLimitsResponse.self)
	}

	private func pathsReferToSameLocation(_ left: String, _ right: String) -> Bool {
		URL(fileURLWithPath: left)
			.standardizedFileURL
			.resolvingSymlinksInPath()
			== URL(fileURLWithPath: right)
				.standardizedFileURL
				.resolvingSymlinksInPath()
	}
}

struct CodexRateLimitResetCredit: Decodable, Equatable, Sendable {
	let id: String
	let grantedAt: Int
	let expiresAt: Int?
	let resetType: String
	let status: String
}

private struct CodexRateLimitResetCreditsSummary: Decodable {
	let availableCount: Int
	let credits: [CodexRateLimitResetCredit]?
}

private struct CodexRateLimitsResponse: Decodable {
	let rateLimitResetCredits: CodexRateLimitResetCreditsSummary?
}

private struct CodexInitializeResponse: Decodable {
	let codexHome: String
}

private struct CodexConsumeResetCreditResponse: Decodable {
	let outcome: ResetCreditConsumeOutcome
}

private struct CodexAccountReadResponse: Decodable {
	let account: Account?

	struct Account: Decodable {
		let type: String
		let email: String?
	}
}

private final class CodexAppServerSession {
	private static let maximumBufferedOutputBytes = 1_048_576
	private let process = Process()
	private let inputPipe = Pipe()
	private let outputPipe = Pipe()
	private let responseTimeout: TimeInterval
	private var bufferedOutput = Data()
	private var didStop = false

	init(
		executableURL: URL,
		codexHomeURL: URL,
		environment: [String: String],
		responseTimeout: TimeInterval
	) throws {
		self.responseTimeout = responseTimeout

		var processEnvironment = environment
		processEnvironment["CODEX_HOME"] = codexHomeURL.path
		processEnvironment["CODEX_SQLITE_HOME"] = codexHomeURL.path
		processEnvironment.removeValue(forKey: "CODEX_ACCESS_TOKEN")
		processEnvironment.removeValue(forKey: "OPENAI_API_KEY")

		process.executableURL = executableURL
		process.arguments = [
			"app-server",
			"--stdio",
			"-c",
			"cli_auth_credentials_store=\"file\"",
		]
		process.environment = processEnvironment
		process.currentDirectoryURL = codexHomeURL
		process.standardInput = inputPipe
		process.standardOutput = outputPipe
		process.standardError = FileHandle.nullDevice

		do {
			try process.run()
		} catch {
			throw CodexResetCreditBridgeError.launchFailed(error.localizedDescription)
		}
	}

	deinit {
		stop()
	}

	func sendRequest(id: Int, method: String, params: Any) throws {
		try send([
			"jsonrpc": "2.0",
			"id": id,
			"method": method,
			"params": params,
		])
	}

	func sendRequest(id: Int, method: String) throws {
		try send([
			"jsonrpc": "2.0",
			"id": id,
			"method": method,
		])
	}

	func sendNotification(method: String) throws {
		try send([
			"jsonrpc": "2.0",
			"method": method,
		])
	}

	func readResponse<Response: Decodable>(
		id: Int,
		as type: Response.Type
	) throws -> Response {
		let deadline = Date().addingTimeInterval(responseTimeout)

		while true {
			let message = try readMessage(deadline: deadline)
			guard messageID(message["id"]) == id else {
				if message["id"] != nil, let method = message["method"] as? String {
					throw CodexResetCreditBridgeError.invalidResponse(
						"unsupported server request \(method)"
					)
				}
				continue
			}

			if let error = message["error"] as? [String: Any] {
				let code = (error["code"] as? NSNumber)?.intValue ?? -1
				let message = error["message"] as? String ?? "unknown error"
				throw CodexResetCreditBridgeError.requestFailed(code, message)
			}
			guard let result = message["result"] else {
				throw CodexResetCreditBridgeError.invalidResponse(
					"request \(id) has no result"
				)
			}

			do {
				let data = try JSONSerialization.data(
					withJSONObject: result,
					options: [.fragmentsAllowed]
				)
				return try JSONDecoder().decode(type, from: data)
			} catch let error as CodexResetCreditBridgeError {
				throw error
			} catch {
				throw CodexResetCreditBridgeError.invalidResponse(
					"request \(id) could not be decoded: \(error.localizedDescription)"
				)
			}
		}
	}

	func stop() {
		guard didStop == false else {
			return
		}
		didStop = true

		try? inputPipe.fileHandleForWriting.close()
		guard process.isRunning else {
			return
		}

		process.terminate()
		let deadline = Date().addingTimeInterval(1)
		while process.isRunning, Date() < deadline {
			usleep(10_000)
		}
		if process.isRunning {
			kill(process.processIdentifier, SIGKILL)
		}
		process.waitUntilExit()
	}

	private func send(_ message: [String: Any]) throws {
		do {
			var data = try JSONSerialization.data(withJSONObject: message)
			data.append(0x0a)
			try inputPipe.fileHandleForWriting.write(contentsOf: data)
		} catch {
			throw CodexResetCreditBridgeError.invalidResponse(
				"could not write a request: \(error.localizedDescription)"
			)
		}
	}

	private func readMessage(deadline: Date) throws -> [String: Any] {
		while true {
			if let newlineIndex = bufferedOutput.firstIndex(of: 0x0a) {
				let line = bufferedOutput[..<newlineIndex]
				bufferedOutput.removeSubrange(...newlineIndex)
				if line.isEmpty {
					continue
				}

				do {
					let object = try JSONSerialization.jsonObject(with: Data(line))
					guard let message = object as? [String: Any] else {
						throw CodexResetCreditBridgeError.invalidResponse(
							"app-server output is not a JSON object"
						)
					}
					return message
				} catch let error as CodexResetCreditBridgeError {
					throw error
				} catch {
					throw CodexResetCreditBridgeError.invalidResponse(
						"app-server output is not valid JSON: \(error.localizedDescription)"
					)
				}
			}

			let remainingMilliseconds = Int32(
				max(0, min(Double(Int32.max), ceil(deadline.timeIntervalSinceNow * 1_000)))
			)
			guard remainingMilliseconds > 0 else {
				throw CodexResetCreditBridgeError.responseTimedOut
			}

			var descriptor = pollfd(
				fd: outputPipe.fileHandleForReading.fileDescriptor,
				events: Int16(POLLIN | POLLHUP),
				revents: 0
			)
			let result = Darwin.poll(&descriptor, 1, remainingMilliseconds)
			if result == 0 {
				throw CodexResetCreditBridgeError.responseTimedOut
			}
			if result < 0 {
				if errno == EINTR {
					continue
				}
				throw CodexResetCreditBridgeError.invalidResponse(
					"could not read app-server output"
				)
			}
			if descriptor.revents & Int16(POLLERR | POLLNVAL) != 0 {
				throw CodexResetCreditBridgeError.invalidResponse(
					"the app-server output stream failed"
				)
			}

			let data = outputPipe.fileHandleForReading.availableData
			guard data.isEmpty == false else {
				let status = process.isRunning ? -1 : process.terminationStatus
				throw CodexResetCreditBridgeError.serverExited(status)
			}
			bufferedOutput.append(data)
			guard bufferedOutput.count <= Self.maximumBufferedOutputBytes else {
				throw CodexResetCreditBridgeError.invalidResponse(
					"the app-server response is too large"
				)
			}
		}
	}

	private func messageID(_ value: Any?) -> Int? {
		if let number = value as? NSNumber {
			return number.intValue
		}
		if let string = value as? String {
			return Int(string)
		}

		return nil
	}
}
