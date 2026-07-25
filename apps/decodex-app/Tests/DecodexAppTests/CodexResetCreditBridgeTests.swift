@testable import DecodexApp
import Foundation
import XCTest

final class CodexResetCreditBridgeTests: XCTestCase {
	func testResolveCreditIDUsesTheUniqueCompleteCurrentCredit() throws {
		let credits = [
			makeCredit(id: "credit-a", grantedAt: 100, expiresAt: 300),
			makeCredit(id: "credit-b", grantedAt: 100, expiresAt: 200),
		]
		let target = makeTarget()

		XCTAssertEqual(
			try CodexResetCreditBridge().resolveCreditID(
				for: target,
				availableCount: 2,
				in: credits
			),
			"credit-b"
		)
	}

	func testResolveCreditIDRejectsAChangedCardList() {
		XCTAssertThrowsError(
			try CodexResetCreditBridge().resolveCreditID(
				for: makeTarget(),
				availableCount: 1,
				in: [makeCredit(id: "other", grantedAt: 100, expiresAt: 300)]
			)
		) { error in
			XCTAssertEqual(
				error.localizedDescription,
				"The reset cards changed. Refresh and try again."
			)
		}
	}

	func testResolveCreditIDRejectsAnOriginallyAmbiguousDescriptor() {
		XCTAssertThrowsError(
			try CodexResetCreditBridge().resolveCreditID(
				for: makeTarget(descriptorMultiplicity: 2),
				availableCount: 1,
				in: [makeCredit(id: "credit-b", grantedAt: 100, expiresAt: 200)]
			)
		)
	}

	func testResolveCreditIDRejectsANewDuplicateDescriptor() {
		XCTAssertThrowsError(
			try CodexResetCreditBridge().resolveCreditID(
				for: makeTarget(),
				availableCount: 2,
				in: [
					makeCredit(id: "credit-a", grantedAt: 100, expiresAt: 200),
					makeCredit(id: "credit-b", grantedAt: 100, expiresAt: 200),
				]
			)
		)
	}

	func testResolveCreditIDRejectsIncompletePresentedDetails() {
		XCTAssertThrowsError(
			try CodexResetCreditBridge().resolveCreditID(
				for: makeTarget(detailsComplete: false),
				availableCount: 1,
				in: [makeCredit(id: "credit-b", grantedAt: 100, expiresAt: 200)]
			)
		)
	}

	func testResolveCreditIDRejectsIncompleteFreshDetails() {
		XCTAssertThrowsError(
			try CodexResetCreditBridge().resolveCreditID(
				for: makeTarget(),
				availableCount: 2,
				in: [makeCredit(id: "credit-b", grantedAt: 100, expiresAt: 200)]
			)
		)
	}

	func testResolveCreditIDRejectsAnUnsupportedResetType() {
		XCTAssertThrowsError(
			try CodexResetCreditBridge().resolveCreditID(
				for: makeTarget(),
				availableCount: 1,
				in: [
					makeCredit(
						id: "credit-b",
						grantedAt: 100,
						expiresAt: 200,
						resetType: "unknown"
					),
				]
			)
		)
	}

