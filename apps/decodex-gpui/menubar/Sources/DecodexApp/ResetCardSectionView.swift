import SwiftUI

struct ResetCardMessageView: View {
	let message: ResetCardStoreMessage
	let dismiss: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.related) {
			Image(systemName: symbol)
				.font(PanelFont.tertiary)
				.foregroundStyle(color)
				.accessibilityHidden(true)

			Text(message.text)
				.font(PanelFont.tertiary)
				.foregroundStyle(color)
				.frame(maxWidth: .infinity, alignment: .leading)
				.fixedSize(horizontal: false, vertical: true)
				.layoutPriority(1)

			Button(action: dismiss) {
				Image(systemName: "xmark")
					.font(PanelFont.tertiary)
			}
			.buttonStyle(PanelPressButtonStyle(pressedScale: 0.9))
			.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			.fixedSize()
			.help("Dismiss message")
			.accessibilityLabel("Dismiss message")
		}
		.padding(.horizontal, PanelSpacing.cardHorizontal)
		.padding(.vertical, PanelSpacing.cardVertical)
		.panelCardSurface(cornerRadius: 14)
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
		VStack(alignment: .leading, spacing: PanelSpacing.related) {
			ForEach(store.pendingAttempts, id: \.idempotencyKey) { attempt in
				let accountLabel = store.accountLabel(for: attempt.target.accountID)
				let status = store.pendingStatus(for: attempt)

				HStack(spacing: PanelSpacing.related) {
					Image(systemName: "clock.arrow.circlepath")
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.warning(colorScheme))
						.accessibilityHidden(true)

					Text(accountLabel)
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.88))
						.lineLimit(1)
						.truncationMode(.tail)

					Text("·")
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
						.fixedSize()

					Text(status.text)
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
						.lineLimit(1)
						.fixedSize(horizontal: true, vertical: false)
						.layoutPriority(1)

					Spacer(minLength: 0)
				}
				.help(helpText(for: status, attempt: attempt))
				.accessibilityElement(children: .ignore)
				.accessibilityLabel("\(accountLabel). \(status.accessibilityText).")
				.accessibilityHint(
					"Saved operation ending in \(attempt.idempotencyKey.suffix(8)). Decodex checks automatically."
				)
			}
		}
		.padding(.horizontal, PanelSpacing.cardHorizontal)
		.padding(.vertical, PanelSpacing.cardVertical)
		.panelCardSurface(cornerRadius: 14)
	}

	private func helpText(
		for status: ResetCardPendingStatus,
		attempt: ResetCardUseAttempt
	) -> String {
		let automaticCheck = "Decodex checks this saved request automatically."
		let operation = "Operation …\(attempt.idempotencyKey.suffix(8))."
		guard let detail = status.detail else {
			return "\(automaticCheck) \(operation)"
		}
		return "\(detail) \(automaticCheck) \(operation)"
	}
}

enum ResetCardInventoryPresentation: Equatable {
	case loginRequired
	case checking
	case connecting(detail: String)
	case unavailable(detail: String)
	case empty
	case available

	init(
		state: ResetCardAccountState
	) {
		if state.requiresLoginRefresh {
			self = .loginRequired
			return
		}

		if case .connecting(let detail) = state.inventoryFailure {
			if state.inventory == nil {
				self = .connecting(detail: detail)
				return
			}
		}

		if case .unavailable(let detail) = state.inventoryFailure {
			self = .unavailable(detail: detail)
			return
		}
		guard let inventory = state.inventory else {
			self = .checking
			return
		}
		guard inventory.detailsComplete else {
			self = .unavailable(
				detail: "Reset Card details are temporarily unavailable."
			)
			return
		}
		self = state.targets.isEmpty ? .empty : .available
	}
}

struct ResetCardAccountRow: View {
	private static let confirmationWindowSeconds = 5

	let state: ResetCardAccountState
	let store: ResetCardStore
	let showsEmail: Bool
	let isAccountCardHovered: Bool
	let isReorderGestureEnabled: Bool
	let onReorderDragChanged: (CGFloat) -> Void
	let onReorderDragEnded: () -> Void
	@Binding private var detailedAccountID: String?
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme
	@State private var confirmation = ResetCardUseConfirmation()
	@State private var confirmationSecondsRemaining = 0
	@State private var isReorderHandleHovered = false
	@State private var isReorderHandleDragging = false

