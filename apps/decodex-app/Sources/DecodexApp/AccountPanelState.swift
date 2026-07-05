import AppKit
import SwiftUI

extension AccountPanelView {
	func displayName(for account: CodexAccount) -> String {
		if emailsHidden {
			return AccountDisplay.aliases(for: store.accounts)[account.id]
				?? account.panelDisplayName(emailsHidden: true)
		}

		return account.panelDisplayName(emailsHidden: false)
	}

	var operatorCurrentLaneCards: [OperatorCurrentLaneCard] {
		store.operatorPresentation?.currentLaneCards
			?? store.operatorSnapshot?.presentation?.currentLaneCards
			?? []
	}

	func operatorCurrentLaneCards(for account: CodexAccount) -> [OperatorCurrentLaneCard] {
		operatorCurrentLaneCards.filter { $0.isAssigned(to: account) }
	}

	func accountRowHeight(for account: CodexAccount) -> CGFloat {
		let base: CGFloat
		if account.hasUsageWindowSummary {
			base = 102
		} else {
			base = 48
		}
		let runSignal: CGFloat = operatorCurrentLaneCards(for: account).isEmpty ? 0 : 22
		let profileSignal: CGFloat = account.hasProfileSummary
			? (account.recentProfileDailyUsage.isEmpty ? 19 : 35)
			: 0

		return base + runSignal + profileSignal
	}

	func account(matching selector: String) -> CodexAccount? {
		store.accounts.first { account in
			account.matchesSelector(selector)
		}
	}

	func requestLogout(_ account: CodexAccount) {
		guard deletingLogoutAccountID == nil else {
			return
		}
		if armedLogoutAccountID == account.id {
			return
		}

		withAnimation(PanelMotion.inlineLayout) {
			logoutErrorMessage = nil
			armedLogoutAccountID = account.id
		}
	}

	func confirmLogout(_ account: CodexAccount) {
		Task {
			withAnimation(PanelMotion.accountRemoval) {
				deletingLogoutAccountID = account.id
				store.beginOptimisticLogoutRemoval(account)
			}
			logoutErrorMessage = nil
			do {
				try await store.logout(account)
				deletingLogoutAccountID = nil
				disarmLogout()
			} catch {
				withAnimation(PanelMotion.accountRemoval) {
					store.cancelOptimisticLogoutRemoval(account)
					deletingLogoutAccountID = nil
					logoutErrorMessage = error.localizedDescription
				}
			}
		}
	}

	func disarmLogout() {
		guard deletingLogoutAccountID == nil else {
			return
		}
		withAnimation(PanelMotion.inlineLayout) {
			logoutErrorMessage = nil
			armedLogoutAccountID = nil
		}
	}

	func presentLogin(_ mode: AccountLoginSheetMode) {
		loginWindowState.mode = mode
		store.resetLoginSession()
		NSApp.activate(ignoringOtherApps: true)
		loginWindowState.isPresented = true
	}
}

extension AccountPanelView {
	var codexAuthLabel: String {
		guard let auth = store.accountList?.codexAuth else {
			return "No Codex auth"
		}

		if emailsHidden {
			if let account = account(matching: auth.selector) {
				return displayName(for: account)
			}
			let identity = auth.accountFingerprint.isEmpty ? auth.selector : auth.accountFingerprint
			return AccountDisplay.alias(forIdentity: identity)
		}

		return AccountDisplay.compactEmail(auth.displayName)
	}

	var decodexModeLabel: String {
		guard let control = store.accountList?.control else {
			return "Not loaded"
		}

		if let selector = control.accountSelector, selector.isEmpty == false {
			if emailsHidden {
				if let account = account(matching: selector) {
					return "To \(displayName(for: account))"
				}

				return "To \(AccountDisplay.alias(forIdentity: selector))"
			}

			if selector.contains("@") {
				return "To \(AccountDisplay.compactEmail(selector))"
			}

			return "To \(AccountDisplay.compactIdentity(selector))"
		}

		return control.mode
	}

	var hasFixedSelection: Bool {
		guard let selector = store.accountList?.control.accountSelector else {
			return false
		}

		return selector.isEmpty == false
	}

	var headerSubtitle: String {
		let count = store.accounts.count
		let accountLabel = "\(count) account\(count == 1 ? "" : "s")"
		let routeLabel = hasFixedSelection ? "Routed" : "Balanced"
		return "\(accountLabel) · \(routeLabel)"
	}

	var emailsHidden: Bool {
		accountPrivacy != AccountPrivacy.visibleValue
	}
}

extension AccountPanelView {
	var accountProfileAggregate: AccountProfileAggregate? {
		AccountProfileAggregate.make(accounts: store.accounts)
	}

	var telemetryMatrixIsVisible: Bool {
		accountProfileAggregate != nil
			|| store.accountList?.usageEstimate != nil
	}

	var telemetryMatrixHeight: CGFloat {
		var rows = [CGFloat]()
		if accountProfileAggregate != nil {
			rows.append(AccountPanelLayout.telemetryProfileHeight)
		}
		if let estimate = store.accountList?.usageEstimate {
			rows.append(
				estimate.accountEstimateCount < estimate.accountCount
					? AccountPanelLayout.telemetryPoolMeasuredHeight
					: AccountPanelLayout.telemetryPoolHeight
			)
		}
		guard rows.isEmpty == false else {
			return 0
		}

		return AccountPanelLayout.telemetryVerticalPadding
			+ rows.reduce(0, +)
			+ CGFloat(rows.count - 1) * AccountPanelLayout.telemetryRowSpacing
	}
}

extension AccountPanelView {
	var panelAnimationKey: AccountPanelAnimationKey {
		AccountPanelAnimationKey(
			accountIDs: store.accounts.map(\.id),
			isInitialLoading: store.isInitialLoading,
			hasAccounts: store.accounts.isEmpty == false,
			hasTelemetry: telemetryMatrixIsVisible,
			hasNotice: store.notice != nil,
			hasUsageProbeError: store.accountList?.usageProbeError != nil,
			hasFixedSelection: hasFixedSelection,
			emailsHidden: emailsHidden,
			needsScrolling: accountListNeedsScrolling
		)
	}
}