	func testConsumeReadsFreshCardsThenUsesTheExactCurrentIDInOneSession() async throws {
		let fixture = try makeFixture(
			scriptBody: """
			IFS= read -r line || exit 10
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '{"jsonrpc":"2.0","id":1,"result":{"codexHome":"%s","platformFamily":"unix","platformOs":"macos","userAgent":"fake"}}\\n' "$CODEX_HOME"
			IFS= read -r line || exit 11
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			IFS= read -r line || exit 12
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":2,"result":{"account":{"type":"chatgpt","email":"copy@example.com","planType":"pro"},"requiresOpenaiAuth":true}}'
			IFS= read -r line || exit 13
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":3,"result":{"rateLimits":{},"rateLimitResetCredits":{"availableCount":2,"credits":[{"id":"credit-a","grantedAt":100,"expiresAt":300,"resetType":"codexRateLimits","status":"available"},{"id":"credit-b","grantedAt":100,"expiresAt":200,"resetType":"codexRateLimits","status":"available"}]}}}'
			IFS= read -r line || exit 14
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":4,"result":{"outcome":"reset"}}'
			"""
		)
		defer { fixture.remove() }

		let result = try await CodexResetCreditBridge(responseTimeout: 2).consume(
			codexExecutableURL: fixture.executableURL,
			codexHomeURL: fixture.codexHomeURL,
			credentials: credentials,
			attempt: makeAttempt()
		)

		XCTAssertEqual(result.outcome, .reset)
		XCTAssertEqual(result.creditID, "credit-b")
		let messages = try readMessages(from: fixture.logURL)
		XCTAssertEqual(messages.count, 5)
		try assertBootstrap(messages)
		XCTAssertEqual(messages[3]["method"] as? String, "account/rateLimits/read")
		XCTAssertNil(messages[3]["params"])
		let consume = messages[4]
		XCTAssertEqual(
			consume["method"] as? String,
			"account/rateLimitResetCredit/consume"
		)
		let params = try XCTUnwrap(consume["params"] as? [String: Any])
		XCTAssertEqual(params["creditId"] as? String, "credit-b")
		XCTAssertEqual(params["idempotencyKey"] as? String, "attempt-1")
		XCTAssertEqual(try fixture.launchCount(), 1)
	}

	func testConsumeAcceptsAnEmailLessBoundAccount() async throws {
		let fixture = try makeFixture(
			scriptBody: """
			IFS= read -r line || exit 10
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '{"jsonrpc":"2.0","id":1,"result":{"codexHome":"%s","platformFamily":"unix","platformOs":"macos","userAgent":"fake"}}\\n' "$CODEX_HOME"
			IFS= read -r line || exit 11
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			IFS= read -r line || exit 12
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":2,"result":{"account":{"type":"chatgpt","email":null,"planType":"pro"},"requiresOpenaiAuth":true}}'
			IFS= read -r line || exit 13
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":3,"result":{"rateLimits":{},"rateLimitResetCredits":{"availableCount":1,"credits":[{"id":"credit-b","grantedAt":100,"expiresAt":200,"resetType":"codexRateLimits","status":"available"}]}}}'
			IFS= read -r line || exit 14
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":4,"result":{"outcome":"nothingToReset"}}'
			"""
		)
		defer { fixture.remove() }

		let result = try await CodexResetCreditBridge(responseTimeout: 2).consume(
			codexExecutableURL: fixture.executableURL,
			codexHomeURL: fixture.codexHomeURL,
			credentials: CodexResetCreditCredentials(expectedEmail: nil),
			attempt: makeAttempt()
		)

		XCTAssertEqual(result.outcome, .nothingToReset)
		XCTAssertEqual(result.creditID, "credit-b")
	}

	func testConsumeRejectsChangedCardsBeforeMutation() async throws {
		let fixture = try makeFixture(
			scriptBody: """
			IFS= read -r line || exit 10
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '{"jsonrpc":"2.0","id":1,"result":{"codexHome":"%s","platformFamily":"unix","platformOs":"macos","userAgent":"fake"}}\\n' "$CODEX_HOME"
			IFS= read -r line || exit 11
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			IFS= read -r line || exit 12
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":2,"result":{"account":{"type":"chatgpt","email":"copy@example.com","planType":"pro"},"requiresOpenaiAuth":true}}'
			IFS= read -r line || exit 13
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":3,"result":{"rateLimits":{},"rateLimitResetCredits":{"availableCount":1,"credits":[{"id":"credit-a","grantedAt":100,"expiresAt":300,"resetType":"codexRateLimits","status":"available"}]}}}'
			"""
		)
		defer { fixture.remove() }

		do {
			_ = try await CodexResetCreditBridge(responseTimeout: 2).consume(
				codexExecutableURL: fixture.executableURL,
				codexHomeURL: fixture.codexHomeURL,
				credentials: credentials,
				attempt: makeAttempt()
			)
			XCTFail("Changed reset cards should fail before consume.")
		} catch {
			XCTAssertEqual(
				error.localizedDescription,
				"The reset cards changed. Refresh and try again."
			)
		}

		let messages = try readMessages(from: fixture.logURL)
		XCTAssertEqual(messages.count, 4)
		try assertBootstrap(messages)
		XCTAssertEqual(messages[3]["method"] as? String, "account/rateLimits/read")
		XCTAssertFalse(
			messages.contains {
				$0["method"] as? String == "account/rateLimitResetCredit/consume"
			}
		)
	}

