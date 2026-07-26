import SwiftUI

struct VNextResetCardsSectionView: View {
	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 5) {
			header

			Rectangle()
				.fill(PanelPalette.separator(colorScheme))
				.frame(height: 0.5)

			ScrollView(.vertical, showsIndicators: false) {
				VStack(alignment: .leading, spacing: 5) {
					if let message = store.message {
						messageRow(message)
					}

					ForEach(store.pendingAttempts, id: \.idempotencyKey) { attempt in
						pendingRow(attempt)
					}

					if store.isInitialLoading {
						loadingRow
					} else if store.accounts.isEmpty {
						emptyRow
					} else {
						ForEach(store.accounts) { account in
							VNextResetCardAccountRow(
								state: account,
								store: store
							)
						}
					}
				}
				.frame(maxWidth: .infinity, alignment: .leading)
			}
		}
		.padding(.horizontal, 7)
		.padding(.vertical, 6)
		.frame(height: AccountPanelLayout.vNextResetCardsHeight)
		.modernGlassSurface(cornerRadius: 9, depth: .row)
		.accessibilityElement(children: .contain)
	}

	private var header: some View {
		HStack(spacing: 6) {
			Image(systemName: "arrow.counterclockwise.circle")
				.font(PanelFont.summaryIcon)
				.foregroundStyle(PanelPalette.routeAccent(colorScheme))
				.accessibilityHidden(true)

			Text("vNext Reset cards")
				.font(PanelFont.accountName)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))

			Spacer(minLength: 4)

			PanelIconButtonView(
				symbol: "arrow.clockwise",
				tint: PanelPalette.secondaryText(colorScheme),
				isActive: false,
				isDisabled: store.isRefreshing || store.submittingKey != nil,
				isSubtle: true,
				size: 20,
				action: {
					Task {
						await store.refresh()
					}
				},
				help: "Refresh vNext reset cards"
			)
		}
	}

	private var loadingRow: some View {
		HStack(spacing: 6) {
			ProgressView()
				.controlSize(.mini)
			Text("Loading daemon-owned reset cards")
				.font(PanelFont.usageLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
		}
		.frame(maxWidth: .infinity, alignment: .leading)
	}

	private var emptyRow: some View {
		Text(store.hasLoaded ? "No vNext accounts expose reset cards." : "Reset-card service is not loaded.")
			.font(PanelFont.usageLabel)
			.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			.frame(maxWidth: .infinity, alignment: .leading)
			.fixedSize(horizontal: false, vertical: true)
	}

	private func messageRow(_ message: ResetCardStoreMessage) -> some View {
		HStack(alignment: .firstTextBaseline, spacing: 5) {
			Image(systemName: messageSymbol(message.tone))
				.font(PanelFont.tertiary)
				.foregroundStyle(messageColor(message.tone))

			Text(message.text)
				.font(PanelFont.tertiary)
				.foregroundStyle(messageColor(message.tone))
				.fixedSize(horizontal: false, vertical: true)

			Spacer(minLength: 2)

			Button {
				store.dismissMessage()
			} label: {
				Image(systemName: "xmark")
					.font(PanelFont.tertiary)
			}
			.buttonStyle(.plain)
			.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			.help("Dismiss reset-card message")
		}
	}

	private func pendingRow(_ attempt: ResetCardUseAttempt) -> some View {
		HStack(spacing: 5) {
			Image(systemName: "clock.arrow.circlepath")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.warning(colorScheme))
				.accessibilityHidden(true)

			Text(
				"Pending …\(attempt.target.accountID.suffix(8)) · key …\(attempt.idempotencyKey.suffix(8))"
			)
			.font(PanelFont.tertiary)
			.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			.lineLimit(1)

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
					: "Read durable status and retry the same logical request only when it is absent."
			)
		}
	}

	private func messageSymbol(_ tone: ResetCardStoreMessage.Tone) -> String {
		switch tone {
		case .information:
			return "info.circle"
		case .success:
			return "checkmark.circle"
		case .error:
			return "exclamationmark.triangle"
		}
	}

	private func messageColor(_ tone: ResetCardStoreMessage.Tone) -> Color {
		switch tone {
		case .information:
			return PanelPalette.secondaryText(colorScheme)
		case .success:
			return PanelPalette.routeAccent(colorScheme)
		case .error:
			return PanelPalette.destructive(colorScheme)
		}
	}
}