	init(
		state: ResetCardAccountState,
		store: ResetCardStore,
		showsEmail: Bool = false,
		detailedAccountID: Binding<String?> = .constant(nil),
		isAccountCardHovered: Bool = false,
		isReorderGestureEnabled: Bool = true,
		onReorderDragChanged: @escaping (CGFloat) -> Void = { _ in },
		onReorderDragEnded: @escaping () -> Void = {}
	) {
		self.state = state
		self.store = store
		self.showsEmail = showsEmail
		self.isAccountCardHovered = isAccountCardHovered
		self.isReorderGestureEnabled = isReorderGestureEnabled
		self.onReorderDragChanged = onReorderDragChanged
		self.onReorderDragEnded = onReorderDragEnded
		_detailedAccountID = detailedAccountID
	}

	var body: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.compact) {
			HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.compact) {
				identityHeader
				AccountPrimaryActionsView(
					state: state,
					store: store
				)
			}

			if let pending = store.pendingRoute,
				pending.accountID == state.account.accountID
			{
				AccountRoutePendingStatusView(pending: pending)
					.transition(.panelInline)
			}

			if exceptionalStatusText != nil {
				exceptionalStatus
					.transition(.panelInline)
			}

			quotaWindows

			HStack(alignment: .center, spacing: PanelSpacing.compact) {
				cardInventory
					.frame(maxWidth: .infinity, alignment: .leading)
					.layoutPriority(1)

				AccountUtilityActionsView(
					state: state,
					store: store,
					isPresentingDetails: detailsBinding
				)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(.horizontal, PanelSpacing.cardHorizontal)
		.padding(.vertical, PanelSpacing.cardVertical)
		.overlay(alignment: .trailing) {
			reorderHandle
				.offset(x: -PanelSpacing.micro)
		}
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
		.animation(rowStateAnimation, value: exceptionalStatusText)
		.animation(rowStateAnimation, value: inventoryPresentation)
		.animation(rowStateAnimation, value: showsReorderHandle)
		.animation(rowStateAnimation, value: isReorderHandleHovered)
	}

	private var reorderHandle: some View {
		ZStack {
			RoundedRectangle(cornerRadius: 5, style: .continuous)
				.frame(width: 14, height: 18)
				.foregroundStyle(
					isReorderHandleHovered
						? PanelPalette.actionBlue(colorScheme).opacity(0.15)
						: PanelPalette.primaryText(colorScheme).opacity(
							colorScheme == .dark ? 0.08 : 0.055
						)
				)

			Image(systemName: "line.3.horizontal")
				.font(.system(size: 9, weight: .semibold))
				.foregroundStyle(
					isReorderHandleHovered
						? PanelPalette.actionBlue(colorScheme)
						: PanelPalette.secondaryText(colorScheme).opacity(0.68)
				)
		}
			.frame(width: 18, height: 28)
			.opacity(showsReorderHandle ? 1 : 0)
			.contentShape(Rectangle())
			.highPriorityGesture(
				DragGesture(
					minimumDistance: 1,
					coordinateSpace: .named(
						AccountCardReorderLayout.coordinateSpaceName
					)
				)
					.onChanged { value in
						isReorderHandleDragging = true
						onReorderDragChanged(value.translation.height)
					}
					.onEnded { _ in
						isReorderHandleDragging = false
						onReorderDragEnded()
					}
			)
			.allowsHitTesting(
				store.canReorderAccounts && isReorderGestureEnabled
			)
			.onHover { isHovered in
				isReorderHandleHovered = isHovered
			}
			.help("Drag to reorder accounts")
			.accessibilityElement()
			.accessibilityLabel("Reorder \(identity.text)")
			.accessibilityValue(reorderAccessibilityValue)
			.accessibilityHint("Drag to change the account routing order.")
			.accessibilityHidden(store.canReorderAccounts == false)
			.accessibilityAction(named: Text("Move up")) {
				moveAccount(by: -1)
			}
			.accessibilityAction(named: Text("Move down")) {
				moveAccount(by: 1)
			}
	}

	private var showsReorderHandle: Bool {
		store.canReorderAccounts
			&& (
				(
					(isAccountCardHovered || isReorderHandleHovered)
						&& isReorderGestureEnabled
				)
					|| isReorderHandleDragging
			)
	}

	private var reorderAccessibilityValue: String {
		guard let index = store.accounts.firstIndex(where: {
			$0.account.accountID == state.account.accountID
		}) else {
			return ""
		}
		return "Position \(index + 1) of \(store.accounts.count)"
	}

	private func moveAccount(by offset: Int) {
		guard store.canReorderAccounts,
			let index = store.accounts.firstIndex(where: {
				$0.account.accountID == state.account.accountID
			}),
			store.accounts.indices.contains(index + offset)
		else {
			return
		}
		let targetAccountID = store.accounts[index + offset].account.accountID
		Task {
			await store.moveAccount(state.account.accountID, onto: targetAccountID)
		}
	}

	private var identityHeader: some View {
		HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.compact) {
			Text(identity.text)
				.font(PanelFont.accountName)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
				.contentTransition(.opacity)
				.lineLimit(1)
				.truncationMode(.middle)
				.layoutPriority(1)
				.animation(identityTransitionAnimation, value: identity.text)

			if let planType {
				Text(planType)
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.fixedSize(horizontal: true, vertical: false)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(identityAccessibilityLabel)
	}

	private var exceptionalStatus: some View {
		HStack(spacing: PanelSpacing.related) {
			Text(exceptionalStatusText ?? "")
				.font(PanelFont.accountDetail)
				.foregroundStyle(exceptionalStatusColor)
				.lineLimit(1)
				.truncationMode(.tail)

			Spacer(minLength: 2)

			if state.requiresLoginRefresh,
				state.account.credentialBinding != nil
			{
				AccountRefreshLoginButton(state: state, store: store)
			}
		}
		.accessibilityElement(children: .contain)
	}

	private var identity: AccountIdentityPresentation {
		AccountIdentityPresentation(
			alias: state.account.alias,
			email: profileEmail,
			revealsEmail: showsEmail
		)
	}

	private var profileEmail: String? {
		state.profile?.email ?? state.profileUnavailable?.claims.email
	}

	private var planType: String? {
		state.profile?.planType ?? state.profileUnavailable?.claims.planType
	}

	private var identityAccessibilityLabel: String {
		let accountLabel =
			identity.showsEmail
			? "Account email \(identity.text)"
			: "Account \(identity.text)"
		guard let planType else {
			return accountLabel
		}
		return "\(accountLabel), \(planType) plan"
	}

	private var exceptionalStatusText: String? {
		if state.account.enabled == false {
			return "Disabled"
		}
		if state.requiresLoginRefresh {
			return "Login refresh required"
		}
		switch state.account.lifecycleReadiness {
		case .credentialAbsent:
			return "Login unavailable"
		case .storeUnavailable:
			return "Credential store unavailable"
		case .storeMismatch:
			return "Credential binding changed"
		case .providerMismatch:
			return "Login belongs to another account"
		case .operationUnsettled:
			return "Account update pending"
		case .callbackCapabilityUnready:
			// Account health and usage come from the direct provider API. The callback
			// capability only gates Quick Task routing, so it is not an account-data
			// error here.
			break
		case .tombstoned:
			return "Logged out"
		case .ready:
			break
		}
		switch state.account.observedState {
		case .available:
			return nil
		case .authFailed:
			return "Login refresh required"
		case .depleted:
			return nil
		case .pluginUnready:
			return "Provider update required"
		case .unknown:
			return "Account status unavailable"
		case .unavailable:
			return "Account unavailable"
		}
	}

	private var exceptionalStatusColor: Color {
		if state.account.enabled == false
			|| state.account.observedState == .unknown
			|| state.account.observedState == .pluginUnready
		{
			return PanelPalette.warning(colorScheme)
		}
		return PanelPalette.destructive(colorScheme)
	}

	private var detailsBinding: Binding<Bool> {
		Binding(
			get: {
				detailedAccountID == state.account.accountID
			},
			set: { isPresented in
				detailedAccountID = isPresented ? state.account.accountID : nil
			}
		)
	}

	@ViewBuilder
	private var quotaWindows: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.micro) {
			if ResetCardQuotaPresentation(window: state.fiveHourQuota).isVisible {
				ResetCardQuotaWindowView(
					title: "5h",
					window: state.fiveHourQuota
				)
			}
			if ResetCardQuotaPresentation(window: state.sevenDayQuota).isVisible {
				ResetCardQuotaWindowView(
					title: "7d",
					window: state.sevenDayQuota
				)
			}
		}
	}

	@ViewBuilder
	private var cardInventory: some View {
		switch inventoryPresentation {
		case .loginRequired:
			Text("Reset Cards need this login")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)
				.help(
					"Choose Refresh login in the menu bar app to sign in with the official Codex device login."
				)
		case .checking:
			ResetCardInventoryPendingView()
		case .connecting(let detail):
			inventoryProgress("Connecting to Decodex…", help: detail)
		case .unavailable(let detail):
			Text("Reset Cards unavailable")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)
				.help(detail)
		case .empty:
			Text("No Reset Cards")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
		case .available:
			ScrollView(.horizontal, showsIndicators: false) {
				HStack(spacing: PanelSpacing.compact) {
					ForEach(state.targets, id: \.self) { target in
						Button {
							tap(target)
						} label: {
							cardChip(target)
						}
						.buttonStyle(
							ResetCardChipButtonStyle(
								isArmed: confirmation.isArmed(target),
								isBusy: confirmation.isSubmitting(target)
							)
						)
						.disabled(store.blocksNewAttempt(for: target))
						.accessibilityLabel(accessibilityLabel(target))
						.accessibilityHint(accessibilityHint(target))
						.help(help(target))
						.transition(
							.opacity.combined(
								with: .scale(scale: 0.96)
							)
						)
					}
				}
				.animation(rowStateAnimation, value: state.targets)
			}
			.frame(height: 26)
			.fixedSize(horizontal: false, vertical: true)
		}
	}

	private func inventoryProgress(
		_ text: String,
		help: String
	) -> some View {
		HStack(spacing: PanelSpacing.related) {
			ProgressView()
				.controlSize(.mini)
				.accessibilityHidden(true)

			Text(text)
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)
		}
		.help(help)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(text)
	}

	private var inventoryPresentation: ResetCardInventoryPresentation {
		ResetCardInventoryPresentation(state: state)
	}

	private var countdownAttempt: ResetCardUseAttempt? {
		confirmation.isSubmitting ? nil : confirmation.armedAttempt
	}

	private func cardChip(_ target: ResetCardUseTarget) -> some View {
		ZStack {
			Text(normalCardChipTitle(target))
				.hidden()

			Text("Confirm · \(Self.confirmationWindowSeconds)s")
				.hidden()

			HStack(spacing: PanelSpacing.micro) {
				if confirmation.isSubmitting(target) {
					ProgressView()
						.controlSize(.mini)
						.accessibilityHidden(true)
				}

				Text(cardChipTitle(target))
					.contentTransition(.opacity)
			}
			.foregroundStyle(cardChipForeground(target))
		}
		.font(PanelFont.resetCardAction)
		.monospacedDigit()
		.lineLimit(1)
		.fixedSize(horizontal: true, vertical: false)
		.padding(.horizontal, PanelSpacing.micro)
		.contentShape(Rectangle())
		.animation(
			rowStateAnimation,
			value: confirmation.isArmed(target)
		)
		.animation(
			rowStateAnimation,
			value: confirmation.isSubmitting(target)
		)
		.animation(
			rowStateAnimation,
			value: confirmationSecondsRemaining
		)
	}

	private func cardChipTitle(_ target: ResetCardUseTarget) -> String {
		if confirmation.isSubmitting(target) {
			return "Using…"
		}
		if confirmation.isArmed(target) {
			let seconds =
				confirmationSecondsRemaining > 0
				? confirmationSecondsRemaining
				: Self.confirmationWindowSeconds
			return "Confirm · \(seconds)s"
		}

		return normalCardChipTitle(target)
	}

	private func normalCardChipTitle(_ target: ResetCardUseTarget) -> String {
		Self.cardExpiryText(target.descriptor.expiresAtUnixSeconds)
	}

	private func cardChipForeground(_ target: ResetCardUseTarget) -> Color {
		if confirmation.isSubmitting(target) {
			return PanelPalette.actionBlue(colorScheme)
		}
		return confirmation.isArmed(target)
			? PanelPalette.warning(colorScheme)
			: PanelPalette.secondaryText(colorScheme)
	}

	private func accessibilityLabel(_ target: ResetCardUseTarget) -> String {
		Self.cardAccessibilityLabel(
			expiresAtUnixSeconds: target.descriptor.expiresAtUnixSeconds
		)
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
		let label = accessibilityLabel(target)
		if confirmation.isSubmitting {
			return confirmation.isSubmitting(target)
				? "\(label). The request is in progress."
				: "Wait until the current Reset Card request finishes."
		}
		if confirmation.isArmed(target) {
			return "\(label). Click again within five seconds to use it."
		}

		return "\(label). Click once to confirm use."
	}

	private func tap(_ target: ResetCardUseTarget) {
		guard let attempt = confirmation.tap(target) else {
			return
		}
		confirmationSecondsRemaining = 0

		Task {
			let completion = await store.use(attempt)
			confirmation.finish(attempt, completion: completion)
		}
	}

	private var identityTransitionAnimation: Animation? {
		reduceMotion ? nil : PanelMotion.identity
	}

	private var rowStateAnimation: Animation? {
		reduceMotion ? nil : PanelMotion.controlState
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

	static func cardExpiryText(
		_ unixSeconds: Int64,
		timeZone: TimeZone = .current
	) -> String {
		let date = Date(timeIntervalSince1970: TimeInterval(unixSeconds))
		var dayStyle = Date.FormatStyle.dateTime
			.month(.abbreviated)
			.day()
		dayStyle.locale = Locale(identifier: "en_US_POSIX")
		dayStyle.calendar = Calendar(identifier: .gregorian)
		dayStyle.timeZone = timeZone
		var timeStyle = Date.FormatStyle.dateTime
			.hour(.twoDigits(amPM: .omitted))
			.minute(.twoDigits)
		timeStyle.locale = Locale(identifier: "en_US_POSIX@hours=h23")
		timeStyle.calendar = Calendar(identifier: .gregorian)
		timeStyle.timeZone = timeZone
		return "\(date.formatted(dayStyle)) \(date.formatted(timeStyle))"
	}

	static func cardAccessibilityLabel(
		expiresAtUnixSeconds: Int64,
		timeZone: TimeZone = .current
	) -> String {
		let expiry = cardExpiryText(
			expiresAtUnixSeconds,
			timeZone: timeZone
		)
		let parts = expiry.split(separator: " ")
		let spokenExpiry: String
		if parts.count == 3 {
			spokenExpiry = "\(parts[0]) \(parts[1]) at \(parts[2])"
		} else {
			spokenExpiry = expiry
		}
		let date = Date(timeIntervalSince1970: TimeInterval(expiresAtUnixSeconds))
		let zone =
			timeZone.abbreviation(for: date)
			?? timeZone.identifier
		return "Reset Card, expires \(spokenExpiry) \(zone)"
	}
}

