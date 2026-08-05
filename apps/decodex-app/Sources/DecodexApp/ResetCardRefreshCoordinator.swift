import Foundation

enum ResetCardRefreshWork: Equatable {
	case full
	case observation
	case skeleton
}

/// Coalesces refresh intent without losing a daemon observation that arrives
/// while another account operation is active.
struct ResetCardRefreshRequests: Equatable {
	private(set) var manualRefreshRequested = false
	private(set) var manualRefreshGeneration: UInt64 = 0
	private(set) var completedManualRefreshGeneration: UInt64 = 0
	private(set) var observationGeneration: UInt64 = 0
	private var consumedObservationGeneration: UInt64 = 0
	private(set) var skeletonGeneration: UInt64 = 0
	private var consumedSkeletonGeneration: UInt64 = 0

	var hasFullRefreshRequest: Bool {
		manualRefreshRequested
	}

	var hasObservationRefreshRequest: Bool {
		observationGeneration != consumedObservationGeneration
	}

	var hasSkeletonRefreshRequest: Bool {
		skeletonGeneration != consumedSkeletonGeneration
	}

	func didConsumeSkeletonRefresh(upTo generation: UInt64) -> Bool {
		consumedSkeletonGeneration >= generation
	}

	func didCompleteManualRefresh(upTo generation: UInt64) -> Bool {
		completedManualRefreshGeneration >= generation
	}

	var hasWork: Bool {
		hasFullRefreshRequest
			|| hasObservationRefreshRequest
			|| hasSkeletonRefreshRequest
	}

	@discardableResult
	mutating func requestManualRefresh() -> UInt64 {
		manualRefreshGeneration &+= 1
		manualRefreshRequested = true
		return manualRefreshGeneration
	}

	mutating func completeManualRefresh() {
		completedManualRefreshGeneration = manualRefreshGeneration
	}

	mutating func requestObservationRefresh() {
		observationGeneration &+= 1
	}

	@discardableResult
	mutating func requestSkeletonRefresh() -> UInt64 {
		skeletonGeneration &+= 1
		return skeletonGeneration
	}

	mutating func takeNext() -> ResetCardRefreshWork? {
		if hasFullRefreshRequest {
			manualRefreshRequested = false
			consumedObservationGeneration = observationGeneration
			return .full
		}
		if hasObservationRefreshRequest {
			consumedObservationGeneration = observationGeneration
			return .observation
		}
		if hasSkeletonRefreshRequest {
			consumedSkeletonGeneration = skeletonGeneration
			return .skeleton
		}
		return nil
	}

	mutating func reset() {
		self = Self()
	}
}
