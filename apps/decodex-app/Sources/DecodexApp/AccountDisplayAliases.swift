import Foundation

extension AccountDisplay {
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
}
