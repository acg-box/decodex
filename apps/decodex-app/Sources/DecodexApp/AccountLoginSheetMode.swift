import Foundation

enum AccountLoginSheetMode: Equatable {
	case newAccount
	case account(String)

	var title: String {
		"Login"
	}

	var icon: String {
		"person.crop.circle.badge.plus"
	}

	func subtitle(fallback: String, isActive: Bool) -> String {
		switch self {
		case .newAccount:
			return fallback
		case .account(let name):
			return isActive == false && name.isEmpty == false ? name : fallback
		}
	}
}
