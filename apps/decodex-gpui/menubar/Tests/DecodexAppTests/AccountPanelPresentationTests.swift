import AppKit
import SwiftUI
import XCTest

@testable import DecodexApp

@MainActor
final class AccountPanelPresentationTests: XCTestCase {
	func testSharedRouteOperationBlocksWithoutDimmingOtherAvailableRoutes() {
		let presentation = AccountRouteActionPresentation(
			isCurrent: false,
			canSelect: true,
			canPerformDirectAccountControl: true,
			isAccountControlInProgress: true,
			isSubmittingResetCard: false
		)

		XCTAssertTrue(presentation.isDisabled)
		XCTAssertFalse(presentation.isVisuallyDisabled)
		XCTAssertFalse(presentation.usesDisabledEnvironment)
	}

	func testUnavailableRouteRemainsVisiblyDisabled() {
		let presentation = AccountRouteActionPresentation(
			isCurrent: false,
			canSelect: false,
			canPerformDirectAccountControl: true,
			isAccountControlInProgress: false,
			isSubmittingResetCard: false
		)

		XCTAssertTrue(presentation.isDisabled)
		XCTAssertTrue(presentation.isVisuallyDisabled)
		XCTAssertTrue(presentation.usesDisabledEnvironment)
	}

	func testPendingRouteHidesConcreteSharedAuthBlockers() {
		let pending = AccountRoutePending(
			operationID: "33333333-3333-4333-8333-333333333333",
			accountID: "11111111-1111-4111-8111-111111111111",
			routingRevision: 9,
			waitReason: .externalCodex(
				blockers: [
					AccountRouteProcessBlocker(
						pid: 44662,
						process: .chatgpt,
						authHome: .shared
					),
					AccountRouteProcessBlocker(
						pid: 44768,
						process: .codex,
						authHome: .unknown
					),
				],
				omitted: 0
			)
		)

		XCTAssertEqual(
			pending.statusText,
			"Waiting for Codex to close or restart."
		)
		XCTAssertEqual(
			pending.helpText,
			"Close Codex and ChatGPT. Reopen them after the account is ready."
		)

		let hostingView = NSHostingView(
			rootView: AccountRoutePendingStatusView(pending: pending)
				.frame(width: 276)
		)
		hostingView.layoutSubtreeIfNeeded()
		XCTAssertGreaterThan(hostingView.fittingSize.height, 0)
		XCTAssertLessThanOrEqual(hostingView.fittingSize.height, 52)
	}

	func testPendingRouteActionAndStatusDeriveFromEveryExactWaitReason() {
		let blocker = AccountRouteProcessBlocker(
			pid: 44662,
			process: .codex,
			authHome: .shared
		)
		let cases: [(AccountRouteWaitReason, String, String)] = [
			(.externalCodex(blockers: [blocker], omitted: 0), "Waiting", "Waiting for Codex to close or restart."),
			(.codexObservationUnavailable, "Waiting", "Waiting for Codex to close or restart."),
			(.accountReadiness(.storeUnavailable), "Switching", "Switching"),
			(.accountReadiness(.storeMismatch), "Switching", "Switching"),
			(.accountReadiness(.operationUnsettled), "Switching", "Switching"),
			(.accountReadiness(.callbackCapabilityUnready), "Switching", "Switching"),
			(.sharedAuthStabilizing, "Switching", "Switching"),
			(.sharedAuthUnavailable, "Switching", "Switching"),
			(.projectionReadback, "Switching", "Switching"),
		]

		for (waitReason, actionTitle, statusText) in cases {
			let pending = AccountRoutePending(
				operationID: "33333333-3333-4333-8333-333333333333",
				accountID: "11111111-1111-4111-8111-111111111111",
				routingRevision: 9,
				waitReason: waitReason
			)
			XCTAssertEqual(pending.actionTitle, actionTitle)
			XCTAssertEqual(pending.statusText, statusText)
		}
	}