	func testRetryUsesTheResolvedCreditWithoutRemapping() async throws {
		let fixture = try makeFixture(
			scriptBody: """
			IFS= read -r line || exit 10
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '{"jsonrpc":"2.0","id":1,"result":{"codexHome":"%s","platformFamily":"unix","platformOs":"macos","userAgent":"fake"}}\\n' "$CODEX_HOME"
			IFS= read -r line || exit 11
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			IFS= read -r line || exit 12
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":2,"result":{"account":{"type":"chatgpt","email":"copy@example.com","planType":"pro"},"requiresOpenaiAuth":true}}'
			IFS= read -r line || exit 13
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":4,"result":{"outcome":"alreadyRedeemed"}}'
			"""
		)
		defer { fixture.remove() }

		let result = try await CodexResetCreditBridge(responseTimeout: 2).consume(
			codexExecutableURL: fixture.executableURL,
			codexHomeURL: fixture.codexHomeURL,
			credentials: credentials,
			attempt: makeAttempt(creditID: "credit-b")
		)

		XCTAssertEqual(result.outcome, .alreadyRedeemed)
		XCTAssertEqual(result.creditID, "credit-b")
		let messages = try readMessages(from: fixture.logURL)
		XCTAssertEqual(messages.count, 4)
		try assertBootstrap(messages)
		let consume = messages[3]
		XCTAssertEqual(
			consume["method"] as? String,
			"account/rateLimitResetCredit/consume"
		)
		let params = try XCTUnwrap(consume["params"] as? [String: Any])
		XCTAssertEqual(params["creditId"] as? String, "credit-b")
		XCTAssertEqual(params["idempotencyKey"] as? String, "attempt-1")
		XCTAssertFalse(
			messages.contains { $0["method"] as? String == "account/rateLimits/read" }
		)
	}

	func testConsumeFailureRetainsTheResolvedCreditForRetry() async throws {
		let fixture = try makeFixture(
			scriptBody: """
			IFS= read -r line || exit 10
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '{"jsonrpc":"2.0","id":1,"result":{"codexHome":"%s","platformFamily":"unix","platformOs":"macos","userAgent":"fake"}}\\n' "$CODEX_HOME"
			IFS= read -r line || exit 11
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			IFS= read -r line || exit 12
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":2,"result":{"account":{"type":"chatgpt","email":"copy@example.com","planType":"pro"},"requiresOpenaiAuth":true}}'
			IFS= read -r line || exit 13
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			printf '%s\\n' '{"jsonrpc":"2.0","id":3,"result":{"rateLimits":{},"rateLimitResetCredits":{"availableCount":1,"credits":[{"id":"credit-b","grantedAt":100,"expiresAt":200,"resetType":"codexRateLimits","status":"available"}]}}}'
			IFS= read -r line || exit 14
			printf '%s\\n' "$line" >> '__LOG_PATH__'
			exit 15
			"""
		)
		defer { fixture.remove() }

		do {
			_ = try await CodexResetCreditBridge(responseTimeout: 2).consume(
				codexExecutableURL: fixture.executableURL,
				codexHomeURL: fixture.codexHomeURL,
				credentials: credentials,
				attempt: makeAttempt()
			)
			XCTFail("A missing consume response should fail.")
		} catch let error as CodexResetCreditUseFailure {
			XCTAssertEqual(error.creditID, "credit-b")
		}
	}

	private var credentials: CodexResetCreditCredentials {
		CodexResetCreditCredentials(expectedEmail: "copy@example.com")
	}

