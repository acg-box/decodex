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
