import AppKit
@testable import DecodexApp
import SwiftUI
import XCTest

@MainActor
final class AccountPanelPresentationTests: XCTestCase {
	func testUnsupportedQuotaWindowIsHidden() {
		let presentation = ResetCardQuotaPresentation(
			window: ResetCardQuotaWindow(
				durationMinutes: 300,
				observedAtUnixMicros: 1_000_000,
				state: .error(.unsupportedWindow)
			)
		)

		XCTAssertFalse(presentation.isVisible)
		XCTAssertEqual(presentation.valueText, "—")
		XCTAssertEqual(presentation.detailText, "Not reported")
		XCTAssertEqual(presentation.tone, .muted)
		XCTAssertNil(presentation.usedPercent)
		XCTAssertNil(presentation.remainingPercent)
		XCTAssertNil(presentation.resetDate)
	}

	func testProtocolFailureIsKeptOutOfTheCompactQuotaRows() {
		let presentation = ResetCardQuotaPresentation(
			window: ResetCardQuotaWindow(
				durationMinutes: 10_080,
				observedAtUnixMicros: 1_000_000,
				state: .error(.protocolUnavailable)
			)
		)

		XCTAssertFalse(presentation.isVisible)
		XCTAssertEqual(presentation.valueText, "—")
		XCTAssertEqual(presentation.detailText, "Invalid provider response")
		XCTAssertEqual(presentation.tone, .muted)
	}

	func testCurrentQuotaRetainsValueAndResetDate() {
		let presentation = ResetCardQuotaPresentation(
			window: ResetCardQuotaWindow(
				durationMinutes: 10_080,
				observedAtUnixMicros: 1_000_000,
				state: .current(
					usedPercent: 79,
					resetsAtUnixMicros: 2_000_000
				)
			)
		)

		XCTAssertTrue(presentation.isVisible)
		XCTAssertEqual(presentation.valueText, "21% left")
		XCTAssertEqual(presentation.tone, .current)
		XCTAssertEqual(presentation.usedPercent, 79)
		XCTAssertEqual(presentation.remainingPercent, 21)
		XCTAssertNotNil(presentation.resetDate)
	}

	func testStaleQuotaHasANonColorStatusMarker() {
		let presentation = ResetCardQuotaPresentation(
			window: ResetCardQuotaWindow(
				durationMinutes: 300,
				observedAtUnixMicros: 1_000_000,
				state: .stale(
					usedPercent: 42,
					resetsAtUnixMicros: 2_000_000
				)
			)
		)

		XCTAssertTrue(presentation.isVisible)
		XCTAssertEqual(presentation.valueText, "58% left")
		XCTAssertEqual(presentation.detailText, "stale")
		XCTAssertEqual(presentation.tone, .current)
		XCTAssertEqual(presentation.usedPercent, 42)
		XCTAssertEqual(presentation.remainingPercent, 58)
		XCTAssertNotNil(presentation.resetDate)
	}

	func testUnknownOptionalQuotaWindowIsHidden() {
		let presentation = ResetCardQuotaPresentation(
			window: .unknown(durationMinutes: 300)
		)

		XCTAssertFalse(presentation.isVisible)
		XCTAssertEqual(presentation.valueText, "—")
		XCTAssertEqual(presentation.detailText, "No data")
		XCTAssertEqual(presentation.tone, .muted)
	}

	func testIdentityUsesExactlyOneEmailOrAliasSlot() {
		let visible = AccountIdentityPresentation(
			alias: "Account 7M4K-P2Q8",
			email: "iris@example.com",
			revealsEmail: true
		)
		XCTAssertEqual(visible.text, "iris@example.com")
		XCTAssertTrue(visible.showsEmail)

		let hidden = AccountIdentityPresentation(
			alias: "Account 7M4K-P2Q8",
			email: "iris@example.com",
			revealsEmail: false
		)
		XCTAssertEqual(hidden.text, "Account 7M4K-P2Q8")
		XCTAssertFalse(hidden.showsEmail)

		XCTAssertEqual(
			AccountIdentityPresentation(
				alias: "Account 7M4K-P2Q8",
				email: "  ",
				revealsEmail: true
			).text,
			"Account 7M4K-P2Q8"
		)
	}

