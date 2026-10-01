import Foundation

struct ResetCardDescriptor: Codable, Hashable, Sendable {
	let grantedAtUnixSeconds: Int64
	let expiresAtUnixSeconds: Int64?

	init(grantedAtUnixSeconds: Int64, expiresAtUnixSeconds: Int64?) throws {
		guard grantedAtUnixSeconds >= 0,
			expiresAtUnixSeconds.map({ $0 > grantedAtUnixSeconds }) ?? true
		else {
			throw ResetCardClientError.invalidResponse
		}

		self.grantedAtUnixSeconds = grantedAtUnixSeconds
		self.expiresAtUnixSeconds = expiresAtUnixSeconds
	}
}

struct ResetCardAuthority: Codable, Hashable, Sendable {
	let profileName: String
	let serverID: String
}

struct ResetCardUseTarget: Codable, Hashable, Sendable {
	let authority: ResetCardAuthority
	let accountID: String
	let expectedRevision: UInt64
	let descriptor: ResetCardDescriptor
}

struct ResetCardUseAttempt: Codable, Equatable, Sendable {
	let target: ResetCardUseTarget
	let idempotencyKey: String
}

struct ResetCardUseCompletion: Equatable, Sendable {
	let resolved: Bool
}

struct ResetCardUseConfirmation: Equatable {
	static let windowSeconds = 5
	private(set) var deadline: ContinuousClock.Instant?
	private(set) var armedAttempt: ResetCardUseAttempt?
	private(set) var isSubmitting = false

	func isArmed(_ target: ResetCardUseTarget) -> Bool {
		armedAttempt?.target == target
	}

	func isArmed(_ attempt: ResetCardUseAttempt) -> Bool {
		armedAttempt == attempt
	}

	func isSubmitting(_ target: ResetCardUseTarget) -> Bool {
		isSubmitting && isArmed(target)
	}

	mutating func tap(
		_ target: ResetCardUseTarget,
		now: ContinuousClock.Instant = .now,
		makeIdempotencyKey: () -> String = { UUID().uuidString.lowercased() }
	) -> ResetCardUseAttempt? {
		guard isSubmitting == false else {
			return nil
		}

		if let armedAttempt, armedAttempt.target == target,
			let deadline, now < deadline {
			isSubmitting = true
			return armedAttempt
		}

		deadline = now.advanced(by: .seconds(Self.windowSeconds))
		armedAttempt = ResetCardUseAttempt(
			target: target,
			idempotencyKey: makeIdempotencyKey()
		)

		return nil
	}

	mutating func finish(
		_ attempt: ResetCardUseAttempt,
		completion _: ResetCardUseCompletion
	) {
		guard armedAttempt == attempt else {
			return
		}

		isSubmitting = false
		armedAttempt = nil
		deadline = nil
	}

	@discardableResult
	mutating func disarm(_ attempt: ResetCardUseAttempt) -> Bool {
		guard isSubmitting == false, armedAttempt == attempt else {
			return false
		}

		armedAttempt = nil
		deadline = nil
		return true
	}

	mutating func cancelPendingConfirmation() {
		guard isSubmitting == false else {
			return
		}

		armedAttempt = nil
		deadline = nil
	}

	mutating func retainOnly(_ targets: Set<ResetCardUseTarget>) {
		guard isSubmitting == false else {
			return
		}

		if let armedAttempt, targets.contains(armedAttempt.target) == false {
			self.armedAttempt = nil
			deadline = nil
		}
	}
}

/// Keep an unresolved operation on its card, even while inventory is refreshing.
enum ResetCardChipPresentation {
	static func targets(
		inventory: [ResetCardUseTarget], pending: [ResetCardUseAttempt],
		completed: ResetCardUseAttempt?, dismissedKey: String?
	) -> [ResetCardUseTarget] {
		var result = inventory
		for attempt in pending + (completed.map { [$0] } ?? []) {
			if !result.contains(where: { $0.descriptor == attempt.target.descriptor }) {
				result.append(attempt.target)
			}
		}
		if let completed, dismissedKey == completed.idempotencyKey {
			result.removeAll { $0.descriptor == completed.target.descriptor }
		}
		return result.sorted { left, right in
			guard let expiry = left.descriptor.expiresAtUnixSeconds else { return false }
			return right.descriptor.expiresAtUnixSeconds.map { expiry < $0 } ?? true
		}
	}
}
