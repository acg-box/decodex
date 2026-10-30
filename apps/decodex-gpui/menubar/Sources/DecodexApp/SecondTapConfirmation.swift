import Foundation

/// A short-lived confirmation owned by one destructive action.
struct SecondTapConfirmation {
	private(set) var deadline: ContinuousClock.Instant?

	func isArmed(at now: ContinuousClock.Instant = .now) -> Bool {
		deadline.map { now < $0 } ?? false
	}

	mutating func tap(at now: ContinuousClock.Instant = .now) -> Bool {
		if isArmed(at: now) {
			cancel()
			return true
		}
		deadline = now.advanced(by: .seconds(5))
		return false
	}

	mutating func cancel() {
		deadline = nil
	}
}
