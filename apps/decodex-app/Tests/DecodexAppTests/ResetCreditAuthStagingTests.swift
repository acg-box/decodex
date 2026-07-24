@testable import DecodexApp
import Foundation
import XCTest

final class ResetCreditAuthStagingTests: XCTestCase {
	func testStagingKeepsSelectedAccountAndExcludesManagedRefreshToken() throws {
		let auth = ResetCreditStoredAuth(
			email: nil,
			authMode: "chatgpt",
			lastRefresh: "2026-07-24T12:00:00Z",
			tokens: ResetCreditStoredAuth.Tokens(
				email: nil,
				idToken: "selected-id-token",
				accessToken: "selected-access-token",
				refreshToken: "managed-refresh-secret",
				accountID: "selected-account-id"
			)
		)

		let data = try auth.stagedData(refreshToken: "disabled-refresh-placeholder")
		let object = try XCTUnwrap(
			JSONSerialization.jsonObject(with: data) as? [String: Any]
		)
		let tokens = try XCTUnwrap(object["tokens"] as? [String: Any])

		XCTAssertEqual(object["auth_mode"] as? String, "chatgpt")
		XCTAssertEqual(object["last_refresh"] as? String, "2026-07-24T12:00:00Z")
		XCTAssertEqual(tokens["id_token"] as? String, "selected-id-token")
		XCTAssertEqual(tokens["access_token"] as? String, "selected-access-token")
		XCTAssertEqual(tokens["account_id"] as? String, "selected-account-id")
		XCTAssertEqual(tokens["refresh_token"] as? String, "disabled-refresh-placeholder")
		XCTAssertFalse(String(decoding: data, as: UTF8.self).contains("managed-refresh-secret"))
	}

	@MainActor
	func testFreshAccountBindingRejectsAnAmbiguousEmailLessFingerprint() throws {
		let target = try makeAccount(
			fingerprint: "...123456",
			email: nil,
			selector: "...123456"
		)
		let store = AccountStore()
		store.accountList = makeAccountList([
			target,
			try makeAccount(
				fingerprint: "...123456",
				email: "other@example.com",
				selector: "other@example.com"
			),
		])

		XCTAssertThrowsError(try store.uniqueFreshResetCreditAccount(matching: target)) { error in
			XCTAssertEqual(
				error.localizedDescription,
				"More than one stored account matches this reset card. Remove the duplicate account and try again."
			)
		}
	}

	@MainActor
	func testFreshAccountBindingAllowsAUniqueEmailLessFingerprint() throws {
		let target = try makeAccount(
			fingerprint: "...123456",
			email: nil,
			selector: "...123456"
		)
		let store = AccountStore()
		store.accountList = makeAccountList([target])

		XCTAssertEqual(
			try store.uniqueFreshResetCreditAccount(matching: target),
			target
		)
	}

	@MainActor
	func testFreshAccountBindingUsesNormalizedEmailWithTheFingerprint() throws {
		let target = try makeAccount(
			fingerprint: "...123456",
			email: " COPY@example.com ",
			selector: " COPY@example.com "
		)
		let expected = try makeAccount(
			fingerprint: "...123456",
			email: "copy@EXAMPLE.com",
			selector: "copy@EXAMPLE.com"
		)
		let store = AccountStore()
		store.accountList = makeAccountList([
			expected,
			try makeAccount(
				fingerprint: "...123456",
				email: "other@example.com",
				selector: "other@example.com"
			),
		])

		XCTAssertEqual(
			try store.uniqueFreshResetCreditAccount(matching: target),
			expected
		)
	}

	@MainActor
	func testFreshAccountBindingRejectsAMissingAccount() throws {
		let target = try makeAccount(
			fingerprint: "...123456",
			email: nil,
			selector: "...123456"
		)
		let store = AccountStore()
		store.accountList = makeAccountList([])

		XCTAssertThrowsError(try store.uniqueFreshResetCreditAccount(matching: target)) { error in
			XCTAssertEqual(
				error.localizedDescription,
				"The account changed. Refresh and try again."
			)
		}
	}

	private func makeAccount(
		fingerprint: String,
		email: String?,
		selector: String
	) throws -> CodexAccount {
		let data = try JSONSerialization.data(withJSONObject: [
			"account_fingerprint": fingerprint,
			"email": email.map { $0 as Any } ?? NSNull(),
			"selector": selector,
			"status": "available",
			"selected": false,
			"codex_active": false,
			"disabled": false,
			"refresh_token_present": true,
		])

		return try JSONDecoder().decode(CodexAccount.self, from: data)
	}
}