	func testRouteActionDoesNotTreatEveryPendingTargetAsWaiting() {
		let presentation = AccountRouteActionPresentation(
			isCurrent: false,
			canSelect: false,
			canPerformDirectAccountControl: true,
			isAccountControlInProgress: false,
			isSubmittingResetCard: false
		)
		let switching = AccountRoutePending(
			operationID: "33333333-3333-4333-8333-333333333333",
			accountID: "11111111-1111-4111-8111-111111111111",
			routingRevision: 9,
			waitReason: .projectionReadback
		)

		XCTAssertEqual(
			presentation.title(
				isSwitching: false,
				pending: switching,
				keepsCurrentRoute: false
			),
			"Switching"
		)

		let waiting = AccountRoutePending(
			operationID: "44444444-4444-4444-8444-444444444444",
			accountID: "11111111-1111-4111-8111-111111111111",
			routingRevision: 9,
			waitReason: .codexObservationUnavailable
		)
		XCTAssertEqual(
			presentation.title(
				isSwitching: true,
				pending: waiting,
				keepsCurrentRoute: false
			),
			"Waiting"
		)
	}

	func testTerminalReadbackAndNativeRecoveryUseReadyAndRestartStates() {
		let current = AccountRouteActionPresentation(
			isCurrent: true,
			canSelect: false,
			canPerformDirectAccountControl: true,
			isAccountControlInProgress: false,
			isSubmittingResetCard: false
		)

		XCTAssertEqual(
			current.title(isSwitching: false, pending: nil, keepsCurrentRoute: false),
			"Ready"
		)
		XCTAssertEqual(
			ResetCardClientError.nativeClientUnavailable.errorDescription,
			"Restart Decodex."
		)
		XCTAssertEqual(
			AccountControlError.client(.nativeClientUnavailable).errorDescription,
			"Restart Decodex."
		)
		XCTAssertEqual(FastModeClientError.unavailable.errorDescription, "Restart Decodex.")
	}

