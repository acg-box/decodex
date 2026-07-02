import SwiftUI

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

	var accountListContentHeight: CGFloat {
		store.accounts.reduce(CGFloat(1)) { total, account in
			total + accountRowHeight(for: account)
		}
	}

	var accountListViewportHeight: CGFloat {
		min(accountListContentHeight, accountListAvailableHeight)
	}

	var accountListNeedsScrolling: Bool {
		accountListContentHeight > accountListAvailableHeight + 1
	}

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

	var accountListAvailableHeight: CGFloat {
		let visibleHeight = AccountPanelLayout.activeScreenVisibleHeight()
		let availableHeight = visibleHeight - accountPanelChromeHeight
		let minimumHeight = min(
			AccountPanelLayout.minimumScrollableListHeight,
			max(140, visibleHeight * 0.42)
		)

		return max(minimumHeight, availableHeight)
	}

	var accountPanelChromeHeight: CGFloat {
		var height = AccountPanelLayout.screenVerticalMargin
			+ AccountPanelLayout.panelVerticalPadding
			+ AccountPanelLayout.headerHeight
			+ AccountPanelLayout.accountSummaryHeight
			+ AccountPanelLayout.sectionSpacing

		if telemetryMatrixIsVisible {
			height += AccountPanelLayout.sectionSpacing + telemetryMatrixHeight
		}
		if store.notice != nil {
			height += AccountPanelLayout.sectionSpacing + AccountPanelLayout.noticeHeight
		}
		if store.accountList?.usageProbeError != nil {
			height += AccountPanelLayout.sectionSpacing + AccountPanelLayout.noticeHeight
		}

		return height
	}

	var accountScrollProbe: some View {
		GeometryReader { proxy in
			Color.clear.preference(
				key: AccountScrollOffsetPreferenceKey.self,
				value: proxy.frame(in: .named(AccountPanelLayout.accountListScrollSpace)).minY
			)
		}
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

struct AccountPanelAnimationKey: Equatable {
	let accountIDs: [String]
	let isInitialLoading: Bool
	let hasAccounts: Bool
	let hasTelemetry: Bool
	let hasNotice: Bool
	let hasUsageProbeError: Bool
	let hasFixedSelection: Bool
	let emailsHidden: Bool
	let needsScrolling: Bool
}
