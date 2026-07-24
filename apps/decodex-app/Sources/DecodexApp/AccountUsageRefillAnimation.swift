import Foundation

enum AccountUsageMeterSlot: Sendable {
	case primary
	case secondary
}

struct AccountUsageMeterRefillAnimation: Equatable, Identifiable, Sendable {
	let eventID: UUID
	let fromPercent: Int

	var id: UUID {
		eventID
	}
}

struct AccountUsageRefillAnimation: Equatable, Identifiable, Sendable {
	let id: UUID
	let accountID: String
	let primaryFromPercent: Int?
	let secondaryFromPercent: Int?

	init(
		id: UUID = UUID(),
		accountID: String,
		primaryFromPercent: Int?,
		secondaryFromPercent: Int?
	) {
		self.id = id
		self.accountID = accountID
		self.primaryFromPercent = Self.refillCandidate(primaryFromPercent)
		self.secondaryFromPercent = Self.refillCandidate(secondaryFromPercent)
	}

	static func make(
		from account: CodexAccount,
		id: UUID = UUID()
	) -> Self? {
		let animation = Self(
			id: id,
			accountID: account.accountFingerprint,
			primaryFromPercent: account.primaryRemainingPercent,
			secondaryFromPercent: account.secondaryRemainingPercent
		)

		return animation.primaryFromPercent != nil
			|| animation.secondaryFromPercent != nil
			? animation
			: nil
	}

	func meterAnimation(
		for slot: AccountUsageMeterSlot,
		currentPercent: Int?
	) -> AccountUsageMeterRefillAnimation? {
		guard let currentPercent, currentPercent >= 100 else {
			return nil
		}

		let fromPercent = switch slot {
		case .primary:
			primaryFromPercent
		case .secondary:
			secondaryFromPercent
		}
		guard let fromPercent else {
			return nil
		}

		return AccountUsageMeterRefillAnimation(
			eventID: id,
			fromPercent: fromPercent
		)
	}

	private static func refillCandidate(_ percent: Int?) -> Int? {
		guard let percent, percent < 100 else {
			return nil
		}

		return percent
	}
}

extension AccountStore {
	func beginUsageRefillAnimation(_ animation: AccountUsageRefillAnimation?) {
		guard let animation else {
			return
		}

		usageRefillCleanupTasks[animation.accountID]?.cancel()
		usageRefillCleanupTasks[animation.accountID] = nil
		usageRefillAnimations[animation.accountID] = animation
	}

	func finishUsageRefillAnimation(
		_ animation: AccountUsageRefillAnimation?,
		refreshSucceeded: Bool
	) {
		guard let animation,
			usageRefillAnimations[animation.accountID]?.id == animation.id
		else {
			return
		}
		guard refreshSucceeded else {
			usageRefillCleanupTasks[animation.accountID]?.cancel()
			usageRefillCleanupTasks[animation.accountID] = nil
			usageRefillAnimations[animation.accountID] = nil
			return
		}

		usageRefillCleanupTasks[animation.accountID] = Task { [weak self] in
			do {
				try await Task.sleep(for: .seconds(2))
			} catch {
				return
			}
			guard self?.usageRefillAnimations[animation.accountID]?.id == animation.id else {
				return
			}

			self?.usageRefillAnimations[animation.accountID] = nil
			self?.usageRefillCleanupTasks[animation.accountID] = nil
		}
	}

	func cancelUsageRefillAnimation(for accountID: String) {
		usageRefillCleanupTasks[accountID]?.cancel()
		usageRefillCleanupTasks[accountID] = nil
		usageRefillAnimations[accountID] = nil
	}
}
