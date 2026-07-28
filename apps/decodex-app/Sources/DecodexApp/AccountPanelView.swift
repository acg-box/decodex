import AppKit
import SwiftUI

struct AccountPanelView: View {
	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme
	@State private var panelScreenVisibleFrame: CGRect?

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
			ScrollView(.vertical, showsIndicators: true) {
				LazyVStack(alignment: .leading, spacing: AccountPanelLayout.accountRowSpacing) {
					ForEach(store.accounts) { state in
						ResetCardAccountRow(
							state: state,
							store: store
						)
					}
				}
				.padding(.trailing, 2)
			}
			.frame(
				height: AccountPanelLayout.accountListHeight(
					accountCount: store.accounts.count,
					windowVisibleFrame: panelScreenVisibleFrame
				)
			)
			.accessibilityLabel("Decodex accounts")
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
