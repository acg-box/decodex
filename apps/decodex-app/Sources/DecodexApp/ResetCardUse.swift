import Foundation

struct ResetCardDescriptor: Codable, Hashable, Sendable {
	let grantedAtUnixSeconds: Int64
	let expiresAtUnixSeconds: Int64

	init(grantedAtUnixSeconds: Int64, expiresAtUnixSeconds: Int64) throws {
		guard grantedAtUnixSeconds >= 0,
			expiresAtUnixSeconds > grantedAtUnixSeconds
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
		makeIdempotencyKey: () -> String = { UUID().uuidString.lowercased() }
	) -> ResetCardUseAttempt? {
		guard isSubmitting == false else {
			return nil
		}

		if let armedAttempt, armedAttempt.target == target {
			isSubmitting = true
			return armedAttempt
		}

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
	}

	@discardableResult
	mutating func disarm(_ attempt: ResetCardUseAttempt) -> Bool {
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

	mutating func retainOnly(_ targets: Set<ResetCardUseTarget>) {
		guard isSubmitting == false else {
			return
		}

		if let armedAttempt, targets.contains(armedAttempt.target) == false {
			self.armedAttempt = nil
		}
	}
}
