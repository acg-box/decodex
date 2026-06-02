@testable import DecodexApp
import XCTest

final class AccountModelTests: XCTestCase {
	func testRefresh401RequiresReloginAndHidesCapacity() {
		let account = makeAccount(
			status: "unusable",
			recoveryAction: "login",
			refreshStatus: "failed",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertTrue(account.needsLogin)
		XCTAssertFalse(account.canRouteRuns)
		XCTAssertEqual(account.statusLabel, "login")
		XCTAssertNil(account.currentCapacityLabel)
	}

	func testExpiredAccountCanRefreshButDoesNotShowCapacity() {
		let account = makeAccount(
			status: "expired",
			recoveryAction: "refresh",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertFalse(account.needsLogin)
		XCTAssertTrue(account.canRouteRuns)
		XCTAssertEqual(account.statusLabel, "refresh")
		XCTAssertNil(account.currentCapacityLabel)
	}

	func testAvailableMeasuredAccountShowsCapacity() {
		let account = makeAccount(
			status: "available",
			planType: "pro",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertEqual(account.currentCapacityLabel, "20x")
	}

	func testCompactEmailKeepsDottedLocalSuffixesConsistent() {
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier@gmail.com"), "aur...ier@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.us@gmail.com"), "aur...us@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.jp@gmail.com"), "aur...jp@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("aurevoirxavier.hk@gmail.com"), "aur...hk@gmail.com")
		XCTAssertEqual(AccountDisplay.compactEmail("xavier.lau@helixbox.ai"), "xav...lau@helixbox.ai")
	}

	func testOperatorSnapshotAssignsCodexAccountRunsToAccountRows() throws {
		let assignedAccount = makeAccount(
			status: "available",
			email: "copy@example.com",
			accountFingerprint: "...123456"
		)
		let poolOnlyAccount = makeAccount(
			status: "available",
			email: "pool@example.com",
			accountFingerprint: "...654321"
		)
		let otherAssignedAccount = makeAccount(
			status: "available",
			email: "other@example.com",
			accountFingerprint: "...abcdef"
		)
		let payload = """
		{
		  "active_runs": [
		    {
		      "run_id": "run-1",
		      "issue_identifier": "XY-445",
		      "codex_account": {
		        "account_email": "copy@example.com",
		        "account_fingerprint": "...123456"
		      },
		      "codex_accounts": [
		        {
		          "account_email": "copy@example.com",
		          "account_fingerprint": "...123456"
		        },
		        {
		          "account_email": "pool@example.com",
		          "account_fingerprint": "...654321"
		        }
		      ]
		    },
		    {
		      "run_id": "run-2",
		      "issue_identifier": "PUB-1147",
		      "codex_account": {
		        "account_email": "other@example.com",
		        "account_fingerprint": "...abcdef"
		      },
		      "codex_accounts": [
		        {
		          "account_email": "copy@example.com",
		          "account_fingerprint": "...123456"
		        },
		        {
		          "account_email": "other@example.com",
		          "account_fingerprint": "...abcdef"
		        },
		        {
		          "account_email": "pool@example.com",
		          "account_fingerprint": "...654321"
		        }
		      ]
		    }
		  ]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: payload)

		XCTAssertEqual(snapshot.activeRuns(for: assignedAccount).map(\.runID), ["run-1"])
		XCTAssertEqual(snapshot.activeRuns(for: otherAssignedAccount).map(\.runID), ["run-2"])
		XCTAssertTrue(snapshot.activeRuns(for: poolOnlyAccount).isEmpty)
	}

	func testOperatorRunActivityOverlayDoesNotReplaceNewerSnapshot() throws {
		let account = makeAccount(
			status: "available",
			email: "copy@example.com",
			accountFingerprint: "...123456"
		)
		let snapshotPayload = """
		{
		  "active_runs": [
		    {
		      "run_id": "run-new",
		      "issue_identifier": "XY-672",
		      "account": {
		        "email": "copy@example.com",
		        "account_fingerprint": "...123456"
		      }
		    }
		  ]
		}
		""".data(using: .utf8)!
		let activityPayload = """
		{
		  "activeRuns": [
		    {
		      "run_id": "run-old",
		      "issue_identifier": "PUB-1147",
		      "account": {
		        "email": "copy@example.com",
		        "account_fingerprint": "...123456"
		      }
		    }
		  ]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: snapshotPayload)
		let activity = try JSONDecoder()
			.decode(OperatorDashboardSocketPayload.self, from: activityPayload)
			.activeRuns ?? []
		let overlay = OperatorRunActivitySnapshot(
			activeRuns: activity,
			emittedAt: Date(timeIntervalSince1970: 10)
		)

		XCTAssertFalse(overlay.shouldOverlay(snapshotPublishedAt: Date(timeIntervalSince1970: 20)))
		XCTAssertEqual(snapshot.activeRuns(for: account).map(\.runID), ["run-new"])
	}

	func testNewerEmptyRunActivityClearsSnapshotRuns() throws {
		let account = makeAccount(
			status: "available",
			email: "copy@example.com",
			accountFingerprint: "...123456"
		)
		let snapshotPayload = """
		{
		  "active_runs": [
		    {
		      "run_id": "run-old",
		      "issue_identifier": "XY-672",
		      "account": {
		        "email": "copy@example.com",
		        "account_fingerprint": "...123456"
		      }
		    }
		  ]
		}
		""".data(using: .utf8)!
		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: snapshotPayload)
		let overlay = OperatorRunActivitySnapshot(
			activeRuns: [],
			emittedAt: Date(timeIntervalSince1970: 30)
		)
		let merged = overlay.merging(into: snapshot)

		XCTAssertTrue(overlay.shouldOverlay(snapshotPublishedAt: Date(timeIntervalSince1970: 20)))
		XCTAssertTrue(merged.activeRuns(for: account).isEmpty)
	}

	func testOperatorSnapshotWarningSummaryUsesRawWarningToken() throws {
		let payload = """
		{
		  "warnings": ["external_observer_status_skipped"]
		}
		""".data(using: .utf8)!

		let snapshot = try JSONDecoder().decode(OperatorSnapshotResponse.self, from: payload)

		XCTAssertEqual(snapshot.warningSummary, "external_observer_status_skipped")
	}

	private func makeAccount(
		status: String,
		email: String = "copy@example.com",
		accountFingerprint: String = "...123456",
		recoveryAction: String? = nil,
		refreshStatus: String? = nil,
		planType: String? = nil,
		checkedAtUnixEpoch: Int? = nil,
		primaryRemainingPercent: Int? = nil
	) -> CodexAccount {
		CodexAccount(
			accountFingerprint: accountFingerprint,
			email: email,
			selector: email,
			randomName: nil,
			randomNameKey: nil,
			randomNameOffset: nil,
			status: status,
			selected: false,
			codexActive: false,
			disabled: false,
			refreshTokenPresent: true,
			accessTokenExpiresAtUnixEpoch: nil,
			lastSelectedAtUnixEpoch: nil,
			cooldownUntilUnixEpoch: nil,
			note: nil,
			planType: planType,
			capacityMultiplier: nil,
			recoveryAction: recoveryAction,
			refreshStatus: refreshStatus,
			checkedAtUnixEpoch: checkedAtUnixEpoch,
			primaryWindowSeconds: nil,
			primaryRemainingPercent: primaryRemainingPercent,
			primaryResetsAtUnixEpoch: nil,
			secondaryWindowSeconds: nil,
			secondaryRemainingPercent: nil,
			secondaryResetsAtUnixEpoch: nil,
			creditsHasCredits: nil,
			creditsUnlimited: nil,
			creditsBalance: nil,
			rateLimitReachedType: nil,
			profileDisplayName: nil,
			profileUsername: nil,
			profileCheckedAtUnixEpoch: nil,
			profileLifetimeTokens: nil,
			profilePeakDailyTokens: nil,
			profileLongestTaskSeconds: nil,
			profileCurrentStreakDays: nil,
			profileLongestStreakDays: nil,
			profileDailyUsage: nil,
			sevenDayUsedPercent: nil,
			sevenDayDailyAveragePercent: nil,
			usageRecords: nil
		)
	}
}
