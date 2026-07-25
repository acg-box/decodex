import AppKit
import Foundation
import SwiftUI

struct AccountPanelView: View {
	let store: AccountStore
	let loginWindowState: LoginWindowState
	@Environment(\.colorScheme) var colorScheme
	@State var accountScrollOffset: CGFloat = 0
	@State var armedLogoutAccountID: String?
	@State var deletingLogoutAccountID: String?
	@State var logoutErrorMessage: String?
	@State var measuredAccountListContentHeight: CGFloat = 0
	@State var panelScreenVisibleFrame: CGRect?
	@AppStorage("decodex.operator.accountPrivacy") var accountPrivacy = AccountPrivacy.hiddenValue

	var body: some View {
		GlassEffectContainer(spacing: 6) {
			panelContent
		}
		.background {
			LoginPanelPresenter(
				store: store,
				state: loginWindowState,
				isPresented: loginWindowState.isPresented,
				mode: loginWindowState.mode
			)
			.frame(width: 0, height: 0)
		}
		.onDisappear {
			disarmLogout()
		}
	}

	private var panelContent: some View {
		return VStack(alignment: .leading, spacing: 6) {
			header
			accountSummary

			if telemetryMatrixIsVisible {
				AccountTelemetryMatrixView(
					aggregate: accountProfileAggregate,
					usageEstimate: store.accountList?.usageEstimate,
					accounts: store.accounts
				)
				.transition(.panelSection)
			}

			if let notice = store.notice {
				NoticeView(notice: notice, onDismiss: store.clearNotice)
					.id(notice.id)
					.transition(.panelSection)
			}

			if let usageProbeError = store.accountList?.usageProbeError {
				NoticeView(
					notice: .error(
						"Some usage data is unavailable",
						details: usageProbeError
					)
				)
					.transition(.panelSection)
			}

			Group {
				if store.isInitialLoading {
					loadingState
				} else if store.accounts.isEmpty {
					emptyState
				} else {
					accountList
				}
			}
			.transition(.panelSection)
		}
		.frame(width: 322)
		.padding(9)
		.modernGlassSurface(
			cornerRadius: 18,
			depth: .panel
		)
		.controlSize(.small)
		.symbolRenderingMode(.hierarchical)
		.animation(PanelMotion.panelLayout, value: panelAnimationKey)
		.sizesPanelWindowToContent { visibleFrame in
			if panelScreenVisibleFrame != visibleFrame {
				panelScreenVisibleFrame = visibleFrame
			}
		}
	}

	private var accountList: some View {
		ScrollView(.vertical, showsIndicators: false) {
			accountRows
				.background(accountScrollProbe)
				.background(accountRowsHeightProbe)
		}
		.coordinateSpace(name: AccountPanelLayout.accountListScrollSpace)
		.frame(height: accountListViewportHeight)
		.overlay(alignment: .trailing) {
			AccountListScrollIndicatorView(
				contentHeight: accountListContentHeight,
				viewportHeight: accountListViewportHeight,
				scrollOffset: accountScrollOffset
			)
			.padding(.trailing, 1)
		}
		.onPreferenceChange(AccountScrollOffsetPreferenceKey.self) { minY in
			let maxOffset = max(0, accountListContentHeight - accountListViewportHeight)
			accountScrollOffset = min(max(0, -minY), maxOffset)
		}
		.onPreferenceChange(AccountRowsHeightPreferenceKey.self) { height in
			let measuredHeight = ceil(height)
			if abs(measuredAccountListContentHeight - measuredHeight) > 0.5 {
				measuredAccountListContentHeight = measuredHeight
			}
		}
		.onChange(of: accountListNeedsScrolling) { _, needsScrolling in
			if needsScrolling == false {
				accountScrollOffset = 0
			}
		}
	}

	private var accountRows: some View {
		let accounts = store.accounts

		return VStack(spacing: 0) {
			ForEach(Array(accounts.enumerated()), id: \.element.id) { index, account in
				let runs = operatorCurrentLaneCards(for: account)

				AccountRowView(
					account: account,
					runs: runs,
					displayName: displayName(for: account),
					showsDivider: index < accounts.count - 1,
					isLogoutArmed: armedLogoutAccountID == account.id,
					isLogoutPending: deletingLogoutAccountID == account.id,
					logoutErrorMessage: armedLogoutAccountID == account.id ? logoutErrorMessage : nil,
					usageRefillAnimation: store.usageRefillAnimations[account.accountFingerprint],
					useInCodex: {
						Task {
							await store.useInCodex(account)
						}
					},
					consumeResetCredit: { attempt in
						await store.consumeResetCredit(attempt, for: account)
					},
					routeRunsHere: {
						Task {
							await store.select(account)
						}
					},
					login: {
						presentLogin(.account(displayName(for: account)))
					},
					logout: {
						requestLogout(account)
					},
					cancelLogout: {
						disarmLogout()
					},
					confirmLogout: {
						confirmLogout(account)
					}
				)
				.transition(.accountRowRemoval)
			}
		}
		.animation(PanelMotion.accountRemoval, value: accounts.map(\.id))
	}
}