	func testProfileUnauthorizedDoesNotOverrideCanonicalAccountAvailability() {
		let account = ResetCardAccountRecord(
			authority: nil,
			accountID: "11111111-1111-4111-8111-111111111111",
			alias: "Account TEST0-00000",
			accountRevision: 1,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
		let state = ResetCardAccountState(
			account: account,
			inventory: nil,
			error: nil,
			isRefreshing: false,
			profileUnavailable: AccountProfileUnavailable(
				error: .unauthorized,
				claims: AccountProfileClaims(email: nil, planType: "pro")
			)
		)

		XCTAssertFalse(state.requiresLoginRefresh)
	}

	func testCodexProjectionDistinguishesUnmanagedUnavailableAndCurrent() {
		XCTAssertEqual(
			CodexProjectionPresentation(
				projection: .unmanaged,
				currentIdentity: nil,
				isInitialLoading: false
			).text,
			"Not managed by Decodex"
		)
		XCTAssertEqual(
			CodexProjectionPresentation(
				projection: .unavailable,
				currentIdentity: nil,
				isInitialLoading: false
			).text,
			"Unavailable"
		)
		XCTAssertEqual(
			CodexProjectionPresentation(
				projection: .current(
					accountID: "11111111-1111-4111-8111-111111111111",
					accountRevision: 7,
					projectionDigest: String(repeating: "a", count: 64)
				),
				currentIdentity: "iris@example.com",
				isInitialLoading: false
			).text,
			"iris@example.com"
		)
		XCTAssertEqual(
			CodexProjectionPresentation(
				projection: .current(
					accountID: "11111111-1111-4111-8111-111111111111",
					accountRevision: 8,
					projectionDigest: String(repeating: "b", count: 64)
				),
				currentIdentity: nil,
				isInitialLoading: false
			).text,
			"Checking account"
		)
	}

	func testResetCardChipAndAccessibilityExposeExpiryOnly() {
		let utc = try! XCTUnwrap(TimeZone(secondsFromGMT: 0))

		XCTAssertEqual(
			ResetCardAccountRow.cardExpiryText(0, timeZone: utc),
			"Jan 1 00:00"
		)
		XCTAssertEqual(
			ResetCardAccountRow.cardAccessibilityLabel(
				ordinal: 2,
				expiresAtUnixSeconds: 0,
				timeZone: utc
			),
			"Reset Card 2, expires Jan 1 at 00:00 GMT"
		)
	}

	func testSixCompactAccountRowsStayWithinTheCurrentDisplayBudget() throws {
		let directory = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		let store = ResetCardStore(
			client: AccountPanelLayoutClient(),
			pendingStore: ResetCardPendingAttemptStore(
				journalURL: directory.appendingPathComponent("pending.json")
			),
			startupRetryDelays: []
		)
		let authority = ResetCardAuthority(
			profileName: "local",
			serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
		)
		let states = try (1 ... 6).map { index in
			let accountID = String(
				format: "018f0f9e-7b6e-4a31-8f4c-%012d",
				index
			)
			let account = ResetCardAccountRecord(
				authority: authority,
				accountID: accountID,
				alias: "Account \(index)",
				accountRevision: UInt64(index),
				enabled: true,
				observedState: .available,
				lifecycleReadiness: .ready,
				fiveHourQuota: .unknown(durationMinutes: 300),
				sevenDayQuota: .unknown(durationMinutes: 10_080)
			)
			let inventory = ResetCardInventory(
				authority: authority,
				accountID: accountID,
				accountRevision: UInt64(index),
				cards: [
					try ResetCardDescriptor(
						grantedAtUnixSeconds: 1_700_000_000,
						expiresAtUnixSeconds: 1_800_000_000
					),
				],
				fiveHourQuota: ResetCardQuotaWindow(
					durationMinutes: 300,
					observedAtUnixMicros: 1_000_000,
					state: .current(
						usedPercent: 25,
						resetsAtUnixMicros: 2_000_000
					)
				),
				sevenDayQuota: ResetCardQuotaWindow(
					durationMinutes: 10_080,
					observedAtUnixMicros: 1_000_000,
					state: .current(
						usedPercent: 50,
						resetsAtUnixMicros: 3_000_000
					)
				),
				observationError: nil
			)
			return ResetCardAccountState(
				account: account,
				inventory: inventory,
				error: nil,
				isRefreshing: false,
				profile: AccountProfileObservation(
					accountID: accountID,
					accountRevision: UInt64(index),
					observedAtUnixMicros: 1_785_276_000_000_000,
					email: "account\(index)@example.com",
					planType: "pro",
					displayName: "Account \(index)",
					username: "account\(index)",
					snapshot: AccountProfileSnapshot(
						lifetimeTokens: UInt64(index) * 100_000,
						peakDailyTokens: UInt64(index) * 10_000,
						longestTaskSeconds: UInt64(index) * 60,
						currentStreakDays: UInt32(index),
						longestStreakDays: UInt32(index * 2),
						dailyUsage: [
							AccountProfileDailyUsage(
								date: "2026-07-28",
								tokens: UInt64(index) * 1_000
							),
						]
					),
					freshness: .current
				)
			)
		}

		let rows = VStack(spacing: 0) {
			ForEach(states) { state in
				ResetCardAccountRow(state: state, store: store)
			}
		}
		.frame(width: AccountPanelLayout.panelWidth)
		let hostingView = NSHostingView(rootView: rows)
		hostingView.layoutSubtreeIfNeeded()

		XCTAssertGreaterThan(hostingView.fittingSize.height, 300)
		XCTAssertLessThanOrEqual(hostingView.fittingSize.height, 708)

		let singleRow = NSHostingView(
			rootView: ResetCardAccountRow(
				state: try XCTUnwrap(states.first),
				store: store,
				showsEmail: true
			)
			.frame(width: AccountPanelLayout.panelWidth)
		)
		singleRow.layoutSubtreeIfNeeded()
		XCTAssertLessThanOrEqual(singleRow.fittingSize.height, 118)
	}

	func testFullAccountPanelShowsSixCompactRowsWithoutOverflowOnCurrentDisplay() async throws {
		let directory = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		defer { try? FileManager.default.removeItem(at: directory) }
		let store = ResetCardStore(
			client: FullAccountPanelClient(),
			pendingStore: ResetCardPendingAttemptStore(
				journalURL: directory.appendingPathComponent("pending.json")
			),
			startupRetryDelays: []
		)
		await store.refresh()
		XCTAssertEqual(store.accounts.count, 6)

		let hostingView = NSHostingView(
			rootView: AccountPanelView(
				store: store,
				fastModeStore: FastModeStore(client: StaticFastModeClient()),
				layoutVisibleFrameOverride: NSRect(
					x: 0,
					y: 0,
					width: 1_600,
					height: 1_350
				),
				loadsExternalState: false
			)
		)
		let window = NSWindow(
			contentRect: NSRect(x: 0, y: 0, width: 306, height: 1_350),
			styleMask: [.borderless],
			backing: .buffered,
			defer: false
		)
		window.contentView = hostingView
		hostingView.frame = window.contentView?.bounds
			?? NSRect(x: 0, y: 0, width: 306, height: 1_350)

		for _ in 0 ..< 2 {
			hostingView.layoutSubtreeIfNeeded()
			try await Task.sleep(for: .milliseconds(20))
		}

		let scrollViews = descendants(
			of: NSScrollView.self,
			in: hostingView
		)
		let accountScroll = try XCTUnwrap(
			scrollViews.first { scrollView in
				guard let documentView = scrollView.documentView else {
					return false
				}
				return scrollView.contentView.bounds.height > 100
					&& documentView.bounds.height > 100
			}
		)
		let documentView = try XCTUnwrap(accountScroll.documentView)
		XCTAssertLessThanOrEqual(
			documentView.bounds.height,
			accountScroll.contentView.bounds.height + 1
		)
		XCTAssertFalse(accountScroll.hasVerticalScroller)
		XCTAssertLessThanOrEqual(hostingView.fittingSize.height, 1_350)
	}

	func testPendingActionsUseTheirOwnBoundedScrollWithoutHidingAccounts() async throws {
		let directory = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		defer { try? FileManager.default.removeItem(at: directory) }
		try FileManager.default.createDirectory(
			at: directory,
			withIntermediateDirectories: true
		)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: directory.path
		)
		let pendingStore = ResetCardPendingAttemptStore(
			journalURL: directory.appendingPathComponent("pending.json")
		)
		for index in 1 ... 64 {
			XCTAssertNotNil(pendingStore.insert(try pendingAttempt(index)))
		}
		let store = ResetCardStore(
			client: FullAccountPanelClient(),
			pendingStore: pendingStore,
			startupRetryDelays: []
		)
		await store.refresh()
		XCTAssertEqual(store.accounts.count, 6)
		XCTAssertEqual(store.pendingAttempts.count, 64)

		let hostingView = NSHostingView(
			rootView: AccountPanelView(
				store: store,
				fastModeStore: FastModeStore(client: StaticFastModeClient()),
				layoutVisibleFrameOverride: NSRect(
					x: 0,
					y: 0,
					width: 800,
					height: 675
				),
				loadsExternalState: false
			)
		)
		let window = NSWindow(
			contentRect: NSRect(x: 0, y: 0, width: 340, height: 675),
			styleMask: [.borderless],
			backing: .buffered,
			defer: false
		)
		window.contentView = hostingView
		hostingView.frame = window.contentView?.bounds
			?? NSRect(x: 0, y: 0, width: 340, height: 675)

		for _ in 0 ..< 2 {
			hostingView.layoutSubtreeIfNeeded()
			try await Task.sleep(for: .milliseconds(20))
		}

		let overflowingScrollViews = descendants(
			of: NSScrollView.self,
			in: hostingView
		).filter { scrollView in
			guard let documentView = scrollView.documentView else {
				return false
			}
			return documentView.bounds.height > scrollView.contentView.bounds.height + 1
		}
		XCTAssertGreaterThanOrEqual(overflowingScrollViews.count, 2)
		XCTAssertTrue(
			overflowingScrollViews.contains { scrollView in
					abs(
						scrollView.contentView.bounds.height
							- AccountPanelLayout.statusMaximumHeight
					) < 4
			}
		)
		XCTAssertLessThanOrEqual(hostingView.fittingSize.height, 675)
	}
}

