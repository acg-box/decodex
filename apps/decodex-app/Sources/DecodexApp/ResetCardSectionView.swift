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
	let showsEmail: Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var confirmation = ResetCardUseConfirmation()
	@State private var confirmationSecondsRemaining = 0

	init(
		state: ResetCardAccountState,
		store: ResetCardStore,
		showsEmail: Bool = false
	) {
		self.state = state
		self.store = store
		self.showsEmail = showsEmail
	}

	var body: some View {
		VStack(alignment: .leading, spacing: 6) {
			accountHeader
			profileIdentity
			profileActivity
			quotaWindows
			cardInventory
		}
		.padding(.horizontal, 8)
		.padding(.vertical, 7)
		.fixedSize(horizontal: false, vertical: true)
		.accessibilityIdentifier("decodex.account.\(state.account.accountID)")
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
			HStack(alignment: .firstTextBaseline, spacing: 5) {
				Text(state.account.displayLabel)
					.font(PanelFont.accountName)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)
					.layoutPriority(1)
					.help("Account \(state.account.accountID)")

				Circle()
					.fill(accountStatusColor)
					.frame(width: 4, height: 4)
					.accessibilityHidden(true)

				Text(state.account.statusLabel)
					.font(PanelFont.tertiary)
					.foregroundStyle(accountStatusColor)
					.lineLimit(1)
			}
			.frame(maxWidth: .infinity, alignment: .leading)
			.accessibilityElement(children: .ignore)
			.accessibilityLabel(accountAccessibilityLabel)

			Spacer(minLength: 3)

			AccountRowActionsView(state: state, store: store)

			if state.isRefreshing || state.isProfileRefreshing {
				ProgressView()
					.controlSize(.mini)
					.help("Refreshing this account")
			}
		}
	}

	private var accountAccessibilityLabel: String {
		let refreshState = state.isRefreshing || state.isProfileRefreshing
			? ", refreshing"
			: ""
		return "Account \(state.account.displayLabel), \(state.account.accountID), \(state.account.statusLabel)\(refreshState)"
	}

	@ViewBuilder
	private var profileIdentity: some View {
		if let text = profileIdentityText {
			HStack(alignment: .firstTextBaseline, spacing: 4) {
				Image(systemName: showsEmail && profileEmail != nil ? "envelope" : "person.text.rectangle")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.accessibilityHidden(true)

				Text(text)
					.font(PanelFont.accountDetail)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)

				if let refreshError = state.profile?.refreshError {
					Image(systemName: "clock.arrow.circlepath")
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.warning(colorScheme))
						.help("Cached profile: \(refreshError.presentation)")
						.accessibilityLabel("Cached profile, \(refreshError.presentation)")
				}
			}
		}
	}

	@ViewBuilder
	private var profileActivity: some View {
		if let profile = state.profile, profile.snapshot.hasContent {
			VStack(alignment: .leading, spacing: 3) {
				AccountProfileSummaryView(profile: profile.snapshot)

				if let degradationText = state.profileDegradationText {
					Label(
						"Saved activity · \(degradationText)",
						systemImage: "exclamationmark.triangle"
					)
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.warning(colorScheme))
					.lineLimit(1)
					.help(degradationText)
					.accessibilityLabel("Saved account activity is not current. \(degradationText)")
				}
			}
		} else if state.isProfileRefreshing, state.profile == nil {
			Text("Loading account activity")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
		} else if state.profileUnavailable?.error == .unauthorized {
			Text("Login refresh required")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.warning(colorScheme))
		}
	}

	private var profileIdentityText: String? {
		var parts = [String]()
		if let profile = state.profile {
			if showsEmail, let email = profile.email {
				parts.append(email)
			}
			if let planType = profile.planType {
				parts.append(planType)
			}
			if let displayName = profile.displayName,
				displayName.caseInsensitiveCompare(state.account.displayLabel) != .orderedSame
			{
				parts.append(displayName)
			} else if let username = profile.username {
				parts.append(username)
			}
			if profile.isCached {
				parts.append("Cached")
			}
		} else if let unavailable = state.profileUnavailable {
			if showsEmail, let email = unavailable.claims.email {
				parts.append(email)
			}
			if let planType = unavailable.claims.planType {
				parts.append(planType)
			}
		}
		return parts.isEmpty ? nil : parts.joined(separator: " · ")
	}

	private var profileEmail: String? {
		state.profile?.email ?? state.profileUnavailable?.claims.email
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
		VStack(alignment: .leading, spacing: 5) {
			if ResetCardQuotaPresentation(window: state.fiveHourQuota).isVisible {
				ResetCardQuotaWindowView(
					title: "5h",
					window: state.fiveHourQuota
				)
			}
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
			Text("No Reset Cards")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
		} else {
			HStack(spacing: 5) {
				Text("Cards \(state.targets.count)")
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
				.frame(height: 22)
				.fixedSize(horizontal: false, vertical: true)
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
		let zone = TimeZone.current.abbreviation()
			?? TimeZone.current.identifier
		if confirmation.isSubmitting {
			return confirmation.isSubmitting(target)
				? "Using Reset Card \(window) (\(zone))"
				: "Wait until the current Reset Card request finishes."
		}
		if confirmation.isArmed(target) {
			return "Click again within five seconds to use this Reset Card."
		}

		return "\(window) (\(zone)). Click once to confirm use."
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

	private static func cardDateRange(_ descriptor: ResetCardDescriptor) -> String {
		let granted = Date(
			timeIntervalSince1970: TimeInterval(descriptor.grantedAtUnixSeconds)
		)
		let expires = Date(
			timeIntervalSince1970: TimeInterval(descriptor.expiresAtUnixSeconds)
		)
		let grantedDay = granted.formatted(.dateTime.month(.abbreviated).day())
		let expiryDay = expires.formatted(.dateTime.month(.abbreviated).day())
		return "\(grantedDay)→\(expiryDay)"
	}
}

enum ResetCardQuotaPresentationTone: Equatable {
	case current
	case warning
	case muted
	case error
}

struct ResetCardQuotaPresentation: Equatable {
	let isVisible: Bool
	let valueText: String
	let detailText: String?
	let tone: ResetCardQuotaPresentationTone
	let usedPercent: UInt8?
	let remainingPercent: UInt8?
	let resetDate: Date?

	init(window: ResetCardQuotaWindow) {
		switch window.state {
		case .current(let usedPercent, _):
			isVisible = true
			let remainingPercent = 100 - min(100, usedPercent)
			valueText = "\(remainingPercent)% left"
			detailText = nil
			tone = .current
			self.usedPercent = usedPercent
			self.remainingPercent = remainingPercent
			resetDate = window.resetDate
		case .stale(let usedPercent, _):
			isVisible = true
			let remainingPercent = 100 - min(100, usedPercent)
			valueText = "\(remainingPercent)% left"
			detailText = "stale"
			tone = .warning
			self.usedPercent = usedPercent
			self.remainingPercent = remainingPercent
			resetDate = window.resetDate
		case .unknown:
			isVisible = false
			valueText = "—"
			detailText = "No data"
			tone = .muted
			usedPercent = nil
			remainingPercent = nil
			resetDate = nil
		case .error(.unsupportedWindow):
			isVisible = false
			valueText = "—"
			detailText = "Not reported"
			tone = .muted
			usedPercent = nil
			remainingPercent = nil
			resetDate = nil
		case .error(let error):
			isVisible = true
			valueText = "Error"
			detailText = error.presentation
			tone = .error
			usedPercent = nil
			remainingPercent = nil
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

				if let detailText = presentation.detailText,
					presentation.resetDate != nil
				{
					Text(detailText)
						.font(PanelFont.tertiary)
						.foregroundStyle(stateColor(for: presentation.tone))
						.lineLimit(1)
				}

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

			if let remainingPercent = presentation.remainingPercent {
				GeometryReader { proxy in
					ZStack(alignment: .leading) {
						Capsule(style: .continuous)
							.fill(PanelPalette.progressTrack(colorScheme))

						Capsule(style: .continuous)
							.fill(stateColor(for: presentation.tone).opacity(0.84))
							.frame(
								width: proxy.size.width
									* CGFloat(remainingPercent)
									/ 100
							)
					}
				}
				.frame(height: 3.2)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.frame(
			height: presentation.remainingPercent == nil ? 14 : 22,
			alignment: .topLeading
		)
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
