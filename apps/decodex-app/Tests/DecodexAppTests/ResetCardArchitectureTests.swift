import Foundation
import XCTest

final class ResetCardArchitectureTests: XCTestCase {
	func testRepeatedAccountRowsUseIndependentCardSurfaces() throws {
		let sourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let cardSurface = try String(
			contentsOf: sourceURL.appendingPathComponent("PanelCardSurface.swift"),
			encoding: .utf8
		)
		let accountPanel = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountPanelView.swift"),
			encoding: .utf8
		)

		XCTAssertFalse(cardSurface.contains(".id(appearanceID)"))
		XCTAssertFalse(accountPanel.contains("LazyVStack"))
		XCTAssertTrue(accountPanel.contains("ForEach(store.accounts)"))
	}

	func testHeaderAndOverviewShareOneCardWithoutARedundantTitleRow() throws {
		let sourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let accountPanel = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountPanelView.swift"),
			encoding: .utf8
		)
		let profileViews = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountProfileViews.swift"),
			encoding: .utf8
		)

		XCTAssertTrue(accountPanel.contains("private var headerOverview: some View"))
		XCTAssertTrue(accountPanel.contains("AccountProfileOverviewView("))
		XCTAssertTrue(accountPanel.contains(".panelCardSurface(cornerRadius: 18)"))
		XCTAssertFalse(profileViews.contains("Text(\"All accounts\")"))
		XCTAssertFalse(profileViews.contains("Text(\"All Accounts\")"))
		XCTAssertFalse(profileViews.contains("Image(systemName: \"chart.bar.xaxis\")"))
		XCTAssertTrue(profileViews.contains("lifetimeTokens: aggregate.lifetimeTokens"))
		XCTAssertTrue(profileViews.contains("peakDailyTokens: aggregate.peakDailyTokens"))
		XCTAssertTrue(profileViews.contains("longestTaskSeconds: aggregate.longestTaskSeconds"))
		XCTAssertTrue(profileViews.contains("currentStreakDays: aggregate.currentStreakDays"))
		XCTAssertTrue(profileViews.contains("AccountProfileMetric.makeOverview("))
		XCTAssertFalse(profileViews.contains("profiles current"))
		XCTAssertFalse(profileViews.contains(" of \\(totalAccountCount) daily"))
		XCTAssertFalse(profileViews.contains("showsAxis"))
	}

	func testPlanTierLivesBesideIdentityAndNotInTheDetailPopoverHeader() throws {
		let sourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let accountRows = try String(
			contentsOf: sourceURL.appendingPathComponent("ResetCardSectionView.swift"),
			encoding: .utf8
		)
		let profileViews = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountProfileViews.swift"),
			encoding: .utf8
		)

		XCTAssertTrue(accountRows.contains("if let planType"))
		XCTAssertTrue(accountRows.contains("Text(planType)"))
		XCTAssertTrue(
			accountRows.contains(
				"state.profile?.planType ?? state.profileUnavailable?.claims.planType"
			)
		)
		XCTAssertFalse(profileViews.contains("Text(\"Account details\")"))
		XCTAssertFalse(profileViews.contains("Text(planType)"))
	}

	func testAccountPanelControlsUseSentenceCase() throws {
		let sourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let panel = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountPanelView.swift"),
			encoding: .utf8
		)
		let controls = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountControlViews.swift"),
			encoding: .utf8
		)
		let login = try String(
			contentsOf: sourceURL.appendingPathComponent(
				"AccountReauthenticationView.swift"
			),
			encoding: .utf8
		)
		let source = panel + controls + login

		for canonicalLabel in [
			"Show email addresses",
			"Hide email addresses",
			"Turn Fast mode off",
			"Turn Fast mode on",
			"Refresh all",
			"Use balanced routing",
			"Disable account",
			"Enable account",
			"Log out",
			"Refresh login",
			"Cancel login",
			"Close login",
		] {
			XCTAssertTrue(
				source.contains("\"\(canonicalLabel)"),
				"Missing sentence-case label: \(canonicalLabel)"
			)
		}

		for retiredLabel in [
			"All Accounts",
			"Show Email Addresses",
			"Hide Email Addresses",
			"Turn Fast Mode",
			"Refresh All",
			"Use Balanced Routing",
			"Disable Account",
			"Enable Account",
			"Log Out",
			"Refresh Login",
			"Open Browser",
			"Copy Code",
		] {
			XCTAssertFalse(
				source.contains(retiredLabel),
				"Found retired title-case label: \(retiredLabel)"
			)
		}
	}

	func testPanelUsesSeparatedMaterialCardsWithoutLiquidGlass() throws {
		let sourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let cardSurface = try String(
			contentsOf: sourceURL.appendingPathComponent("PanelCardSurface.swift"),
			encoding: .utf8
		)
		let accountPanel = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountPanelView.swift"),
			encoding: .utf8
		)
		let accountControls = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountControlViews.swift"),
			encoding: .utf8
		)
		let resetCards = try String(
			contentsOf: sourceURL.appendingPathComponent("ResetCardSectionView.swift"),
			encoding: .utf8
		)
		let typography = try String(
			contentsOf: sourceURL.appendingPathComponent("PanelTypography.swift"),
			encoding: .utf8
		)
		let appScene = try String(
			contentsOf: sourceURL.appendingPathComponent("DecodexApp.swift"),
			encoding: .utf8
		)
		let statusPanel = try String(
			contentsOf: sourceURL.appendingPathComponent("StatusPanelController.swift"),
			encoding: .utf8
		)
		let panelWindow = try String(
			contentsOf: sourceURL.appendingPathComponent("PanelWindowSizing.swift"),
			encoding: .utf8
		)
		let panelControls = try String(
			contentsOf: sourceURL.appendingPathComponent("PanelControls.swift"),
			encoding: .utf8
		)
		let addControlStart = try XCTUnwrap(
			accountPanel.range(of: "symbol: \"plus\"")
		)
		let addControlEnd = try XCTUnwrap(
			accountPanel.range(
				of: "help: \"Add Codex login\"",
				range: addControlStart.lowerBound ..< accountPanel.endIndex
			)
		)
		let addControl = accountPanel[
			addControlStart.lowerBound ..< addControlEnd.upperBound
		]

		XCTAssertTrue(cardSurface.contains("shape.fill(.thinMaterial)"))
		XCTAssertTrue(cardSurface.contains("shape.fill(.regularMaterial)"))
		XCTAssertTrue(cardSurface.contains(".background"))
		XCTAssertTrue(cardSurface.contains(".shadow"))
		XCTAssertTrue(cardSurface.contains("radius: 3"))
		XCTAssertTrue(cardSurface.contains("radius: 10"))
		XCTAssertTrue(cardSurface.contains("y: 1"))
		XCTAssertTrue(cardSurface.contains("y: 4"))
		XCTAssertFalse(cardSurface.contains("cardFill"))
		XCTAssertFalse(cardSurface.contains("cardStroke"))
		XCTAssertFalse(cardSurface.contains("strokeBorder"))
		XCTAssertFalse(cardSurface.contains("Glass"))
		XCTAssertFalse(cardSurface.contains("glassEffect"))
		XCTAssertFalse(accountPanel.contains("GlassEffectContainer"))
		XCTAssertFalse(addControl.contains("isDisabled:"))
		XCTAssertTrue(accountPanel.contains(".panelCardSurface(cornerRadius: 18)"))
		XCTAssertTrue(accountPanel.contains(".panelCardSurface(cornerRadius: 16)"))
		XCTAssertFalse(accountPanel.contains(".panelCardSurface(cornerRadius: 20"))
		XCTAssertFalse(accountControls.contains(".panelCardSurface"))
		XCTAssertFalse(accountControls.contains("\"Use in Codex\""))
		XCTAssertTrue(accountControls.contains("await store.routeAccount("))
		XCTAssertFalse(accountControls.contains("await store.useAccountInCodex("))
		XCTAssertTrue(
			accountControls.contains(".disabled(store.canBeginEnrollment == false)")
		)
		XCTAssertFalse(resetCards.contains(".panelCardSurface(cornerRadius: 6"))
		XCTAssertFalse(resetCards.contains(".enumerated()"))
		XCTAssertFalse(resetCards.contains("ordinal"))
		XCTAssertFalse(resetCards.contains("Image(systemName: \"person.crop.circle\")"))
		XCTAssertFalse(resetCards.contains("identity.showsEmail ? \"envelope\""))
		XCTAssertTrue(resetCards.contains(".font(PanelFont.quotaText)"))
		XCTAssertTrue(
			typography.contains(
				"static let resetCardAction = text(9.4, weight: .medium)"
			)
		)
		XCTAssertTrue(resetCards.contains(": PanelPalette.secondaryText(colorScheme)"))
		XCTAssertFalse(appScene.contains("MenuBarExtra"))
		XCTAssertFalse(appScene.contains(".menuBarExtraStyle"))
		XCTAssertTrue(statusPanel.contains("NSStatusBar.system.statusItem"))
		XCTAssertTrue(statusPanel.contains("TransparentStatusPanel"))
		XCTAssertTrue(statusPanel.contains("styleMask: [.borderless]"))
		XCTAssertTrue(statusPanel.contains("TransparentHostingView(rootView: StatusPanelRootView"))
		XCTAssertTrue(statusPanel.contains("override var isOpaque"))
		XCTAssertTrue(statusPanel.contains("AccountPanelView(store: store)"))
		XCTAssertTrue(statusPanel.contains("panel.hidesOnDeactivate = false"))
		XCTAssertTrue(statusPanel.contains("button.sendAction(on: [.leftMouseDown])"))
		XCTAssertTrue(
			statusPanel.contains("NSApplication.didResignActiveNotification")
		)
		XCTAssertTrue(
			statusPanel.contains("#selector(applicationDidResignActive(_:))")
		)
		XCTAssertEqual(
			panelControls.components(
				separatedBy: "isDisabled && isActive == false"
			).count - 1,
			1
		)
		XCTAssertFalse(statusPanel.contains(".preferredColorScheme"))
		XCTAssertFalse(statusPanel.contains(".environment(\\.colorScheme"))
		XCTAssertTrue(resetCards.contains(".buttonStyle(.bordered)"))
		XCTAssertTrue(resetCards.contains(".controlSize(.small)"))
		XCTAssertTrue(
			resetCards.contains(
				"Self.cardExpiryText(target.descriptor.expiresAtUnixSeconds)"
			)
		)
		XCTAssertFalse(resetCards.contains("\"Use · "))
		XCTAssertFalse(resetCards.contains("Image(systemName: \"creditcard\")"))
		XCTAssertTrue(resetCards.contains(".frame(minWidth: 88, maxWidth: .infinity)"))
		XCTAssertFalse(accountPanel.contains("headerState("))
		XCTAssertFalse(accountPanel.contains("routingSubtitle"))
		XCTAssertFalse(accountPanel.contains("codexProjectionSubtitle"))
		XCTAssertTrue(panelWindow.contains("window.hasShadow = false"))
		XCTAssertTrue(
			panelWindow.contains(
				"window.contentView?.layer?.backgroundColor = NSColor.clear.cgColor"
			)
		)
		XCTAssertFalse(panelWindow.contains("window.appearance ="))
		XCTAssertFalse(
			FileManager.default.fileExists(
				atPath:
					sourceURL
					.appendingPathComponent("PanelInteractiveButtonStyle.swift")
					.path
			)
		)
	}

	func testProductionSourceUsesOnlyTheNativeResetCardAuthority() throws {
		let testsURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
		let appURL =
			testsURL
			.deletingLastPathComponent()
			.deletingLastPathComponent()
		let sourceURL =
			appURL
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let fileManager = FileManager.default
		let swiftFiles = try XCTUnwrap(
			fileManager.enumerator(
				at: sourceURL,
				includingPropertiesForKeys: [.isRegularFileKey]
			)
		)
		.compactMap { $0 as? URL }
		.filter { url in
			url.pathExtension == "swift"
		}

		XCTAssertFalse(swiftFiles.isEmpty)
		let banned = [
			"account/rateLimitResetCredit/consume",
			"app-server",
			"reset-card accounts",
			"CODEX_HOME",
			"access_token",
			"auth_json_path",
			"creditID",
			"creditId",
			"credit_id",
			"AccountStore",
			"DecodexAppBridge",
			"DecodexServerBridge",
			"DashboardWebSocket",
			"URLSessionWebSocketTask",
			"127.0.0.1:8192",
			"localhost:8192",
			"ResetCardCLIClient",
			"FastModeCLIClient",
			"Process(",
			"Contents/Helpers",
			"decodex-cli",
			"legacy",
			"vNext",
			"VNext",
		]
		for fileURL in swiftFiles {
			let source = try String(contentsOf: fileURL, encoding: .utf8)
			for term in banned {
				XCTAssertFalse(
					source.contains(term),
					"\(fileURL.lastPathComponent) contains retired authority or UI term \(term)"
				)
			}
		}

		for retiredFile in [
			"AccountStore.swift",
			"DecodexAppBridge.swift",
			"DecodexServerBridge.swift",
			"DashboardWebSocketConnection.swift",
			"LoginSheetView.swift",
			"OperatorSnapshotModels.swift",
		] {
			XCTAssertFalse(
				fileManager.fileExists(
					atPath: sourceURL.appendingPathComponent(retiredFile).path
				),
				"\(retiredFile) must not remain in the clean-break app."
			)
		}
	}

	func testBundleStagesTheAppAndNativeClientWithoutCLI() throws {
		let testsURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
		let appURL =
			testsURL
			.deletingLastPathComponent()
			.deletingLastPathComponent()
		let script = try String(
			contentsOf: appURL.appendingPathComponent("script/build_and_run.sh"),
			encoding: .utf8
		)

		XCTAssertTrue(
			script.contains(#"NATIVE_CLIENT_NAME="libdecodex_app_client_ffi.dylib""#)
		)
		XCTAssertTrue(script.contains("-p decodex-app-client-ffi"))
		XCTAssertTrue(
			script.contains(#"cp "$NATIVE_CLIENT_BINARY" "$APP_NATIVE_CLIENT""#)
		)
		XCTAssertTrue(script.contains(#""$APP_NATIVE_CLIENT""#))
		for retiredPackageTerm in [
			"CLI_NAME",
			"APP_CLI_BINARY",
			"-p decodex-cli",
			"Contents/Helpers",
			"decodex-app-helper",
			"-p decodexd",
			"decodex-server",
			"SERVER_NAME",
			"DAEMON_NAME",
			":8192",
		] {
			XCTAssertFalse(
				script.contains(retiredPackageTerm),
				"Build script still packages retired component \(retiredPackageTerm)."
			)
		}
	}

	func testLoginRecoveryUsesNativeBrowserOAuthAndNoPanelDisappearCancel() throws {
		let sourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let view = try String(
			contentsOf: sourceURL.appendingPathComponent(
				"AccountReauthenticationView.swift"
			),
			encoding: .utf8
		)
		let controls = try String(
			contentsOf: sourceURL.appendingPathComponent(
				"AccountControlViews.swift"
			),
			encoding: .utf8
		)
		let panel = try String(
			contentsOf: sourceURL.appendingPathComponent("AccountPanelView.swift"),
			encoding: .utf8
		)
		let app = try String(
			contentsOf: sourceURL.appendingPathComponent("DecodexApp.swift"),
			encoding: .utf8
		)
		let native = try String(
			contentsOf: sourceURL.appendingPathComponent("DecodexNativeClient.swift"),
			encoding: .utf8
		)

		XCTAssertTrue(view.contains(#"Image(systemName: "xmark")"#))
		XCTAssertFalse(view.contains(#"Image(systemName: "safari")"#))
		XCTAssertFalse(view.contains("doc.on.doc"))
		XCTAssertFalse(view.contains("one-time code"))
		XCTAssertFalse(view.contains("verificationURL"))
		XCTAssertTrue(view.contains(".keyboardShortcut(.cancelAction)"))
		XCTAssertTrue(view.contains(".frame(width: 220)"))
		XCTAssertTrue(view.contains(".padding(14)"))
		XCTAssertFalse(view.contains("Enter this one-time code"))
		XCTAssertFalse(view.contains("Divider()"))
		XCTAssertFalse(view.contains(".interactiveDismissDisabled"))
		XCTAssertFalse(view.contains(".onDisappear"))
		XCTAssertFalse(view.contains(".onChange(of:"))
		XCTAssertTrue(panel.contains("reauthenticationOverlay"))
		XCTAssertTrue(panel.contains(".disabled(store.accountReauthentication != nil)"))
		XCTAssertTrue(panel.contains(".allowsHitTesting(store.accountReauthentication == nil)"))
		XCTAssertTrue(panel.contains(".accessibilityHidden(store.accountReauthentication != nil)"))
		XCTAssertTrue(panel.contains(".panelModalSurface(cornerRadius: 16)"))
		XCTAssertTrue(panel.contains(".accessibilityAddTraits(.isModal)"))
		XCTAssertFalse(panel.contains(".padding(.horizontal, 20)"))
		XCTAssertFalse(panel.contains("isPresented: Binding("))
		XCTAssertTrue(view.contains("@FocusState private var focusedAction"))
		XCTAssertTrue(view.contains(".focused($focusedAction, equals: .cancel)"))
		XCTAssertTrue(view.contains(".task(id: desiredFocus)"))
		XCTAssertTrue(view.contains("presentation.canCloseWithoutCancellation"))
		XCTAssertTrue(
			controls.contains("store.beginAccountReauthentication(for:")
		)
		XCTAssertFalse(
			controls.contains("await store.refreshCredentials(for:")
		)
		XCTAssertTrue(app.contains("await DecodexNativeClient.shutdownSharedSession()"))
		XCTAssertTrue(native.contains("sharedSession.shutdown()"))
		XCTAssertTrue(native.contains("library.destroy(handle)"))
	}

	func testAccountDataUsesTheFormerFifteenSecondRefreshCadence() throws {
		let sourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("Sources/DecodexApp", isDirectory: true)
		let store = try String(
			contentsOf: sourceURL.appendingPathComponent("ResetCardStore.swift"),
			encoding: .utf8
		)
		let panel = try String(
			contentsOf: sourceURL.appendingPathComponent("StatusPanelController.swift"),
			encoding: .utf8
		)

		XCTAssertTrue(
			store.contains(
				"defaultAutomaticRefreshInterval: Duration = .seconds(15)"
			)
		)
		XCTAssertTrue(store.contains("private var automaticRefreshTask"))
		XCTAssertTrue(store.contains("private var refreshCycleTask"))
		XCTAssertTrue(store.contains("startAutomaticRefresh()"))
		XCTAssertTrue(store.contains("automaticRefreshTask?.cancel()"))
		XCTAssertTrue(store.contains("ContinuousClock()"))
		XCTAssertTrue(store.contains("clock.sleep(until: nextRefresh)"))
		XCTAssertTrue(store.contains("self.requestRefresh()"))
		XCTAssertTrue(store.contains("await self.performRefreshCycle()"))
		XCTAssertFalse(store.contains("Timer.publish"))
		XCTAssertTrue(panel.contains("store.requestRefresh()"))
	}
}
