import Foundation

enum AccountDisplay {
	static func alias(for account: CodexAccount) -> String {
		randomNames[preferredNameIndex(for: account)]
	}

	static func aliases(for accounts: [CodexAccount]) -> [String: String] {
		var usedNames = Set<String>()
		var aliases = [String: String]()
		let orderedAccounts = accounts.sorted { left, right in
			aliasSortKey(for: left) < aliasSortKey(for: right)
		}

		for account in orderedAccounts {
			let alias = uniqueAlias(startingAt: preferredNameIndex(for: account), usedNames: usedNames)
			usedNames.insert(alias)
			aliases[account.id] = alias
		}

		return aliases
	}

	static func alias(forIdentity identity: String) -> String {
		let seed = identity.trimmingCharacters(in: .whitespacesAndNewlines)
		let hash = identityHash(seed.isEmpty ? "account" : seed)
		let index = Int(hash % UInt32(randomNames.count))

		return randomNames[index]
	}
}