@MainActor
private func descendants<T: NSView>(
	of _: T.Type,
	in root: NSView
) -> [T] {
	var matches = [T]()
	if let match = root as? T {
		matches.append(match)
	}
	for child in root.subviews {
		matches.append(contentsOf: descendants(of: T.self, in: child))
	}
	return matches
}

private struct StaticFastModeClient: FastModeClient {
	func status() async throws -> Bool {
		false
	}

	func setEnabled(_ enabled: Bool) async throws -> Bool {
		enabled
	}
}

private func pendingAttempt(_ index: Int) throws -> ResetCardUseAttempt {
	ResetCardUseAttempt(
		target: ResetCardUseTarget(
			authority: ResetCardAuthority(
				profileName: "local",
				serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
			),
			accountID: "018f0f9e-7b6e-4a31-8f4c-000000000001",
			expectedRevision: 1,
			descriptor: try ResetCardDescriptor(
				grantedAtUnixSeconds: Int64(1_700_000_000 + index * 2),
				expiresAtUnixSeconds: Int64(1_700_000_001 + index * 2)
			)
		),
		idempotencyKey: String(
			format: "018f0f9e-7b6e-4a31-8f4c-%012llx",
			UInt64(index)
		)
	)
}

private actor FullAccountPanelClient: ResetCardClient, AccountProfileClient {
	private static let authority = ResetCardAuthority(
		profileName: "local",
		serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
	)

	func accounts(
		authority _: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		(1 ... 6).map(Self.account)
	}

	func inventory(
		for account: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		ResetCardInventory(
			authority: Self.authority,
			accountID: account.accountID,
			accountRevision: account.accountRevision,
			cards: [
				try ResetCardDescriptor(
					grantedAtUnixSeconds: 1_700_000_000,
					expiresAtUnixSeconds: 1_800_000_000
				),
			],
			fiveHourQuota: ResetCardQuotaWindow(
				durationMinutes: 300,
				observedAtUnixMicros: 1_000_000,
				state: .current(
					usedPercent: 25,
					resetsAtUnixMicros: 2_000_000
				)
			),
			sevenDayQuota: ResetCardQuotaWindow(
				durationMinutes: 10_080,
				observedAtUnixMicros: 1_000_000,
				state: .current(
					usedPercent: 50,
					resetsAtUnixMicros: 3_000_000
				)
			),
			observationError: nil
		)
	}

	func profile(
		for account: ResetCardAccountRecord,
		includeEmail: Bool
	) async throws -> AccountProfileRead {
		.available(
			AccountProfileObservation(
				accountID: account.accountID,
				accountRevision: account.accountRevision,
				observedAtUnixMicros: 1_785_276_000_000_000,
				email: includeEmail ? "\(account.alias.lowercased())@example.com" : nil,
				planType: "pro",
				displayName: account.alias,
				username: account.alias.lowercased(),
				snapshot: AccountProfileSnapshot(
					lifetimeTokens: account.accountRevision * 100_000,
					peakDailyTokens: account.accountRevision * 10_000,
					longestTaskSeconds: account.accountRevision * 60,
					currentStreakDays: UInt32(account.accountRevision),
					longestStreakDays: UInt32(account.accountRevision * 2),
					dailyUsage: [
						AccountProfileDailyUsage(
							date: "2026-07-28",
							tokens: account.accountRevision * 1_000
						),
					]
				),
				freshness: .current
			)
		)
	}

	func use(
		_: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func status(
		for _: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	private static func account(_ index: Int) -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: authority,
			accountID: String(
				format: "018f0f9e-7b6e-4a31-8f4c-%012d",
				index
			),
			alias: "Account \(index)",
			accountRevision: UInt64(index),
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
	}
}

private actor AccountPanelLayoutClient: ResetCardClient {
	func accounts(
		authority _: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		[]
	}

	func inventory(
		for _: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		throw ResetCardClientError.invalidResponse
	}

	func use(
		_: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func status(
		for _: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}
}
