import SwiftUI

struct ResetCardMessageView: View {
	let message: ResetCardStoreMessage
	let dismiss: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 6) {
			Image(systemName: symbol)
				.font(PanelFont.tertiary)
				.foregroundStyle(color)
				.accessibilityHidden(true)

			Text(message.text)
				.font(PanelFont.tertiary)
				.foregroundStyle(color)
				.fixedSize(horizontal: false, vertical: true)

			Spacer(minLength: 2)

			Button(action: dismiss) {
				Image(systemName: "xmark")
					.font(PanelFont.tertiary)
			}
			.buttonStyle(.plain)
			.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			.help("Dismiss message")
		}
		.padding(.horizontal, 8)
		.padding(.vertical, 6)
		.modernGlassSurface(cornerRadius: 8, depth: .row)
	}

	private var symbol: String {
		switch message.tone {
		case .information:
			return "info.circle"
		case .success:
			return "checkmark.circle"
		case .error:
			return "exclamationmark.triangle"
		}
	}

	private var color: Color {
		switch message.tone {
		case .information:
			return PanelPalette.secondaryText(colorScheme)
		case .success:
			return PanelPalette.routeAccent(colorScheme)
		case .error:
			return PanelPalette.destructive(colorScheme)
		}
	}
}

struct ResetCardPendingAttemptsView: View {
	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 5) {
			ForEach(store.pendingAttempts, id: \.idempotencyKey) { attempt in
				HStack(spacing: 6) {
					Image(systemName: "clock.arrow.circlepath")
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.warning(colorScheme))
						.accessibilityHidden(true)

					Text(
						"\(store.accountLabel(for: attempt.target.accountID)) · pending …\(attempt.idempotencyKey.suffix(8))"
					)
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)

					Spacer(minLength: 2)

					Button("Resume") {
						Task {
							await store.resume(attempt)
						}
					}
					.buttonStyle(.borderless)
					.controlSize(.mini)
					.disabled(store.submittingKey != nil)
					.help(
						store.isPendingRecoveryBlocked
							? "Read durable status only. Repair the recovery journal before any retry."
							: "Read durable status and use the same operation key if the command is absent."
					)
				}
			}
		}
		.padding(.horizontal, 8)
		.padding(.vertical, 6)
		.modernGlassSurface(cornerRadius: 8, depth: .row)
	}
}

struct ResetCardAccountRow: View {
	private static let confirmationWindowSeconds = 5

	let state: ResetCardAccountState
	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme
	@State private var confirmation = ResetCardUseConfirmation()
	@State private var confirmationSecondsRemaining = 0

	var body: some View {
		VStack(alignment: .leading, spacing: 4) {
			accountHeader
			quotaWindows
			cardInventory
		}
		.padding(.horizontal, 7)
		.padding(.vertical, 5)
		.onAppear {
			confirmation.retainOnly(Set(state.targets))
		}
		.onChange(of: state.targets) { _, targets in
			confirmation.retainOnly(Set(targets))
		}
		.onDisappear {
			confirmation.cancelPendingConfirmation()
			confirmationSecondsRemaining = 0
		}
		.task(id: countdownAttempt) {
			await runConfirmationCountdown(for: countdownAttempt)
		}
	}

	private var accountHeader: some View {
		HStack(alignment: .center, spacing: 5) {
			Text(state.account.displayLabel)
				.font(PanelFont.accountName)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
				.lineLimit(1)
				.truncationMode(.middle)
				.layoutPriority(1)
				.help("Account \(state.account.accountID)")

			Text("…\(state.account.accountID.suffix(6))")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.monospaced()
				.fixedSize(horizontal: true, vertical: false)
				.accessibilityHidden(true)

			Spacer(minLength: 3)

			if state.isRefreshing {
				ProgressView()
					.controlSize(.mini)
					.help("Refreshing this account")
			}

			Circle()
				.fill(accountStatusColor)
				.frame(width: 4, height: 4)
				.accessibilityHidden(true)

			Text(state.account.statusLabel)
				.font(PanelFont.tertiary)
				.foregroundStyle(accountStatusColor)
				.lineLimit(1)
		}
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(accountAccessibilityLabel)
	}

	private var accountAccessibilityLabel: String {
		let refreshState = state.isRefreshing ? ", refreshing" : ""
		return "Account \(state.account.displayLabel), \(state.account.accountID), \(state.account.statusLabel)\(refreshState)"
	}

	private var accountStatusColor: Color {
		if state.account.enabled == false
			|| state.account.lifecycleReadiness != .ready
			|| state.account.observedState == .authFailed
			|| state.account.observedState == .unavailable
		{
			return PanelPalette.destructive(colorScheme)
		}
		if state.account.observedState == .depleted
			|| state.account.observedState == .unknown
			|| state.account.observedState == .pluginUnready
		{
			return PanelPalette.warning(colorScheme)
		}
		return PanelPalette.routeAccent(colorScheme)
	}

