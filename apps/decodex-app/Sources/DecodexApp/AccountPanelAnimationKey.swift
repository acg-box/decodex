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
