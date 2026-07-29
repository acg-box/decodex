import AppKit
import SwiftUI

struct AccountPanelView: View {
	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme
	@State private var panelScreenVisibleFrame: CGRect?
	@State private var measuredAccountListContentHeight: CGFloat = 0
	@State private var accountScrollOffset: CGFloat = 0

	var body: some View {
		GlassEffectContainer(spacing: 6) {
			VStack(alignment: .leading, spacing: 7) {
				header

				if let message = store.message {
					ResetCardMessageView(message: message) {
						store.dismissMessage()
					}
					.transition(.panelSection)
				}

				if store.pendingAttempts.isEmpty == false {
					ResetCardPendingAttemptsView(store: store)
						.transition(.panelSection)
				}

				accountContent
			}
			.frame(width: AccountPanelLayout.panelWidth)
			.padding(9)
			.modernGlassSurface(cornerRadius: 18, depth: .panel)
			.controlSize(.small)
			.symbolRenderingMode(.hierarchical)
			.animation(PanelMotion.panelLayout, value: store.accounts.map(\.id))
			.sizesPanelWindowToContent { visibleFrame in
				if panelScreenVisibleFrame != visibleFrame {
					panelScreenVisibleFrame = visibleFrame
				}
			}
		}
		// Re-key the singleton panel, rather than every repeated glass row, when
		// system appearance changes.
		.id(colorScheme == .dark ? "account-panel-dark" : "account-panel-light")
	}

	private var header: some View {
		HStack(alignment: .center, spacing: 8) {
			Image(nsImage: AppAssets.statusBarIcon)
				.resizable()
				.renderingMode(.template)
				.scaledToFit()
				.foregroundStyle(PanelPalette.actionBlue(colorScheme))
				.frame(width: 20, height: 20)
				.frame(width: 28, height: 28)
				.accessibilityHidden(true)

			VStack(alignment: .leading, spacing: 2) {
				Text("Decodex")
					.font(PanelFont.headerTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
				Text(headerSubtitle)
					.font(PanelFont.headerSubtitle)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
			}
			.layoutPriority(1)

			Spacer(minLength: 4)

			PanelIconButtonView(
				symbol: "arrow.clockwise",
				tint: PanelPalette.secondaryText(colorScheme),
				isActive: false,
				isDisabled: store.isRefreshing || store.submittingKey != nil,
				isSubtle: true,
				size: 22,
				action: {
					Task {
						await store.refresh()
					}
				},
				help: "Refresh accounts and Reset Cards"
			)

			PanelIconButtonView(
				symbol: "power",
				tint: PanelPalette.secondaryText(colorScheme),
				isActive: false,
				isSubtle: true,
				size: 22,
				action: {
					NSApplication.shared.terminate(nil)
				},
				help: "Quit Decodex"
			)
		}
		.padding(.horizontal, 2)
	}

	private var headerSubtitle: String {
		if store.isInitialLoading, store.accounts.isEmpty {
			return "Loading accounts"
		}
		let count = store.accounts.count
		return "\(count) account\(count == 1 ? "" : "s") · usage and Reset Cards"
	}

	@ViewBuilder
	private var accountContent: some View {
		if store.accounts.isEmpty {
			emptyOrLoadingState
		} else {
			ScrollView(.vertical, showsIndicators: false) {
				VStack(alignment: .leading, spacing: 0) {
					ForEach(Array(store.accounts.enumerated()), id: \.element.id) { index, state in
						ResetCardAccountRow(
							state: state,
							store: store
						)

						if index < store.accounts.count - 1 {
							Rectangle()
								.fill(PanelPalette.separator(colorScheme))
								.frame(height: 0.5)
								.padding(.horizontal, 7)
								.allowsHitTesting(false)
						}
					}
				}
				.background(accountScrollProbe)
				.background(accountRowsHeightProbe)
			}
			.coordinateSpace(name: AccountPanelLayout.accountListScrollSpace)
			.frame(
				height: accountListViewportHeight
			)
			.overlay(alignment: .trailing) {
				AccountListScrollIndicatorView(
					contentHeight: accountListContentHeight,
					viewportHeight: accountListViewportHeight,
					scrollOffset: accountScrollOffset
				)
				.padding(.trailing, 1)
			}
			.onPreferenceChange(AccountScrollOffsetPreferenceKey.self) { minY in
				let maximumOffset = max(0, accountListContentHeight - accountListViewportHeight)
				accountScrollOffset = min(max(0, -minY), maximumOffset)
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
			.accessibilityLabel("Decodex accounts")
		}
	}

	private var accountListContentHeight: CGFloat {
		AccountPanelLayout.resolvedAccountListContentHeight(
			measured: measuredAccountListContentHeight,
			estimated: CGFloat(max(1, store.accounts.count))
				* AccountPanelLayout.estimatedAccountRowHeight
		)
	}

	private var accountListViewportHeight: CGFloat {
		AccountPanelLayout.accountListHeight(
			accountCount: store.accounts.count,
			measuredContentHeight: measuredAccountListContentHeight,
			windowVisibleFrame: panelScreenVisibleFrame
		)
	}

	private var accountListNeedsScrolling: Bool {
		accountListContentHeight > accountListViewportHeight + 1
	}

	private var accountScrollProbe: some View {
		GeometryReader { proxy in
			Color.clear.preference(
				key: AccountScrollOffsetPreferenceKey.self,
				value: proxy.frame(in: .named(AccountPanelLayout.accountListScrollSpace)).minY
			)
		}
	}

	private var accountRowsHeightProbe: some View {
		GeometryReader { proxy in
			Color.clear.preference(
				key: AccountRowsHeightPreferenceKey.self,
				value: proxy.size.height
			)
		}
	}

	private var emptyOrLoadingState: some View {
		HStack(alignment: .center, spacing: 8) {
			if store.isInitialLoading {
				ProgressView()
					.controlSize(.small)
			} else {
				Image(systemName: store.hasLoaded ? "person.2.slash" : "bolt.horizontal.circle")
					.font(PanelFont.emptyIcon)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			}

			VStack(alignment: .leading, spacing: 2) {
				Text(store.isInitialLoading ? "Loading accounts" : "No accounts")
					.font(PanelFont.emptyTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
				Text(
					store.hasLoaded
						? "Import an account with the Decodex CLI, then refresh."
						: "The account service has not returned a complete list."
				)
				.font(PanelFont.emptyBody)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.fixedSize(horizontal: false, vertical: true)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(9)
		.modernGlassSurface(cornerRadius: 9, depth: .row)
	}
}