	private func assertBootstrap(
		_ messages: [[String: Any]],
		file: StaticString = #filePath,
		line: UInt = #line
	) throws {
		XCTAssertEqual(
			Set(messages.compactMap { $0["jsonrpc"] as? String }),
			["2.0"],
			file: file,
			line: line
		)
		XCTAssertEqual(messages[0]["method"] as? String, "initialize", file: file, line: line)
		XCTAssertEqual(messages[1]["method"] as? String, "initialized", file: file, line: line)
		XCTAssertNil(messages[1]["params"], file: file, line: line)
		XCTAssertEqual(messages[2]["method"] as? String, "account/read", file: file, line: line)
		let accountRead = try XCTUnwrap(
			messages[2]["params"] as? [String: Any],
			file: file,
			line: line
		)
		XCTAssertEqual(accountRead["refreshToken"] as? Bool, false, file: file, line: line)
		XCTAssertFalse(
			messages.contains { message in String(describing: message).contains("access-token") },
			file: file,
			line: line
		)
	}

	private func makeTarget(
		occurrence: Int = 0,
		descriptorMultiplicity: Int = 1,
		detailsComplete: Bool = true
	) -> ResetCreditUseTarget {
		ResetCreditUseTarget(
			accountID: "account-a",
			descriptor: ResetCreditDescriptor(
				credit: AccountResetCredit(
					grantedAtUnixEpoch: 100,
					expiresAtUnixEpoch: 200,
					status: "available"
				)
			),
			occurrence: occurrence,
			descriptorMultiplicity: descriptorMultiplicity,
			detailsComplete: detailsComplete
		)
	}

	private func makeAttempt(creditID: String? = nil) -> ResetCreditUseAttempt {
		ResetCreditUseAttempt(
			target: makeTarget(),
			idempotencyKey: "attempt-1",
			creditID: creditID
		)
	}

	private func makeCredit(
		id: String,
		grantedAt: Int,
		expiresAt: Int,
		resetType: String = "codexRateLimits"
	) -> CodexRateLimitResetCredit {
		CodexRateLimitResetCredit(
			id: id,
			grantedAt: grantedAt,
			expiresAt: expiresAt,
			resetType: resetType,
			status: "available"
		)
	}

	private func makeFixture(scriptBody: String) throws -> AppServerFixture {
		let directoryURL = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		let codexHomeURL = directoryURL.appendingPathComponent("codex-home", isDirectory: true)
		let executableURL = directoryURL.appendingPathComponent("fake-codex")
		let logURL = directoryURL.appendingPathComponent("requests.jsonl")
		let launchLogURL = directoryURL.appendingPathComponent("launches.log")
		try FileManager.default.createDirectory(
			at: codexHomeURL,
			withIntermediateDirectories: true
		)

		let script = """
		#!/bin/sh
		printf '%s\\n' "$$" >> '\(launchLogURL.path)'
		[ -z "$CODEX_ACCESS_TOKEN" ] || exit 9
		[ -z "$OPENAI_API_KEY" ] || exit 8
		[ "$1" = "app-server" ] || exit 7
		[ "$2" = "--stdio" ] || exit 6
		[ "$3" = "-c" ] || exit 5
		[ "$4" = 'cli_auth_credentials_store="file"' ] || exit 4
		\(scriptBody.replacingOccurrences(of: "__LOG_PATH__", with: logURL.path))
		"""
		try script.write(to: executableURL, atomically: true, encoding: .utf8)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: executableURL.path
		)

		return AppServerFixture(
			directoryURL: directoryURL,
			codexHomeURL: codexHomeURL,
			executableURL: executableURL,
			logURL: logURL,
			launchLogURL: launchLogURL
		)
	}

	private func readMessages(from url: URL) throws -> [[String: Any]] {
		try String(contentsOf: url, encoding: .utf8)
			.split(separator: "\n")
			.map { line in
				let data = Data(line.utf8)
				return try XCTUnwrap(
					JSONSerialization.jsonObject(with: data) as? [String: Any]
				)
			}
	}
}

private struct AppServerFixture {
	let directoryURL: URL
	let codexHomeURL: URL
	let executableURL: URL
	let logURL: URL
	let launchLogURL: URL

	func launchCount() throws -> Int {
		try String(contentsOf: launchLogURL, encoding: .utf8)
			.split(separator: "\n")
			.count
	}

	func remove() {
		try? FileManager.default.removeItem(at: directoryURL)
	}
}
