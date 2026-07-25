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

struct ResetCreditUseAttempt: Equatable, Sendable {
	let target: ResetCreditUseTarget
	let idempotencyKey: String
	let creditID: String?

	func resolvingCreditID(_ creditID: String) -> ResetCreditUseAttempt {
		ResetCreditUseAttempt(
			target: target,
			idempotencyKey: idempotencyKey,
			creditID: creditID
		)
	}
}

struct ResetCreditUseCompletion: Equatable, Sendable {
	let resolved: Bool
	let creditID: String?
}

struct ResetCreditUseConfirmation: Equatable {
	private(set) var armedAttempt: ResetCreditUseAttempt?
	private(set) var isSubmitting = false

	func isArmed(_ target: ResetCreditUseTarget) -> Bool {
		armedAttempt?.target == target
	}

	func isArmed(_ attempt: ResetCreditUseAttempt) -> Bool {
		armedAttempt == attempt
	}

	func isSubmitting(_ target: ResetCreditUseTarget) -> Bool {
		isSubmitting && isArmed(target)
	}

	mutating func tap(
		_ target: ResetCreditUseTarget,
		makeIdempotencyKey: () -> String = { UUID().uuidString }
	) -> ResetCreditUseAttempt? {
		guard isSubmitting == false else {
			return nil
		}

		if let armedAttempt, armedAttempt.target == target {
			isSubmitting = true
			return armedAttempt
		}

		armedAttempt = ResetCreditUseAttempt(
			target: target,
			idempotencyKey: makeIdempotencyKey(),
			creditID: nil
		)

		return nil
	}

	mutating func finish(
		_ attempt: ResetCreditUseAttempt,
		completion: ResetCreditUseCompletion
	) {
		guard armedAttempt == attempt else {
			return
		}

		isSubmitting = false
		if completion.resolved {
			armedAttempt = nil
			return
		}

		if let creditID = completion.creditID?
			.trimmingCharacters(in: .whitespacesAndNewlines),
			creditID.isEmpty == false
		{
			armedAttempt = attempt.resolvingCreditID(creditID)
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

		armedAttempt = nil
	}

	mutating func retainOnly(_ targets: Set<ResetCreditUseTarget>) {
		guard isSubmitting == false else {
			return
		}

		if let armedAttempt, targets.contains(armedAttempt.target) == false {
			self.armedAttempt = nil
		}
	}
}