	func testRouteCapabilityDoesNotDependOnConversationCallbackReadiness() {
		let authority = ResetCardAuthority(
			profileName: "local",
			serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
		)
		let account = ResetCardAccountRecord(
			authority: authority,
			accountID: "11111111-1111-4111-8111-111111111111",
			alias: "Account TEST0-00000",
			accountRevision: 7,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .callbackCapabilityUnready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)

		XCTAssertEqual(
			ResetCardAccountState(
				account: account,
				inventory: nil,
				error: nil,
				isRefreshing: true
			).routeCapability,
			.ready
		)
		XCTAssertEqual(
			ResetCardAccountState(
				account: ResetCardAccountRecord(
					authority: authority,
					accountID: account.accountID,
					alias: account.alias,
					accountRevision: account.accountRevision,
					enabled: true,
					observedState: .available,
					lifecycleReadiness: .operationUnsettled,
					fiveHourQuota: .unknown(durationMinutes: 300),
					sevenDayQuota: .unknown(durationMinutes: 10_080)
				),
				inventory: nil,
				error: nil,
				isRefreshing: false
			).routeCapability,
			.operationPending
		)
	}

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
		XCTAssertEqual(presentation.valueText, "21%")
		XCTAssertEqual(presentation.tone, .warning)
		XCTAssertEqual(presentation.usedPercent, 79)
		XCTAssertEqual(presentation.remainingPercent, 21)
		XCTAssertNotNil(presentation.resetDate)
	}

	func testQuotaUsesTheNewestCurrentObservationDuringReconciliation() {
		let authority = ResetCardAuthority(
			profileName: "local",
			serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
		)
		let inventoryQuota = ResetCardQuotaWindow(
			durationMinutes: 300,
			observedAtUnixMicros: 1_000_000,
			state: .current(
				usedPercent: 100,
				resetsAtUnixMicros: 2_000_000
			)
		)
		let skeletonQuota = ResetCardQuotaWindow(
			durationMinutes: 300,
			observedAtUnixMicros: 2_000_000,
			state: .current(
				usedPercent: 0,
				resetsAtUnixMicros: 4_000_000
			)
		)
		let account = ResetCardAccountRecord(
			authority: authority,
			accountID: "11111111-1111-4111-8111-111111111111",
			alias: "Account TEST0-00000",
			accountRevision: 8,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			fiveHourQuota: skeletonQuota,
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
		let retainedInventory = ResetCardInventory(
			authority: authority,
			accountID: account.accountID,
			accountRevision: 7,
			cards: [],
			fiveHourQuota: inventoryQuota,
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: nil
		)

		let state = ResetCardAccountState(
			account: account,
			inventory: retainedInventory,
			error: nil,
			isRefreshing: true
		)

		XCTAssertEqual(state.fiveHourQuota, skeletonQuota)
	}

	func testCurrentQuotaToneTracksRemainingCapacity() {
		func tone(usedPercent: UInt8) -> ResetCardQuotaPresentationTone {
			ResetCardQuotaPresentation(
				window: ResetCardQuotaWindow(
					durationMinutes: 10_080,
					observedAtUnixMicros: 1_000_000,
					state: .current(
						usedPercent: usedPercent,
						resetsAtUnixMicros: 2_000_000
					)
				)
			).tone
		}

		XCTAssertEqual(tone(usedPercent: 49), .healthy)
		XCTAssertEqual(tone(usedPercent: 50), .warning)
		XCTAssertEqual(tone(usedPercent: 79), .warning)
		XCTAssertEqual(tone(usedPercent: 80), .critical)
		XCTAssertEqual(tone(usedPercent: 100), .critical)
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
			alias: "Iris",
			email: "iris@example.com",
			revealsEmail: true
		)
		XCTAssertEqual(visible.text, "iris@example.com")
		XCTAssertTrue(visible.showsEmail)

		let hidden = AccountIdentityPresentation(
			alias: "Iris",
			email: "iris@example.com",
			revealsEmail: false
		)
		XCTAssertEqual(hidden.text, "Iris")
		XCTAssertFalse(hidden.showsEmail)

		XCTAssertEqual(
			AccountIdentityPresentation(
				alias: "Iris",
				email: "  ",
				revealsEmail: true
			).text,
			"Iris"
		)
	}

	func testInstallingLoginCannotBeCancelledOrClosed() {
		let presentation = AccountReauthenticationPresentation(
			mode: .reauthentication,
			accountID: "10000000-0000-4000-8000-000000000001",
			accountLabel: "Val",
			sessionID: "20000000-0000-4000-8000-000000000001",
			authority: nil,
			phase: .installing,
			loginMethod: .browserRedirect,
			prompt: nil,
			authorizationURL: nil
		)

		XCTAssertFalse(presentation.canRequestCancellation)
		XCTAssertFalse(presentation.canCloseWithoutCancellation)
		XCTAssertEqual(presentation.title, "Refresh login")
		XCTAssertEqual(presentation.headerAccountLabel, "Val")
		XCTAssertEqual(presentation.statusText, "Saving login")
		XCTAssertTrue(presentation.showsStatusText)
	}

	func testEnrollmentPresentationUsesAddAccountLabels() {
		let presentation = AccountReauthenticationPresentation(
			mode: .enrollment,
			accountID: "10000000-0000-4000-8000-000000000001",
			accountLabel: "New account",
			sessionID: "20000000-0000-4000-8000-000000000001",
			authority: nil,
			phase: .installing,
			loginMethod: .deviceCode,
			prompt: nil,
			authorizationURL: nil
		)

		XCTAssertEqual(presentation.title, "Add account")
		XCTAssertNil(presentation.headerAccountLabel)
		XCTAssertEqual(presentation.accessibilityLabel, "Add account")
		XCTAssertEqual(presentation.statusText, "Adding account")
		XCTAssertEqual(presentation.cancelActionLabel, "Cancel adding account")
		XCTAssertEqual(presentation.closeActionLabel, "Close add account")
	}

	func testLoginMethodSelectorIsLocalAndDevicePromptHidesStatusCopy() {
		let selecting = AccountReauthenticationPresentation(
			mode: .enrollment,
			accountID: "10000000-0000-4000-8000-000000000001",
			accountLabel: "New account",
			sessionID: "20000000-0000-4000-8000-000000000001",
			authority: nil,
			phase: .selectingMethod,
			loginMethod: nil,
			prompt: nil,
			authorizationURL: nil
		)
		let prompt = AccountReauthenticationPrompt(
			verificationURL: AccountReauthenticationPrompt.verificationURL,
			userCode: "AB12-CDE34"
		)
		let device = AccountReauthenticationPresentation(
			mode: .enrollment,
			accountID: selecting.accountID,
			accountLabel: selecting.accountLabel,
			sessionID: selecting.sessionID,
			authority: nil,
			phase: .waitingForBrowser,
			loginMethod: .deviceCode,
			prompt: prompt,
			authorizationURL: nil
		)

		XCTAssertTrue(selecting.isSelectingMethod)
		XCTAssertTrue(selecting.canCloseWithoutCancellation)
		XCTAssertFalse(selecting.canRequestCancellation)
		XCTAssertEqual(selecting.statusText, "Choose a sign-in method")
		XCTAssertFalse(selecting.showsStatusText)
		XCTAssertFalse(selecting.showsProgress)
		XCTAssertEqual(device.prompt, prompt)
		XCTAssertEqual(device.statusText, "Waiting for browser sign-in")
		XCTAssertFalse(device.showsStatusText)
		XCTAssertFalse(device.showsProgress)
	}

	func testMissingAccountLabelDoesNotExposeAccountID() {
		let directory = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		defer { try? FileManager.default.removeItem(at: directory) }
		let store = ResetCardStore(
			client: AccountPanelLayoutClient(),
			pendingStore: ResetCardPendingAttemptStore(
				journalURL: directory.appendingPathComponent("pending.json")
			),
			startupRetryDelays: []
		)

		XCTAssertEqual(
			store.accountLabel(for: "11111111-1111-4111-8111-111111111111"),
			"Unknown account"
		)
	}

	func testPendingStatusRowStaysCompactAtPanelWidth() throws {
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
		let attempt = try pendingAttempt(1)
		XCTAssertEqual(pendingStore.insert(attempt), [attempt])
		let store = ResetCardStore(
			client: AccountPanelLayoutClient(),
			pendingStore: pendingStore,
			startupRetryDelays: []
		)
		let hostingView = NSHostingView(
			rootView: ResetCardPendingAttemptsView(store: store)
				.frame(width: 276)
		)

		hostingView.layoutSubtreeIfNeeded()

		XCTAssertLessThanOrEqual(hostingView.fittingSize.height, 44)
	}

	func testUnavailableProfileUnauthorizedPresentsLoginRecoveryWithoutHidingCanonicalInventory()
		throws
	{
		let authority = ResetCardAuthority(
			profileName: "local",
			serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
		)
		let account = ResetCardAccountRecord(
			authority: authority,
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
			inventory: ResetCardInventory(
				authority: authority,
				accountID: account.accountID,
				accountRevision: account.accountRevision,
				cards: [
					try ResetCardDescriptor(
						grantedAtUnixSeconds: 1_700_000_000,
						expiresAtUnixSeconds: 1_800_000_000
					)
				],
				fiveHourQuota: .unknown(durationMinutes: 300),
				sevenDayQuota: .unknown(durationMinutes: 10_080),
				observationError: nil
			),
			error: nil,
			isRefreshing: false,
			profileUnavailable: AccountProfileUnavailable(
				error: .unauthorized,
				claims: AccountProfileClaims(email: nil, planType: "pro")
			)
		)

		XCTAssertTrue(state.requiresLoginRefresh)
		XCTAssertEqual(state.targets.count, 1)

		let source = try resetCardSectionSource()
		XCTAssertFalse(source.contains("state.needsLoginRecovery"))
		XCTAssertEqual(
			source.components(separatedBy: "state.requiresLoginRefresh").count - 1,
			3
		)
	}

	func testStaleInventoryKeepsQuotaVisibleWithoutExposingUseTargets() throws {
		let authority = ResetCardAuthority(
			profileName: "local",
			serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
		)
		let account = ResetCardAccountRecord(
			authority: authority,
			accountID: "11111111-1111-4111-8111-111111111111",
			alias: "Account TEST0-00000",
			accountRevision: 8,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
		let retainedQuota = ResetCardQuotaWindow(
			durationMinutes: 300,
			observedAtUnixMicros: 1_000_000,
			state: .current(
				usedPercent: 100,
				resetsAtUnixMicros: 2_000_000
			)
		)
		let staleInventory = ResetCardInventory(
			authority: authority,
			accountID: account.accountID,
			accountRevision: 7,
			cards: [
				try ResetCardDescriptor(
					grantedAtUnixSeconds: 100,
					expiresAtUnixSeconds: 200
				)
			],
			fiveHourQuota: retainedQuota,
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: nil
		)
		let state = ResetCardAccountState(
			account: account,
			inventory: staleInventory,
			error: .transportBackpressured,
			isRefreshing: false
		)

		XCTAssertEqual(state.fiveHourQuota, retainedQuota)
		XCTAssertTrue(
			state.targets.isEmpty,
			"A retained inventory is display-only after the account revision advances."
		)
		XCTAssertEqual(
			ResetCardInventoryPresentation(
				state: state
			),
			.empty
		)
		XCTAssertEqual(
			ResetCardInventoryPresentation(
				state: ResetCardAccountState(
					account: account,
					inventory: staleInventory,
					error: .transportDisconnected,
					isRefreshing: false
				)
			),
			.empty
		)
		XCTAssertEqual(
			ResetCardInventoryPresentation(
				state: ResetCardAccountState(
					account: account,
					inventory: staleInventory,
					error: nil,
					isRefreshing: true
				)
			),
			.empty
		)

		let source = try resetCardSectionSource()
		XCTAssertFalse(source.contains("Updating usage…"))
		XCTAssertTrue(source.contains("Connecting to Decodex…"))
		XCTAssertFalse(source.contains("Reconnecting…"))
	}

	func testIncompletePositiveResetCardInventoryKeepsCheckingForExpiryDetails() {
		let authority = ResetCardAuthority(
			profileName: "local",
			serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
		)
		let account = ResetCardAccountRecord(
			authority: authority,
			accountID: "11111111-1111-4111-8111-111111111111",
			alias: "Blake",
			accountRevision: 2,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
		let inventory = ResetCardInventory(
			authority: authority,
			accountID: account.accountID,
			accountRevision: account.accountRevision,
			reportedAvailableCount: 1,
			detailsComplete: false,
			cards: [],
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: nil
		)
		let state = ResetCardAccountState(
			account: account,
			inventory: inventory,
			error: nil,
			isRefreshing: false
		)

		XCTAssertEqual(
			ResetCardInventoryPresentation(
				state: state
			),
			.unavailable(detail: "Reset Card details are temporarily unavailable.")
		)
	}

	func testQuotaRowsPutTheFlexibleBarBeforeTheCompactPercentage() throws {
		let source = try resetCardSectionSource()

		XCTAssertTrue(
			source.contains(
				".frame(width: Self.titleColumnWidth, alignment: .leading)"
			)
		)
		let progressRange = try XCTUnwrap(
			source.range(of: ".frame(minWidth: 88, maxWidth: .infinity)")
		)
		let valueRange = try XCTUnwrap(
			source.range(of: "Text(presentation.valueText)")
		)
		XCTAssertLessThan(
			source.distance(from: source.startIndex, to: progressRange.lowerBound),
			source.distance(from: source.startIndex, to: valueRange.lowerBound)
		)
		XCTAssertFalse(source.contains("valueColumnWidth"))
		XCTAssertFalse(source.contains("resetDateColumnWidth"))
	}

	func testResetCardChipAndAccessibilityExposeExpiryOnly() {
		let utc = try! XCTUnwrap(TimeZone(secondsFromGMT: 0))

		XCTAssertEqual(
			ResetCardAccountRow.cardExpiryText(0, timeZone: utc),
			"Jan 1 00:00"
		)
		XCTAssertEqual(
			ResetCardAccountRow.cardAccessibilityLabel(
				expiresAtUnixSeconds: 0,
				timeZone: utc
			),
			"Reset Card, expires Jan 1 at 00:00 GMT"
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
		let states = try (1...6).map { index in
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
					)
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
							)
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
		hostingView.frame =
			window.contentView?.bounds
			?? NSRect(x: 0, y: 0, width: 306, height: 1_350)

		for _ in 0..<2 {
			hostingView.layoutSubtreeIfNeeded()
			try await Task.sleep(for: .milliseconds(20))
		}

		XCTAssertFalse(window.isOpaque)
		XCTAssertEqual(window.backgroundColor, .clear)
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

	func testPendingRequestsUseTheirOwnBoundedScrollWithoutHidingAccounts() async throws {
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
		for index in 1...64 {
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
		hostingView.frame =
			window.contentView?.bounds
			?? NSRect(x: 0, y: 0, width: 340, height: 675)

		for _ in 0..<2 {
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

private func resetCardSectionSource() throws -> String {
	let testsURL = URL(fileURLWithPath: #filePath)
		.deletingLastPathComponent()
	let sourceURL =
		testsURL
		.deletingLastPathComponent()
		.deletingLastPathComponent()
		.appendingPathComponent("Sources/DecodexApp/ResetCardSectionView.swift")
	return try String(contentsOf: sourceURL, encoding: .utf8)
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
		(1...6).map(Self.account)
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
				)
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
						)
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
