import Foundation

enum AccountDisplay {
	static let randomNames = [
		"Alex",
		"Avery",
		"Bailey",
		"Blake",
		"Casey",
		"Charlie",
		"Clara",
		"Dana",
		"Drew",
		"Eden",
		"Elliot",
		"Emery",
		"Evan",
		"Finley",
		"Harper",
		"Hayden",
		"Iris",
		"Jamie",
		"Jordan",
		"Kai",
		"Kendall",
		"Lane",
		"Liam",
		"Logan",
		"Mason",
		"Maya",
		"Mia",
		"Morgan",
		"Noah",
		"Nora",
		"Owen",
		"Paige",
		"Parker",
		"Quinn",
		"Reese",
		"Remy",
		"Riley",
		"Rowan",
		"Sage",
		"Sasha",
		"Sidney",
		"Taylor",
		"Theo",
		"Val",
	]

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

	static func compactEmail(_ email: String) -> String {
		let text = email.trimmingCharacters(in: .whitespacesAndNewlines)
		guard let atIndex = text.firstIndex(of: "@"), atIndex > text.startIndex else {
			return compactIdentity(text)
		}

		let local = String(text[..<atIndex])
		let domain = String(text[atIndex...])
		if local.count <= 6 {
			return "\(local)\(domain)"
		}

		return "\(local.prefix(3))...\(compactLocalSuffix(local))\(domain)"
	}

	static func compactIdentity(_ value: String) -> String {
		let text = trimLeadingEllipsis(value)
		if text.isEmpty || text == "unknown" {
			return text
		}

		let edgeLength = max(3, min(6, text.count / 2))
		return "\(text.prefix(edgeLength))...\(text.suffix(edgeLength))"
	}

	static func aliasSortKey(for account: CodexAccount) -> String {
		if let key = account.randomNameKey?.trimmingCharacters(in: .whitespacesAndNewlines),
			key.isEmpty == false
		{
			return key
		}

		return account.randomNameSeed
	}

	static func preferredNameIndex(for account: CodexAccount) -> Int {
		if let randomName = account.randomName?.trimmingCharacters(in: .whitespacesAndNewlines),
			let index = randomNames.firstIndex(of: randomName)
		{
			return index
		}

		let hash = randomNameHash(for: account)
		let offset = normalizedOffset(account.randomNameOffset ?? 0)

		return (Int(hash % UInt32(randomNames.count)) + offset) % randomNames.count
	}

	static func randomNameHash(for account: CodexAccount) -> UInt32 {
		if let key = account.randomNameKey?.trimmingCharacters(in: .whitespacesAndNewlines),
			key.isEmpty == false,
			let hash = UInt32(key, radix: 16)
		{
			return hash
		}

		return identityHash(account.randomNameSeed)
	}

	static func normalizedOffset(_ offset: Int) -> Int {
		((offset % randomNames.count) + randomNames.count) % randomNames.count
	}

	static func uniqueAlias(startingAt startIndex: Int, usedNames: Set<String>) -> String {
		for probe in 0..<randomNames.count {
			let name = randomNames[(startIndex + probe) % randomNames.count]
			if usedNames.contains(name) == false {
				return name
			}
		}

		let baseName = randomNames[startIndex % randomNames.count]
		var suffix = 2
		while true {
			let name = "\(baseName) \(suffix)"
			if usedNames.contains(name) == false {
				return name
			}
			suffix += 1
		}
	}

	static func trimLeadingEllipsis(_ value: String) -> String {
		let text = value.trimmingCharacters(in: .whitespacesAndNewlines)
		if text.hasPrefix("..."), text.dropFirst(3).contains("...") == false {
			return String(text.dropFirst(3))
		}

		return text
	}

	static func compactLocalSuffix(_ local: String) -> String {
		if let separator = local.lastIndex(of: ".") {
			let segment = String(local[local.index(after: separator)...])
			if (2...4).contains(segment.count), segment.allSatisfy(\.isLetter) {
				return segment
			}
		}

		return String(local.suffix(3))
	}

	static func identityHash(_ value: String) -> UInt32 {
		var hash: UInt32 = 2_166_136_261
		for unit in value.utf16 {
			hash ^= UInt32(unit)
			hash = hash &* 16_777_619
		}

		return hash
	}
}
