import Foundation

struct ResetCreditDescriptor: Hashable, Sendable {
	let grantedAtUnixEpoch: Int?
	let expiresAtUnixEpoch: Int?

	init(credit: AccountResetCredit) {
		grantedAtUnixEpoch = credit.grantedAtUnixEpoch
		expiresAtUnixEpoch = credit.expiresAtUnixEpoch
	}
}

struct ResetCreditUseTarget: Hashable, Sendable {
	let accountID: String
	let descriptor: ResetCreditDescriptor
	let occurrence: Int
	let descriptorMultiplicity: Int
	let detailsComplete: Bool

	static func makeTargets(
		accountID: String,
		reportedAvailableCount: Int?,
		credits: [AccountResetCredit]
	) -> [ResetCreditUseTarget] {
		let descriptors = credits.map(ResetCreditDescriptor.init)
		let multiplicityByDescriptor = Dictionary(
			grouping: descriptors,
			by: { $0 }
		)
		.mapValues(\.count)
		var occurrenceByDescriptor = [ResetCreditDescriptor: Int]()
		let detailsComplete = reportedAvailableCount == credits.count

		return descriptors.map { descriptor in
			let occurrence = occurrenceByDescriptor[descriptor, default: 0]
			occurrenceByDescriptor[descriptor] = occurrence + 1

			return ResetCreditUseTarget(
				accountID: accountID,
				descriptor: descriptor,
				occurrence: occurrence,
				descriptorMultiplicity: multiplicityByDescriptor[descriptor, default: 0],
				detailsComplete: detailsComplete
			)
		}
	}
}

struct ResetCreditUsePreparation: Equatable, Sendable {
	let target: ResetCreditUseTarget
	let idempotencyKey: String
}

struct ResetCreditUseAttempt: Equatable, Sendable {
	let target: ResetCreditUseTarget
	let creditID: String
	let idempotencyKey: String
}

enum ResetCreditUseAction: Equatable, Sendable {
	case prepare(ResetCreditUsePreparation)
	case consume(ResetCreditUseAttempt)
}

struct ResetCreditUseConfirmation: Equatable {
	private(set) var pendingPreparation: ResetCreditUsePreparation?
	private(set) var armedAttempt: ResetCreditUseAttempt?
	private(set) var isPreparing = false
	private(set) var isSubmitting = false

	func isArmed(_ target: ResetCreditUseTarget) -> Bool {
		armedAttempt?.target == target
	}

	func isArmed(_ attempt: ResetCreditUseAttempt) -> Bool {
		armedAttempt == attempt
	}

	func isPreparing(_ target: ResetCreditUseTarget) -> Bool {
		isPreparing && pendingPreparation?.target == target
	}

	func isSubmitting(_ target: ResetCreditUseTarget) -> Bool {
		isSubmitting && isArmed(target)
	}

	var isBusy: Bool {
		isPreparing || isSubmitting
	}

	mutating func tap(
		_ target: ResetCreditUseTarget,
		makeIdempotencyKey: () -> String = { UUID().uuidString }
	) -> ResetCreditUseAction? {
		guard isBusy == false else {
			return nil
		}

		if let armedAttempt, armedAttempt.target == target {
			isSubmitting = true
			return .consume(armedAttempt)
		}

		let preparation = ResetCreditUsePreparation(
			target: target,
			idempotencyKey: makeIdempotencyKey()
		)
		pendingPreparation = preparation
		armedAttempt = nil
		isPreparing = true

		return .prepare(preparation)
	}

	@discardableResult
	mutating func finishPreparation(
		_ preparation: ResetCreditUsePreparation,
		creditID: String?
	) -> ResetCreditUseAttempt? {
		guard pendingPreparation == preparation else {
			return nil
		}

		pendingPreparation = nil
		isPreparing = false

		guard let creditID = creditID?
			.trimmingCharacters(in: .whitespacesAndNewlines),
			creditID.isEmpty == false
		else {
			return nil
		}

		let attempt = ResetCreditUseAttempt(
			target: preparation.target,
			creditID: creditID,
			idempotencyKey: preparation.idempotencyKey
		)
		armedAttempt = attempt
		return attempt
	}

	mutating func finish(_ attempt: ResetCreditUseAttempt, resolved: Bool) {
		guard armedAttempt == attempt else {
			return
		}

		isSubmitting = false
		if resolved {
			armedAttempt = nil
		}
	}

	@discardableResult
	mutating func disarm(_ attempt: ResetCreditUseAttempt) -> Bool {
		guard isSubmitting == false, armedAttempt == attempt else {
			return false
		}

		armedAttempt = nil
		return true
	}

	mutating func cancelPendingConfirmation() {
		guard isSubmitting == false else {
			return
		}

		pendingPreparation = nil
		armedAttempt = nil
		isPreparing = false
	}

	mutating func retainOnly(_ targets: Set<ResetCreditUseTarget>) {
		if let armedAttempt, targets.contains(armedAttempt.target) == false {
			self.armedAttempt = nil
			isSubmitting = false
		}

		if let pendingPreparation,
			targets.contains(pendingPreparation.target) == false
		{
			self.pendingPreparation = nil
			isPreparing = false
		}
	}
}
