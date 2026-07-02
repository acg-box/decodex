import SwiftUI

extension AccountPanelView {
	var header: some View {
		HStack(alignment: .center, spacing: 8) {
			Image(nsImage: AppAssets.statusBarIcon)
				.resizable()
				.renderingMode(.template)
				.scaledToFit()
				.foregroundStyle(PanelPalette.actionBlue(colorScheme))
				.frame(width: 20, height: 20)
				.frame(width: 28, height: 28)

			VStack(alignment: .leading, spacing: 2) {
				Text("Decodex")
					.font(PanelFont.headerTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
				Text(headerSubtitle)
					.font(PanelFont.headerSubtitle)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.minimumScaleFactor(0.9)
			}
			.layoutPriority(1)

			Spacer(minLength: 4)

			HStack(spacing: 5) {
				PanelIconButtonView(
					symbol: emailsHidden ? "eye.slash" : "eye",
					tint: PanelPalette.secondaryText(colorScheme),
					isActive: false,
					action: {
						withAnimation(PanelMotion.inlineLayout) {
							accountPrivacy = emailsHidden ? AccountPrivacy.visibleValue : AccountPrivacy.hiddenValue
						}
					},
					help: emailsHidden ? "Show account emails" : "Hide account emails"
				)

				PanelIconButtonView(
					symbol: store.fastModeEnabled ? "bolt.fill" : "bolt",
					tint: PanelPalette.fastModeAccent(colorScheme),
					isActive: store.fastModeEnabled,
					isDisabled: store.isSettingFastMode,
					size: 25,
					action: {
						Task {
							await store.setFastMode(store.fastModeEnabled == false)
						}
					},
					help: store.fastModeEnabled ? "Turn fast mode off" : "Turn fast mode on"
				)

				PanelIconButtonView(
					symbol: "safari",
					tint: PanelPalette.actionBlue(colorScheme),
					isActive: false,
					action: {
						Task {
							await store.openWebUI()
						}
					},
					help: "Open Decodex WebUI"
				)

				PanelIconButtonView(
					symbol: "plus",
					tint: PanelPalette.actionBlue(colorScheme),
					isActive: false,
					isPrimary: true,
					size: 25,
					action: {
						presentLogin(.newAccount)
					},
					help: "Add login"
				)
			}
		}
		.animation(PanelMotion.state, value: hasFixedSelection)
	}

	var accountSummary: some View {
		HStack(alignment: .firstTextBaseline, spacing: 7) {
			SummaryTileView(
				title: "Codex",
				value: codexAuthLabel,
				symbol: "terminal",
				tint: PanelPalette.codexAccent(colorScheme)
			)

			Rectangle()
				.fill(PanelPalette.separator(colorScheme))
				.frame(width: 0.5, height: 16)
				.alignmentGuide(.firstTextBaseline) { dimensions in
					dimensions[VerticalAlignment.center] + 4
				}

			SummaryTileView(
				title: "Runs",
				value: decodexModeLabel,
				symbol: "arrow.triangle.branch",
				tint: hasFixedSelection ? PanelPalette.actionBlue(colorScheme) : PanelPalette.secondaryText(colorScheme)
			)
		}
		.padding(.horizontal, 3)
		.padding(.top, 1)
		.padding(.bottom, 4)
		.overlay(alignment: .bottom) {
			Rectangle()
				.fill(PanelPalette.separator(colorScheme).opacity(colorScheme == .dark ? 0.72 : 0.9))
				.frame(height: 0.5)
				.allowsHitTesting(false)
		}
	}

	var emptyState: some View {
		VStack(alignment: .leading, spacing: 6) {
			Image(systemName: "person.crop.circle.badge.plus")
				.font(PanelFont.emptyIcon)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			Text("No accounts in the local pool")
				.font(PanelFont.emptyTitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
			Text("Add a ChatGPT login before switching the Codex auth file.")
				.font(PanelFont.emptyBody)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.fixedSize(horizontal: false, vertical: true)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(8)
		.modernGlassSurface(cornerRadius: 9, depth: .row)
	}

	var loadingState: some View {
		HStack(spacing: 7) {
			ProgressView()
				.controlSize(.small)
			Text("Loading accounts")
				.font(PanelFont.emptyTitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
			Spacer()
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(8)
		.modernGlassSurface(cornerRadius: 9, depth: .row)
	}
}
