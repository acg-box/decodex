import Foundation

extension AccountStore {
	func beginOptimisticLogoutRemoval(_ account: CodexAccount) {
		pendingLogoutRemovalKeys.formUnion(Self.logoutRemovalKeys(for: account))
	}

	func cancelOptimisticLogoutRemoval(_ account: CodexAccount) {
		pendingLogoutRemovalKeys.subtract(Self.logoutRemovalKeys(for: account))
	}

	func applyAccountList(_ response: AccountListResponse) {
		accountList = response
		reconcilePendingLogoutRemovals(with: response.accounts)
	}

	private func reconcilePendingLogoutRemovals(with accounts: [CodexAccount]) {
		guard pendingLogoutRemovalKeys.isEmpty == false else {
			return
		}

		let visibleKeys = accounts.reduce(into: Set<String>()) { keys, account in
			keys.formUnion(Self.logoutRemovalKeys(for: account))
		}
		pendingLogoutRemovalKeys = pendingLogoutRemovalKeys.intersection(visibleKeys)
	}

	func isLogoutRemovalPending(for account: CodexAccount) -> Bool {
		Self.logoutRemovalKeys(for: account).isDisjoint(with: pendingLogoutRemovalKeys) == false
	}

	private static func logoutRemovalKeys(for account: CodexAccount) -> Set<String> {
		[
			account.id,
			account.selector,
			account.email,
			account.accountFingerprint,
		]
		.compactMap { value in
			guard let key = value?.trimmingCharacters(in: .whitespacesAndNewlines),
				key.isEmpty == false
			else {
				return nil
			}
			return key
		}
		.reduce(into: Set<String>()) { keys, key in
			keys.insert(key)
		}
	}
}