	private var quotaWindows: some View {
		HStack(spacing: 7) {
			ResetCardQuotaWindowView(
				title: "5h",
				window: state.fiveHourQuota
			)

			Rectangle()
				.fill(PanelPalette.separator(colorScheme))
				.frame(width: 0.5, height: 19)
				.allowsHitTesting(false)

			ResetCardQuotaWindowView(
				title: "7d",
				window: state.sevenDayQuota
			)
		}
	}

	@ViewBuilder
	private var cardInventory: some View {
		if let error = state.inventory?.observationError {
			Text(error.presentation)
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.destructive(colorScheme))
				.lineLimit(1)
				.help(error.presentation)
		} else if let error = state.error {
			Text(error.localizedDescription)
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.destructive(colorScheme))
				.lineLimit(1)
				.help(error.localizedDescription)
		} else if state.isRefreshing, state.inventory == nil {
			HStack(spacing: 5) {
				ProgressView()
					.controlSize(.mini)
				Text("Loading Reset Cards")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			}
		} else if state.targets.isEmpty {
			Text("No available Reset Cards")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
		} else {
			HStack(spacing: 5) {
				Text("Cards")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.fixedSize(horizontal: true, vertical: false)

				ScrollView(.horizontal, showsIndicators: false) {
					HStack(spacing: 4) {
						ForEach(Array(state.targets.enumerated()), id: \.element) { index, target in
							Button {
								tap(target)
							} label: {
								cardChip(target, ordinal: index + 1)
							}
							.buttonStyle(.plain)
							.disabled(store.blocksNewAttempt(for: target))
							.accessibilityLabel(accessibilityLabel(target, ordinal: index + 1))
							.accessibilityHint(accessibilityHint(target))
							.help(help(target))
						}
					}
				}
			}
		}
	}

	private var countdownAttempt: ResetCardUseAttempt? {
		confirmation.isSubmitting ? nil : confirmation.armedAttempt
	}

	private func cardChip(_ target: ResetCardUseTarget, ordinal: Int) -> some View {
		Text(cardChipTitle(target, ordinal: ordinal))
			.font(PanelFont.usageValue)
			.foregroundStyle(
				confirmation.isArmed(target)
					? PanelPalette.warning(colorScheme)
					: PanelPalette.primaryText(colorScheme).opacity(0.9)
			)
			.monospacedDigit()
			.lineLimit(1)
			.fixedSize(horizontal: true, vertical: false)
		.padding(.horizontal, 5)
		.padding(.vertical, 2)
		.background(
			confirmation.isArmed(target)
				? PanelPalette.warning(colorScheme).opacity(colorScheme == .dark ? 0.2 : 0.14)
				: PanelPalette.routeAccent(colorScheme).opacity(colorScheme == .dark ? 0.16 : 0.11)
		)
		.clipShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
		.contentShape(Rectangle())
	}

	private func cardChipTitle(_ target: ResetCardUseTarget, ordinal: Int) -> String {
		if confirmation.isSubmitting(target) {
			return "Using \(ordinal)…"
		}
		if confirmation.isArmed(target) {
			let seconds = confirmationSecondsRemaining > 0
				? confirmationSecondsRemaining
				: Self.confirmationWindowSeconds
			return "Confirm \(ordinal) · \(seconds)s"
		}

		return "\(ordinal) · \(Self.cardDateRange(target.descriptor))"
	}

	private func cardWindow(_ descriptor: ResetCardDescriptor) -> String {
		"\(Self.cardDateTime(descriptor.grantedAtUnixSeconds)) → \(Self.cardDateTime(descriptor.expiresAtUnixSeconds))"
	}

	private func accessibilityLabel(_ target: ResetCardUseTarget, ordinal: Int) -> String {
		let window = cardWindow(target.descriptor)
		if confirmation.isSubmitting(target) {
			return "Using Reset Card \(ordinal), \(window)"
		}
		if confirmation.isArmed(target) {
			return "Confirm use of Reset Card \(ordinal), \(window)"
		}

		return "Reset Card \(ordinal), \(window)"
	}

	private func accessibilityHint(_ target: ResetCardUseTarget) -> String {
		if confirmation.isSubmitting {
			return confirmation.isSubmitting(target)
				? "The request is in progress."
				: "Wait until the current Reset Card request finishes."
		}

		return confirmation.isArmed(target)
			? "Activate again to submit the same descriptor and operation key."
			: "Activate once to confirm use. Confirmation cancels after five seconds."
	}

	private func help(_ target: ResetCardUseTarget) -> String {
		let window = cardWindow(target.descriptor)
		if confirmation.isSubmitting {
			return confirmation.isSubmitting(target)
				? "Using Reset Card \(window)"
				: "Wait until the current Reset Card request finishes."
		}
		if confirmation.isArmed(target) {
			return "Click again within five seconds to use this Reset Card."
		}

		return "\(window). Click once to confirm use."
	}

	private func tap(_ target: ResetCardUseTarget) {
		guard let attempt = confirmation.tap(target) else {
			return
		}

		Task {
			let completion = await store.use(attempt)
			confirmation.finish(attempt, completion: completion)
		}
	}

	@MainActor
	private func runConfirmationCountdown(
		for attempt: ResetCardUseAttempt?
	) async {
		guard let attempt else {
			confirmationSecondsRemaining = 0
			return
		}

		let clock = ContinuousClock()
		let deadline = clock.now.advanced(by: .seconds(Self.confirmationWindowSeconds))
		confirmationSecondsRemaining = Self.confirmationWindowSeconds

		while clock.now < deadline {
			let nextWake = min(clock.now.advanced(by: .seconds(1)), deadline)
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

	private static func cardDateRange(_ descriptor: ResetCardDescriptor) -> String {
		let granted = Date(timeIntervalSince1970: TimeInterval(descriptor.grantedAtUnixSeconds))
		let expires = Date(timeIntervalSince1970: TimeInterval(descriptor.expiresAtUnixSeconds))
		let grantedDay = granted.formatted(.dateTime.month(.abbreviated).day())
		let expiryDay = expires.formatted(.dateTime.month(.abbreviated).day())
		return "\(grantedDay)→\(expiryDay)"
	}

	private static func cardDateTime(_ unixSeconds: Int64) -> String {
		let date = Date(timeIntervalSince1970: TimeInterval(unixSeconds))
		let day = date.formatted(.dateTime.month(.abbreviated).day())
		let time = date.formatted(
			.dateTime
				.hour(.twoDigits(amPM: .omitted))
				.minute(.twoDigits)
		)
		return "\(day) \(time)"
	}
}

enum ResetCardQuotaPresentationTone: Equatable {
	case current
	case warning
	case muted
	case error
}

struct ResetCardQuotaPresentation: Equatable {
	let valueText: String
	let detailText: String?
	let tone: ResetCardQuotaPresentationTone
	let usedPercent: UInt8?
	let resetDate: Date?

	init(window: ResetCardQuotaWindow) {
		switch window.state {
		case .current(let usedPercent, _):
			valueText = "\(usedPercent)% used"
			detailText = nil
			tone = .current
			self.usedPercent = usedPercent
			resetDate = window.resetDate
		case .stale(let usedPercent, _):
			valueText = "\(usedPercent)% stale"
			detailText = nil
			tone = .warning
			self.usedPercent = usedPercent
			resetDate = window.resetDate
		case .unknown:
			valueText = "—"
			detailText = "No data"
			tone = .muted
			usedPercent = nil
			resetDate = nil
		case .error(.unsupportedWindow):
			valueText = "—"
			detailText = "Not reported"
			tone = .muted
			usedPercent = nil
			resetDate = nil
		case .error(let error):
			valueText = "Error"
			detailText = error.presentation
			tone = .error
			usedPercent = nil
			resetDate = nil
		}
	}
}

private struct ResetCardQuotaWindowView: View {
	let title: String
	let window: ResetCardQuotaWindow
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		let presentation = ResetCardQuotaPresentation(window: window)

		VStack(alignment: .leading, spacing: 3) {
			HStack(alignment: .firstTextBaseline, spacing: 4) {
				Text(title)
					.font(PanelFont.usageLabel)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.frame(width: 15, alignment: .leading)

				Text(presentation.valueText)
					.font(PanelFont.usageValue)
					.foregroundStyle(stateColor(for: presentation.tone))
					.monospacedDigit()
					.lineLimit(1)

				Spacer(minLength: 2)

				if let resetDate = presentation.resetDate {
					Text(Self.compactDateTime(resetDate))
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
						.monospacedDigit()
						.lineLimit(1)
				} else if let detailText = presentation.detailText {
					Text(detailText)
						.font(PanelFont.tertiary)
						.foregroundStyle(stateColor(for: presentation.tone))
						.lineLimit(1)
						.truncationMode(.tail)
				}
			}

			GeometryReader { proxy in
				ZStack(alignment: .leading) {
					Capsule(style: .continuous)
						.fill(PanelPalette.progressTrack(colorScheme))

					if let usedPercent = presentation.usedPercent {
						Capsule(style: .continuous)
							.fill(stateColor(for: presentation.tone).opacity(0.84))
							.frame(
								width: proxy.size.width
									* CGFloat(usedPercent)
									/ 100
							)
					}
				}
			}
			.frame(height: 3.2)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.frame(height: 22, alignment: .topLeading)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel("\(title) quota")
		.accessibilityValue(window.accessibilityValue)
		.help(window.accessibilityValue)
	}

	private func stateColor(for tone: ResetCardQuotaPresentationTone) -> Color {
		switch tone {
		case .current:
			return PanelPalette.usageCyan(colorScheme)
		case .warning:
			return PanelPalette.warning(colorScheme)
		case .muted:
			return PanelPalette.secondaryText(colorScheme)
		case .error:
			return PanelPalette.destructive(colorScheme)
		}
	}

	private static func compactDateTime(_ date: Date) -> String {
		let day = date.formatted(.dateTime.month(.abbreviated).day())
		let time = date.formatted(
			.dateTime
				.hour(.twoDigits(amPM: .omitted))
				.minute(.twoDigits)
		)
		return "\(day) \(time)"
	}
}