private struct ResetCardInventoryPendingView: View {
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		ProgressView()
			.controlSize(.mini)
			.frame(width: 16, height: 16)
			.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			.help("Reset Card inventory is updating in the background.")
			.accessibilityLabel("Reset Card inventory is updating")
	}
}

private struct ResetCardChipButtonStyle: ButtonStyle {
	let isArmed: Bool
	let isBusy: Bool
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.isEnabled) private var isEnabled

	func makeBody(configuration: Configuration) -> some View {
		let shape = RoundedRectangle(cornerRadius: 6, style: .continuous)

		configuration.label
			.padding(.horizontal, PanelSpacing.compact)
			.frame(minHeight: 24)
			.background {
				shape.fill(fillColor)
			}
			.overlay {
				shape.strokeBorder(borderColor, lineWidth: 1)
			}
			.contentShape(shape)
			.opacity(
				isEnabled || isBusy
					? (configuration.isPressed ? 0.76 : 1)
					: 0.46
			)
			.scaleEffect(configuration.isPressed ? 0.985 : 1)
			.animation(
				reduceMotion ? nil : PanelMotion.press,
				value: configuration.isPressed
			)
			.animation(
				reduceMotion ? nil : PanelMotion.controlState,
				value: isArmed
			)
	}

	private var fillColor: Color {
		if isBusy {
			return PanelPalette.actionBlue(colorScheme)
				.opacity(colorScheme == .dark ? 0.12 : 0.09)
		}
		if isArmed {
			return PanelPalette.warning(colorScheme)
				.opacity(colorScheme == .dark ? 0.12 : 0.09)
		}
		return PanelPalette.primaryText(colorScheme)
			.opacity(colorScheme == .dark ? 0.055 : 0.04)
	}

	private var borderColor: Color {
		if isBusy {
			return PanelPalette.actionBlue(colorScheme)
				.opacity(colorScheme == .dark ? 0.72 : 0.62)
		}
		if isArmed {
			return PanelPalette.warning(colorScheme)
				.opacity(colorScheme == .dark ? 0.72 : 0.62)
		}
		return PanelPalette.primaryText(colorScheme)
			.opacity(colorScheme == .dark ? 0.22 : 0.18)
	}
}

