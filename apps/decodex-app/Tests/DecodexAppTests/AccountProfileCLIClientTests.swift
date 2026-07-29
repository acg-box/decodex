@testable import DecodexApp
import Foundation
import XCTest

final class AccountProfileCLIClientTests: XCTestCase {
	private let accountID = "11111111-1111-4111-8111-111111111111"
	private let serverID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"

	func testProfileArgumentsPinAuthorityAndIncludeEmailOnlyWhenRequested() {
		let authority = ResetCardAuthority(profileName: "local", serverID: serverID)
		XCTAssertEqual(
			ResetCardCLIClient.profileArguments(
				accountID: accountID,
				includeEmail: false,
				authority: authority
			),
			[
				"--profile", "local",
				"--expected-server-id", serverID,
				"--output", "json",
				"account", "profile",
				"--account-id", accountID,
			]
		)
		XCTAssertEqual(
			ResetCardCLIClient.profileArguments(
				accountID: accountID,
				includeEmail: true,
				authority: authority
			).last,
			"--include-email"
		)
	}

	func testCurrentProfileDecodesEveryRichMetricAndVisibleEmail() async throws {
		let result = try await profile(
			json: document(
				result: """
				{"outcome":"current","data":\(profileJSON(email: #"{"visibility":"visible","value":"iris@example.com"}"#))}
				"""
			),
			includeEmail: true
		)

		guard case .available(let profile) = result else {
			return XCTFail("Expected one available profile.")
		}
		XCTAssertEqual(profile.accountID, accountID)
		XCTAssertEqual(profile.accountRevision, 7)
		XCTAssertEqual(profile.email, "iris@example.com")
		XCTAssertEqual(profile.planType, "pro")
		XCTAssertEqual(profile.displayName, "Iris")
		XCTAssertEqual(profile.username, "iris")
		XCTAssertEqual(profile.snapshot.lifetimeTokens, 1_234_567)
		XCTAssertEqual(profile.snapshot.peakDailyTokens, 42_000)
		XCTAssertEqual(profile.snapshot.longestTaskSeconds, 7_201)
		XCTAssertEqual(profile.snapshot.currentStreakDays, 4)
		XCTAssertEqual(profile.snapshot.longestStreakDays, 12)
		XCTAssertEqual(
			profile.snapshot.dailyUsage,
			[
				AccountProfileDailyUsage(date: "2026-07-27", tokens: 2_000),
				AccountProfileDailyUsage(date: "2026-07-28", tokens: 3_000),
			]
		)
		XCTAssertEqual(profile.freshness, .current)
	}

	func testCachedAndUnavailableProfilesRemainTyped() async throws {
		let cached = try await profile(
			json: document(
				result: """
				{"outcome":"cached","data":{"profile":\(profileJSON(email: #"{"visibility":"redacted"}"#)),"refresh_error":"provider_unavailable"}}
				"""
			),
			includeEmail: false
		)
		guard case .available(let observation) = cached else {
			return XCTFail("Expected cached profile.")
		}
		XCTAssertNil(observation.email)
		XCTAssertEqual(
			observation.freshness,
			.cached(refreshError: .providerUnavailable)
		)

		let unavailable = try await profile(
			json: document(
				result: """
				{"outcome":"unavailable","data":{"error":"credential_unavailable","email":{"visibility":"redacted"},"plan_type":"pro"}}
				"""
			),
			includeEmail: false
		)
		XCTAssertEqual(
			unavailable,
			.unavailable(
				AccountProfileUnavailable(
					error: .credentialUnavailable,
					claims: AccountProfileClaims(email: nil, planType: "pro")
				)
			)
		)

		let visibleUnavailableJSON = document(
			result: """
				{"outcome":"unavailable","data":{"error":"unauthorized","email":{"visibility":"visible","value":"iris@example.com"},"plan_type":"pro"}}
				"""
		)
		let visibleUnavailable = try await profile(
			json: visibleUnavailableJSON,
			includeEmail: true
		)
		XCTAssertEqual(
			visibleUnavailable,
			.unavailable(
				AccountProfileUnavailable(
					error: .unauthorized,
					claims: AccountProfileClaims(
						email: "iris@example.com",
						planType: "pro"
					)
				)
			)
		)

		do {
			_ = try await profile(
				json: visibleUnavailableJSON,
				includeEmail: false
			)
			XCTFail("A visible unavailable email must require an explicit request.")
		} catch let error as ResetCardClientError {
			XCTAssertEqual(error, .invalidResponse)
		}
	}

