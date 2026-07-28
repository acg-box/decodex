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
		VStack(alignment: .leading, spacing: 7) {
			accountHeader
			quotaWindows
			cardInventory
		}
		.padding(.horizontal, 8)
		.padding(.vertical, 7)
		.modernGlassSurface(cornerRadius: 10, depth: .row)
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
		HStack(alignment: .firstTextBaseline, spacing: 6) {
			Text(state.account.displayLabel)
				.font(PanelFont.accountName)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
				.lineLimit(1)
				.truncationMode(.middle)

			Text("…\(state.account.accountID.suffix(8))")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.monospaced()

			Spacer(minLength: 3)

			if state.isRefreshing {
				ProgressView()
					.controlSize(.mini)
					.help("Refreshing this account")
			}

			Text(state.account.statusLabel)
				.font(PanelFont.tertiary)
				.foregroundStyle(accountStatusColor)
				.lineLimit(1)
		}
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
		HStack(spacing: 6) {
			ResetCardQuotaWindowView(
				title: "5 hour",
				window: state.fiveHourQuota
			)
			ResetCardQuotaWindowView(
				title: "7 day",
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
				.fixedSize(horizontal: false, vertical: true)
		} else if let error = state.error {
			Text(error.localizedDescription)
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.destructive(colorScheme))
				.fixedSize(horizontal: false, vertical: true)
		} else if state.isRefreshing, state.inventory == nil {
			Text("Loading Reset Cards")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
		} else if state.targets.isEmpty {
			Text("No available Reset Cards")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
		} else {
			ScrollView(.horizontal, showsIndicators: false) {
				HStack(spacing: 5) {
					ForEach(state.targets, id: \.self) { target in
						Button {
							tap(target)
						} label: {
							cardChip(target)
						}
						.buttonStyle(.plain)
						.disabled(store.blocksNewAttempt(for: target))
						.accessibilityLabel(accessibilityLabel(target))
						.accessibilityHint(accessibilityHint(target))
						.help(help(target))
					}
				}
			}
		}
	}

	private var countdownAttempt: ResetCardUseAttempt? {
		confirmation.isSubmitting ? nil : confirmation.armedAttempt
	}

	private func cardChip(_ target: ResetCardUseTarget) -> some View {
		VStack(alignment: .leading, spacing: 1) {
			Text(cardChipTitle(target))
				.font(PanelFont.usageValue)
				.foregroundStyle(
					confirmation.isArmed(target)
						? PanelPalette.warning(colorScheme)
						: PanelPalette.primaryText(colorScheme).opacity(0.9)
				)
				.monospacedDigit()
				.lineLimit(1)

			if confirmation.isArmed(target) == false,
				confirmation.isSubmitting(target) == false
			{
				Text(cardWindow(target.descriptor))
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.monospacedDigit()
					.lineLimit(1)
			}
		}
		.fixedSize(horizontal: true, vertical: false)
		.padding(.horizontal, 6)
		.padding(.vertical, 3)
		.background(
			confirmation.isArmed(target)
				? PanelPalette.warning(colorScheme).opacity(colorScheme == .dark ? 0.2 : 0.14)
				: PanelPalette.routeAccent(colorScheme).opacity(colorScheme == .dark ? 0.16 : 0.11)
		)
		.clipShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
		.contentShape(Rectangle())
	}

	private func cardChipTitle(_ target: ResetCardUseTarget) -> String {
		if confirmation.isSubmitting(target) {
			return "Using"
		}
		if confirmation.isArmed(target) {
			let seconds = confirmationSecondsRemaining > 0
				? confirmationSecondsRemaining
				: Self.confirmationWindowSeconds
			return "Confirm Use · \(seconds)s"
		}

		return "Reset Card"
	}

	private func cardWindow(_ descriptor: ResetCardDescriptor) -> String {
		"\(Self.cardDate(descriptor.grantedAtUnixSeconds)) → \(Self.cardDate(descriptor.expiresAtUnixSeconds))"
	}

	private func accessibilityLabel(_ target: ResetCardUseTarget) -> String {
		let window = cardWindow(target.descriptor)
		if confirmation.isSubmitting(target) {
			return "Using Reset Card, \(window)"
		}
		if confirmation.isArmed(target) {
			return "Confirm use of Reset Card, \(window)"
		}

		return "Reset Card, \(window)"
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

	private static func cardDate(_ unixSeconds: Int64) -> String {
		let date = Date(timeIntervalSince1970: TimeInterval(unixSeconds))
		return date.formatted(
			Date.FormatStyle()
				.month(.abbreviated)
				.day()
				.hour(.twoDigits(amPM: .omitted))
				.minute(.twoDigits)
		)
	}
}

private struct ResetCardQuotaWindowView: View {
	let title: String
	let window: ResetCardQuotaWindow
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 3) {
			HStack(alignment: .firstTextBaseline, spacing: 4) {
				Text(title)
					.font(PanelFont.usageLabel)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				Spacer(minLength: 2)
				Text(window.stateLabel)
					.font(PanelFont.tertiary)
					.foregroundStyle(stateColor)
			}

			if let usedPercent = window.usedPercent {
				GeometryReader { proxy in
					ZStack(alignment: .leading) {
						Capsule(style: .continuous)
							.fill(PanelPalette.progressTrack(colorScheme))
						Capsule(style: .continuous)
							.fill(stateColor.opacity(0.84))
							.frame(
								width: proxy.size.width
									* CGFloat(usedPercent)
									/ 100
							)
					}
				}
				.frame(height: 4)

				HStack(spacing: 3) {
					Text("\(usedPercent)% used")
					Spacer(minLength: 2)
					if let resetDate = window.resetDate {
						Text(resetDate, format: .dateTime.month(.abbreviated).day().hour().minute())
							.lineLimit(1)
					}
				}
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.monospacedDigit()
			} else {
				Text(window.detailLabel)
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
			}
		}
		.padding(.horizontal, 6)
		.padding(.vertical, 5)
		.frame(maxWidth: .infinity, minHeight: 49, alignment: .topLeading)
		.background(
			PanelPalette.progressTrack(colorScheme).opacity(0.55),
			in: RoundedRectangle(cornerRadius: 6, style: .continuous)
		)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel("\(title) quota")
		.accessibilityValue(window.accessibilityValue)
	}

	private var stateColor: Color {
		switch window.state {
		case .current:
			return PanelPalette.usageCyan(colorScheme)
		case .stale, .unknown:
			return PanelPalette.warning(colorScheme)
		case .error:
			return PanelPalette.destructive(colorScheme)
		}
	}
}