enum ResetCardQuotaPresentationTone: Equatable {
	case healthy
	case warning
	case critical
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
			valueText = "\(remainingPercent)%"
			detailText = nil
			tone =
				switch remainingPercent {
				case 51...:
					.healthy
				case 21...:
					.warning
				default:
					.critical
				}
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
		case .error(let error):
			isVisible = false
			valueText = "—"
			detailText =
				error == .unsupportedWindow
				? "Not reported"
				: error.presentation
			tone = .muted
			usedPercent = nil
			remainingPercent = nil
			resetDate = nil
		}
	}
}

private struct ResetCardQuotaWindowView: View {
	private static let titleColumnWidth: CGFloat = 17

	let title: String
	let window: ResetCardQuotaWindow
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		let presentation = ResetCardQuotaPresentation(window: window)
		let remainingPercent = presentation.remainingPercent ?? 0

		HStack(alignment: .center, spacing: PanelSpacing.compact) {
			Text(title)
				.font(PanelFont.quotaText)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.frame(width: Self.titleColumnWidth, alignment: .leading)

			if presentation.remainingPercent != nil {
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
							.animation(
								quotaValueAnimation,
								value: remainingPercent
							)
					}
				}
				.frame(minWidth: 88, maxWidth: .infinity)
				.frame(height: 5)
				.layoutPriority(1)
			}

			HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.micro) {
				Text(presentation.valueText)
					.font(PanelFont.usageValue)
					.contentTransition(
						.numericText(value: Double(remainingPercent))
					)

				if let detailText = presentation.detailText,
					presentation.resetDate != nil
				{
					Text(detailText)
						.font(PanelFont.quotaText)
				}
			}
			.foregroundStyle(stateColor(for: presentation.tone))
			.monospacedDigit()
			.lineLimit(1)
			.fixedSize(horizontal: true, vertical: false)
			.animation(quotaValueAnimation, value: remainingPercent)

			if let resetDate = presentation.resetDate {
				Text(Self.compactDateTime(resetDate))
					.font(PanelFont.quotaText)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.monospacedDigit()
					.lineLimit(1)
					.fixedSize(horizontal: true, vertical: false)
			} else if let detailText = presentation.detailText {
				Text(detailText)
					.font(PanelFont.quotaText)
					.foregroundStyle(stateColor(for: presentation.tone))
					.lineLimit(1)
					.truncationMode(.tail)
					.frame(maxWidth: .infinity, alignment: .trailing)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.frame(height: 15, alignment: .center)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel("\(title) quota")
		.accessibilityValue(window.accessibilityValue)
		.help(window.accessibilityValue)
	}

	private var quotaValueAnimation: Animation? {
		reduceMotion ? nil : PanelMotion.quotaValue
	}

	private func stateColor(for tone: ResetCardQuotaPresentationTone) -> Color {
		switch tone {
		case .healthy:
			return PanelPalette.usageCyan(colorScheme)
		case .warning:
			return PanelPalette.warning(colorScheme)
		case .critical, .error:
			return PanelPalette.destructive(colorScheme)
		case .muted:
			return PanelPalette.secondaryText(colorScheme)
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