	func testProfileDecoderRejectsUnknownFieldsIdentityDriftAndPrivacyLeaks() async throws {
		let invalid = [
			document(
				result: """
				{"outcome":"current","data":\(profileJSON(email: #"{"visibility":"redacted"}"#).dropLast()),"unexpected":true}}
				"""
			),
			document(
				result: """
				{"outcome":"current","data":\(profileJSON(email: #"{"visibility":"visible","value":"iris@example.com"}"#))}
				"""
			),
			document(
				result: """
				{"outcome":"current","data":\(profileJSON(email: #"{"visibility":"redacted"}"#).replacingOccurrences(of: accountID, with: "22222222-2222-4222-8222-222222222222"))}
				"""
			),
			document(
				result: """
					{"outcome":"current","data":\(profileJSON(email: #"{"visibility":"redacted"}"#).replacingOccurrences(of: #""2026-07-28","tokens":3000"#, with: #""2026-07-27","tokens":3000"#))}
					"""
			),
			document(
				result: """
					{"outcome":"current","data":\(profileJSON(email: #"{"visibility":"redacted"}"#).replacingOccurrences(of: "2026-07-28", with: "ééééé"))}
					"""
			),
		]

		for json in invalid {
			do {
				_ = try await profile(json: json, includeEmail: false)
				XCTFail("Malformed, drifting, or privacy-violating profiles must fail closed.")
			} catch let error as ResetCardClientError {
				XCTAssertEqual(error, .invalidResponse)
			}
		}
	}

	func testProfileDecoderEnforcesExactClaimBoundsAndUnavailableFields() async throws {
		let longPlan = String(repeating: "p", count: 129)
		let longEmail = "\(String(repeating: "e", count: 309))@example.com"
		let invalid = [
			document(
				result: """
					{"outcome":"current","data":\(profileJSON(email: #"{"visibility":"redacted"}"#).replacingOccurrences(of: #""plan_type":"pro""#, with: #""plan_type":"\#(longPlan)""#))}
					"""
			),
			document(
				result: """
					{"outcome":"unavailable","data":{"error":"provider_unavailable","email":{"visibility":"visible","value":"\(longEmail)"},"plan_type":"pro"}}
					"""
			),
			document(
				result: """
					{"outcome":"unavailable","data":{"error":"provider_unavailable","email":{"visibility":"redacted"},"plan_type":"pro","unexpected":true}}
					"""
			),
		]

		for (index, json) in invalid.enumerated() {
			do {
				_ = try await profile(json: json, includeEmail: index == 1)
				XCTFail("Out-of-contract profile claims must fail closed.")
			} catch let error as ResetCardClientError {
				XCTAssertEqual(error, .invalidResponse)
			}
		}
	}

	private func profile(
		json: String,
		includeEmail: Bool
	) async throws -> AccountProfileRead {
		let directory = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		let executable = directory.appendingPathComponent("fake-decodex")
		try FileManager.default.createDirectory(
			at: directory,
			withIntermediateDirectories: true
		)
		defer { try? FileManager.default.removeItem(at: directory) }
		let script = """
		#!/bin/sh
		printf '%s\\n' '\(json)'
		"""
		try script.write(to: executable, atomically: true, encoding: .utf8)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: executable.path
		)
		let client = ResetCardCLIClient(
			executableURL: executable,
			environment: ["HOME": directory.path],
			timeout: 2
		)
		return try await client.profile(
			for: accountRecord(),
			includeEmail: includeEmail
		)
	}

	private func accountRecord() -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: ResetCardAuthority(profileName: "local", serverID: serverID),
			accountID: accountID,
			displayLabel: "Iris",
			accountRevision: 7,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
	}

	private func document(result: String) -> String {
		"""
		{"schema":"decodex/cli-account/1","command":"profile","outcome":"success","result":\(result)}
		"""
	}

	private func profileJSON(email: String) -> String {
		"""
		{"account_id":"\(accountID)","account_revision":7,"observed_at_unix_micros":1785276000000000,"email":\(email),"plan_type":"pro","display_name":"Iris","username":"iris","lifetime_tokens":1234567,"peak_daily_tokens":42000,"longest_task_seconds":7201,"current_streak_days":4,"longest_streak_days":12,"daily_usage":[{"start_date":"2026-07-27","tokens":2000},{"start_date":"2026-07-28","tokens":3000}]}
		"""
	}
}
