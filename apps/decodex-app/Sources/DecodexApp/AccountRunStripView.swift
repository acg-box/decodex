import Foundation
import SwiftUI

struct AccountRunSummaryView: View {
	let runs: [OperatorCurrentLaneCard]
	@State private var placementStore = AccountRunStripPlacementStore()
	@State private var scrollProxy = AccountRunStripScrollProxy()
	@State private var scrollMetrics = AccountRunStripMetrics()
	@State private var showsEdgeControls = false

	var body: some View {
		HStack(spacing: AccountRunStripLayout.edgeControlSpacing) {
			if showsEdgeControls {
				AccountRunStripEdgeButton(
					direction: .backward,
					isEnabled: scrollMetrics.canScrollBackward
				) {
					scrollProxy.scrollToAdjacentRun(.backward)
				} startContinuousAction: {
					scrollProxy.startContinuousScroll(.backward)
				} stopContinuousAction: {
					scrollProxy.stopContinuousScroll()
				}
				.transition(.panelInline)
			}

			AccountRunStripScrollView(
				placementStore: placementStore,
				scrollProxy: scrollProxy,
				onMetricsChange: { metrics in
					updateScrollMetrics(metrics)
				}
			) {
				HStack(spacing: 5) {
					ForEach(runs) { card in
						AccountRunChipView(card: card)
							.modifier(
								AccountRunChipPlacementReporter(
									runID: card.id,
									placementStore: placementStore
								)
							)
					}
				}
				.padding(.trailing, 1)
				.fixedSize(horizontal: true, vertical: false)
				.coordinateSpace(name: AccountRunStripLayout.contentCoordinateSpace)
			}
			.mask {
				AccountRunStripFadeMask(metrics: showsEdgeControls ? scrollMetrics : AccountRunStripMetrics())
			}
			.frame(maxWidth: .infinity, alignment: .leading)

			if showsEdgeControls {
				AccountRunStripEdgeButton(
					direction: .forward,
					isEnabled: scrollMetrics.canScrollForward
				) {
					scrollProxy.scrollToAdjacentRun(.forward)
				} startContinuousAction: {
					scrollProxy.startContinuousScroll(.forward)
				} stopContinuousAction: {
					scrollProxy.stopContinuousScroll()
				}
				.transition(.panelInline)
			}
		}
		.frame(height: AccountRunChipLayout.height)
		.frame(maxWidth: .infinity, alignment: .leading)
		.contentShape(Rectangle())
		.accessibilityLabel("\(runs.count) current lane\(runs.count == 1 ? "" : "s")")
		.onAppear {
			placementStore.retainOnly(Set(runs.map(\.id)))
		}
		.onChange(of: runs.map(\.id)) { _, runIDs in
			placementStore.retainOnly(Set(runIDs))
		}
		.animation(PanelMotion.inlineLayout, value: showsEdgeControls)
	}

	private func updateScrollMetrics(_ metrics: AccountRunStripMetrics) {
		let nextShowsEdgeControls = shouldShowEdgeControls(for: metrics)
		guard metrics != scrollMetrics || nextShowsEdgeControls != showsEdgeControls else {
			return
		}

		if showsEdgeControls && nextShowsEdgeControls == false {
			scrollProxy.stopContinuousScroll()
		}

		if metrics != scrollMetrics {
			var transaction = Transaction()
			transaction.disablesAnimations = true
			withTransaction(transaction) {
				scrollMetrics = metrics
			}
		}

		if nextShowsEdgeControls != showsEdgeControls {
			withAnimation(PanelMotion.inlineLayout) {
				showsEdgeControls = nextShowsEdgeControls
			}
		}
	}

	private func shouldShowEdgeControls(for metrics: AccountRunStripMetrics) -> Bool {
		let reservedWidth = showsEdgeControls ? AccountRunStripLayout.edgeControlReservedWidth : 0
		let viewportWidthWithoutEdgeControls = metrics.viewportWidth + reservedWidth

		return metrics.contentWidth > viewportWidthWithoutEdgeControls + AccountRunStripLayout.overflowTolerance
	}
}
