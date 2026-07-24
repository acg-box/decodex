import Foundation
import SwiftUI

struct AccountUsageSummaryView: View {
	let account: CodexAccount
	let usageRefillAnimation: AccountUsageRefillAnimation?
	let prepareResetCredit: (ResetCreditUsePreparation) async -> String?
	let consumeResetCredit: (ResetCreditUseAttempt) async -> Bool

	var body: some View {
		TimelineView(.periodic(from: Date(), by: 30)) { timeline in
			VStack(spacing: 5) {
				if account.hasProfileSummary {
					AccountProfileSummaryView(account: account)
						.transition(.panelInline)
				}

				if account.hasResetCreditsSummary {
					AccountResetCreditsSummaryView(
						account: account,
						prepareResetCredit: prepareResetCredit,
						consumeResetCredit: consumeResetCredit
					)
						.transition(.panelInline)
				}

				if account.hasPrimaryUsageData {
					AccountUsageMeterView(
						label: account.windowLabel(seconds: account.primaryWindowSeconds),
						remainingPercent: account.primaryRemainingPercent,
						resetAtUnixEpoch: account.primaryResetsAtUnixEpoch,
						dailyAveragePercent: account.sevenDayAveragePercent(
							forWindowSeconds: account.primaryWindowSeconds
						),
						tone: account.usageTone(remainingPercent: account.primaryRemainingPercent),
						currentTime: timeline.date,
						refillAnimation: usageRefillAnimation?.meterAnimation(
							for: .primary,
							currentPercent: account.primaryRemainingPercent
						)
					)
					.transition(.panelInline)
				}

				if account.hasSecondaryUsageData {
					AccountUsageMeterView(
						label: account.windowLabel(seconds: account.secondaryWindowSeconds),
						remainingPercent: account.secondaryRemainingPercent,
						resetAtUnixEpoch: account.secondaryResetsAtUnixEpoch,
						dailyAveragePercent: account.sevenDayAveragePercent(
							forWindowSeconds: account.secondaryWindowSeconds
						),
						tone: account.usageTone(remainingPercent: account.secondaryRemainingPercent),
						currentTime: timeline.date,
						refillAnimation: usageRefillAnimation?.meterAnimation(
							for: .secondary,
							currentPercent: account.secondaryRemainingPercent
						)
					)
					.transition(.panelInline)
				}
			}
			.frame(maxWidth: .infinity)
			.padding(.horizontal, 1)
			.padding(.vertical, 1)
			.animation(PanelMotion.inlineLayout, value: account.hasProfileSummary)
			.animation(PanelMotion.inlineLayout, value: account.hasResetCreditsSummary)
			.animation(PanelMotion.inlineLayout, value: account.hasPrimaryUsageData)
			.animation(PanelMotion.inlineLayout, value: account.hasSecondaryUsageData)
		}
	}
}

struct AccountResetCreditsSummaryView: View {
	let account: CodexAccount
	let prepareResetCredit: (ResetCreditUsePreparation) async -> String?
	let consumeResetCredit: (ResetCreditUseAttempt) async -> Bool
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .center, spacing: 5) {
			PanelMetricIconView(
				symbol: "arrow.counterclockwise.circle",
				tint: PanelPalette.routeAccent(colorScheme).opacity(0.84)
			)
				.accessibilityHidden(true)

			Text(summaryText)
				.font(PanelFont.usageLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.86))
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)
				.accessibilityHidden(true)

			AccountResetCreditExpiryStripView(
				account: account,
				prepareResetCredit: prepareResetCredit,
				consumeResetCredit: consumeResetCredit
			)
		}
		.frame(minHeight: 18)
		.accessibilityElement(children: .contain)
		.accessibilityLabel("Reset cards, \(summaryText)")
	}

	private var summaryText: String {
		let count = account.visibleResetCreditCount ?? account.availableResetCredits.count
		return "\(count) reset"
	}

}

struct AccountResetCreditExpiryStripView: View {
	private static let confirmationWindowSeconds = 5

	let account: CodexAccount
	let prepareResetCredit: (ResetCreditUsePreparation) async -> String?
	let consumeResetCredit: (ResetCreditUseAttempt) async -> Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var placementStore = AccountRunStripPlacementStore()
	@State private var scrollProxy = AccountRunStripScrollProxy()
	@State private var scrollMetrics = AccountRunStripMetrics()
	@State private var showsEdgeControls = false
	@State private var confirmation = ResetCreditUseConfirmation()
	@State private var confirmationSecondsRemaining = 0