private struct VNextResetCardAccountRow: View {
	private static let confirmationWindowSeconds = 5

	let state: ResetCardAccountState
	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme
	@State private var confirmation = ResetCardUseConfirmation()
	@State private var confirmationSecondsRemaining = 0

	var body: some View {
		VStack(alignment: .leading, spacing: 4) {
			HStack(alignment: .firstTextBaseline, spacing: 5) {
				Text(state.account.displayLabel)
					.font(PanelFont.usageValue)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)

				Text(accountSuffix)
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.monospaced()

				Spacer(minLength: 3)

				Text(admissionLabel)
					.font(PanelFont.tertiary)
					.foregroundStyle(admissionColor)
			}

			if let error = state.error {
				Text(error.localizedDescription)
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.destructive(colorScheme))
					.fixedSize(horizontal: false, vertical: true)
			} else if state.targets.isEmpty {
				Text("No available cards")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			} else {
				ScrollView(.horizontal, showsIndicators: false) {
					HStack(spacing: 4) {
						ForEach(state.targets, id: \.self) { target in
							Button {
								tap(target)
							} label: {
								chip(target)
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
		.padding(.vertical, 2)
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

	private var accountSuffix: String {
		"…\(state.account.accountID.suffix(8))"
	}

	private var admissionLabel: String {
		switch state.account.admissionState {
		case .available:
			return "Available"
		case .depleted:
			return "Depleted"
		}
	}

	private var admissionColor: Color {
		switch state.account.admissionState {
		case .available:
			return PanelPalette.secondaryText(colorScheme)
		case .depleted:
			return PanelPalette.warning(colorScheme)
		}
	}

	private var countdownAttempt: ResetCardUseAttempt? {
		confirmation.isSubmitting ? nil : confirmation.armedAttempt
	}

	private func chip(_ target: ResetCardUseTarget) -> some View {
		Text(chipText(target))
			.font(PanelFont.usageValue)
			.foregroundStyle(
				confirmation.isArmed(target)
					? PanelPalette.warning(colorScheme)
					: PanelPalette.primaryText(colorScheme).opacity(0.88)
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
			.clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
			.contentShape(Rectangle())
	}

	private func chipText(_ target: ResetCardUseTarget) -> String {
		if confirmation.isSubmitting(target) {
			return "Using"
		}
		if confirmation.isArmed(target) {
			let seconds = confirmationSecondsRemaining > 0
				? confirmationSecondsRemaining
				: Self.confirmationWindowSeconds
			return "Confirm Use · \(seconds)s"
		}

		return resetCardDate(target.descriptor.expiresAtUnixSeconds)
	}

	private func accessibilityLabel(_ target: ResetCardUseTarget) -> String {
		let expiry = resetCardDate(target.descriptor.expiresAtUnixSeconds)
		if confirmation.isSubmitting(target) {
			return "Using vNext reset card that expires \(expiry)"
		}
		if confirmation.isArmed(target) {
			return "Confirm use of vNext reset card that expires \(expiry)"
		}

		return "vNext reset card, expires \(expiry)"
	}

	private func accessibilityHint(_ target: ResetCardUseTarget) -> String {
		if confirmation.isSubmitting {
			return confirmation.isSubmitting(target)
				? "The daemon-owned request is in progress."
				: "Wait until the current reset-card request finishes."
		}

		return confirmation.isArmed(target)
			? "Activate again to submit the same public descriptor and idempotency key."
			: "Activate once to confirm use. Confirmation cancels after five seconds."
	}

	private func help(_ target: ResetCardUseTarget) -> String {
		let expiry = resetCardDate(target.descriptor.expiresAtUnixSeconds)
		if confirmation.isSubmitting {
			return confirmation.isSubmitting(target)
				? "Using the daemon-owned reset card that expires \(expiry)"
				: "Wait until the current reset-card request finishes."
		}
		if confirmation.isArmed(target) {
			return "Click again within five seconds to use the reset card that expires \(expiry)."
		}

		return "Expires \(expiry). Click once to confirm daemon-owned use."
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

	private func resetCardDate(_ unixSeconds: Int64) -> String {
		let date = Date(timeIntervalSince1970: TimeInterval(unixSeconds))
		let formatter = DateFormatter()
		formatter.locale = Locale(identifier: "en_US_POSIX")
		formatter.timeZone = .autoupdatingCurrent
		formatter.dateFormat = "MMM d HH:mm"

		return formatter.string(from: date)
	}
}