	var body: some View {
		HStack(spacing: AccountRunStripLayout.edgeControlSpacing) {
			if showsEdgeControls {
				edgeButton(.backward)
					.transition(.panelInline)
			}

			AccountRunStripScrollView(
				placementStore: placementStore,
				scrollProxy: scrollProxy,
				allowsPointerPanning: false,
				onMetricsChange: { metrics in
					updateScrollMetrics(metrics)
				}
			) {
				HStack(spacing: 4) {
					ForEach(Array(account.availableResetCredits.enumerated()), id: \.offset) { index, credit in
						let target = resetCreditTargets[index]

						Button {
							tapResetCredit(target)
						} label: {
							resetCreditChip(
								formatResetCreditDate(credit.expiresAtUnixEpoch),
								target: target
							)
						}
							.buttonStyle(.plain)
							.modifier(
								AccountRunChipPlacementReporter(
									runID: "reset-\(index)",
									placementStore: placementStore
								)
							)
							.disabled(confirmation.isBusy)
							.accessibilityLabel(resetCreditAccessibilityLabel(credit, target: target))
							.accessibilityHint(resetCreditAccessibilityHint(target))
							.help(resetCreditHelp(credit, target: target))
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
				edgeButton(.forward)
					.transition(.panelInline)
			}
		}
		.frame(height: AccountRunChipLayout.height)
		.frame(maxWidth: .infinity, alignment: .leading)
		.onAppear {
			placementStore.retainOnly(resetCreditIDs)
			confirmation.retainOnly(Set(resetCreditTargets))
		}
		.onDisappear {
			confirmation.cancelPendingConfirmation()
			confirmationSecondsRemaining = 0
		}
		.onChange(of: resetCreditIDs) { _, ids in
			placementStore.retainOnly(ids)
		}
		.onChange(of: resetCreditTargets) { _, targets in
			confirmation.retainOnly(Set(targets))
		}
		.task(id: confirmationCountdownAttempt) {
			await runConfirmationCountdown(for: confirmationCountdownAttempt)
		}
		.animation(PanelMotion.inlineLayout, value: showsEdgeControls)
	}

	private var resetCreditTargets: [ResetCreditUseTarget] {
		ResetCreditUseTarget.makeTargets(
			accountID: account.accountFingerprint,
			reportedAvailableCount: account.resetCreditsAvailableCount,
			credits: account.availableResetCredits
		)
	}

	private var resetCreditIDs: Set<String> {
		Set(account.availableResetCredits.indices.map { "reset-\($0)" })
	}

	private var confirmationCountdownAttempt: ResetCreditUseAttempt? {
		confirmation.isSubmitting ? nil : confirmation.armedAttempt
	}

	private func edgeButton(_ direction: AccountRunStripScrollDirection) -> some View {
		AccountRunStripEdgeButton(
			direction: direction,
			isEnabled: direction == .backward
				? scrollMetrics.canScrollBackward
				: scrollMetrics.canScrollForward,
			accessibilityLabel: direction == .backward ? "Previous reset card" : "Next reset card",
			disabledHelp: direction == .backward
				? "Already at the first reset card"
				: "Already at the last reset card"
		) {
			scrollProxy.scrollToAdjacentRun(direction)
		} startContinuousAction: {
			scrollProxy.startContinuousScroll(direction)
		} stopContinuousAction: {
			scrollProxy.stopContinuousScroll()
		}
	}

	private func resetCreditChip(
		_ expiryText: String,
		target: ResetCreditUseTarget
	) -> some View {
		Text(resetCreditChipText(expiryText, target: target))
			.font(PanelFont.usageValue)
			.foregroundStyle(resetCreditChipForeground(target))
			.monospacedDigit()
			.lineLimit(1)
			.fixedSize(horizontal: true, vertical: false)
			.padding(.horizontal, 4)
			.padding(.vertical, 1)
			.background(resetCreditChipBackground(target))
			.contentShape(Rectangle())
	}

	private func resetCreditChipText(
		_ expiryText: String,
		target: ResetCreditUseTarget
	) -> String {
		if confirmation.isPreparing(target) {
			return "Preparing"
		}
		if confirmation.isSubmitting(target) {
			return "Using"
		}
		if confirmation.isArmed(target) {
			let seconds = confirmationSecondsRemaining > 0
				? confirmationSecondsRemaining
				: Self.confirmationWindowSeconds
			return "Confirm Use · \(seconds)s"
		}

		return expiryText
	}

	private func resetCreditChipForeground(_ target: ResetCreditUseTarget) -> Color {
		if confirmation.isPreparing(target) || confirmation.isArmed(target) {
			return PanelPalette.warning(colorScheme)
		}

		return PanelPalette.primaryText(colorScheme).opacity(0.88)
	}

	private func resetCreditChipBackground(_ target: ResetCreditUseTarget) -> Color {
		if confirmation.isPreparing(target) || confirmation.isArmed(target) {
			return PanelPalette.warning(colorScheme).opacity(colorScheme == .dark ? 0.2 : 0.14)
		}

		return PanelPalette.routeAccent(colorScheme).opacity(colorScheme == .dark ? 0.16 : 0.11)
	}

	private func resetCreditHelp(
		_ credit: AccountResetCredit,
		target: ResetCreditUseTarget
	) -> String {
		let expiry = formatResetCreditDate(credit.expiresAtUnixEpoch)
		if confirmation.isPreparing {
			return confirmation.isPreparing(target)
				? "Preparing reset card that expires \(expiry)"
				: "Wait until the current reset-card request finishes."
		}
		if confirmation.isSubmitting {
			return confirmation.isSubmitting(target)
				? "Using reset card that expires \(expiry)"
				: "Wait until the current reset-card request finishes."
		}
		if confirmation.isArmed(target) {
			return "Click again within \(Self.confirmationWindowSeconds) seconds to use the reset card that expires \(expiry). Otherwise, confirmation cancels automatically."
		}

		return "Expires \(expiry). Click once to prepare this reset card."
	}

	private func resetCreditAccessibilityLabel(
		_ credit: AccountResetCredit,
		target: ResetCreditUseTarget
	) -> String {
		let expiry = formatResetCreditDate(credit.expiresAtUnixEpoch)
		if confirmation.isPreparing(target) {
			return "Preparing reset card that expires \(expiry)"
		}
		if confirmation.isSubmitting(target) {
			return "Using reset card that expires \(expiry)"
		}
		if confirmation.isArmed(target) {
			let seconds = confirmationSecondsRemaining > 0
				? confirmationSecondsRemaining
				: Self.confirmationWindowSeconds
			return "Confirm use of reset card that expires \(expiry), \(seconds) seconds remaining"
		}

		return "Reset card, expires \(expiry)"
	}

	private func resetCreditAccessibilityHint(_ target: ResetCreditUseTarget) -> String {
		if confirmation.isPreparing {
			return confirmation.isPreparing(target)
				? "The reset card is being prepared for confirmation."
				: "Wait until the current reset-card request finishes."
		}
		if confirmation.isSubmitting {
			return confirmation.isSubmitting(target)
				? "The reset-card request is in progress."
				: "Wait until the current reset-card request finishes."
		}

		return confirmation.isArmed(target)
			? "Activate again to use this reset card. Confirmation cancels automatically after \(Self.confirmationWindowSeconds) seconds."
			: "Activate once to prepare this reset card."
	}

	@MainActor
	private func runConfirmationCountdown(
		for attempt: ResetCreditUseAttempt?
	) async {
		guard let attempt else {
			confirmationSecondsRemaining = 0
			return
		}

		let clock = ContinuousClock()
		let deadline = clock.now.advanced(
			by: .seconds(Self.confirmationWindowSeconds)
		)
		confirmationSecondsRemaining = Self.confirmationWindowSeconds

		while clock.now < deadline {
			let nextWake = min(
				clock.now.advanced(by: .seconds(1)),
				deadline
			)
			do {
				try await clock.sleep(until: nextWake)
			} catch {
				return
			}
			guard Task.isCancelled == false,
				confirmation.isArmed(attempt),
				confirmation.isSubmitting == false
			else {
				return
			}

			confirmationSecondsRemaining = Self.roundedUpSeconds(
				clock.now.duration(to: deadline)
			)
		}
		guard Task.isCancelled == false,
			confirmation.isArmed(attempt),
			confirmation.isSubmitting == false
		else {
			return
		}

		confirmation.disarm(attempt)
		confirmationSecondsRemaining = 0
	}

	private static func roundedUpSeconds(_ duration: Duration) -> Int {
		let components = duration.components
		guard components.seconds >= 0 else {
			return 0
		}

		return Int(components.seconds) + (components.attoseconds > 0 ? 1 : 0)
	}

	private func tapResetCredit(_ target: ResetCreditUseTarget) {
		guard let action = confirmation.tap(target) else {
			return
		}

		switch action {
		case .prepare(let preparation):
			Task {
				let creditID = await prepareResetCredit(preparation)
				confirmation.finishPreparation(preparation, creditID: creditID)
			}
		case .consume(let attempt):
			Task {
				let resolved = await consumeResetCredit(attempt)
				confirmation.finish(attempt, resolved: resolved)
			}
		}
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

func formatResetCreditDate(
	_ seconds: Int?,
	timeZone: TimeZone = .autoupdatingCurrent
) -> String {
	guard let seconds, seconds > 0 else {
		return "-"
	}
	let date = Date(timeIntervalSince1970: TimeInterval(seconds))
	guard date.timeIntervalSince1970.isFinite else {
		return "-"
	}

	let formatter = DateFormatter()
	formatter.locale = Locale(identifier: "en_US_POSIX")
	formatter.timeZone = timeZone
	formatter.dateFormat = "MMM d HH:mm"
	return formatter.string(from: date)
}
