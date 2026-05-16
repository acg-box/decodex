import Foundation

@MainActor
final class AccountStore: ObservableObject {
	@Published private(set) var accountList: AccountListResponse?
	@Published private(set) var isRefreshing = false
	@Published private(set) var isLoggingIn = false
	@Published var loginTranscript = ""
	@Published var notice: String?

	private let cli = DecodexCLI()

	var accounts: [CodexAccount] {
		accountList?.accounts ?? []
	}

	var modeLabel: String {
		guard let control = accountList?.control else {
			return "Not loaded"
		}

		if let selector = control.accountSelector, !selector.isEmpty {
			return "Fixed: \(selector)"
		}

		return control.mode == "balanced" ? "Balanced" : control.mode.capitalized
	}

	var menuSymbol: String {
		if accounts.contains(where: \.selected) {
			return "person.crop.circle.badge.checkmark"
		}
		if accounts.isEmpty {
			return "person.crop.circle.badge.plus"
		}

		return "person.2.circle"
	}

	func refresh() async {
		isRefreshing = true
		defer {
			isRefreshing = false
		}

		do {
			accountList = try await cli.runJSON(["account", "list", "--json"], as: AccountListResponse.self)
			notice = nil
		} catch {
			notice = error.localizedDescription
		}
	}

	func select(_ account: CodexAccount) async {
		do {
			if account.selected {
				_ = try await cli.run(["account", "clear", "--json"])
			} else {
				_ = try await cli.run(["account", "select", account.selector, "--json"])
			}

			await refresh()
		} catch {
			notice = error.localizedDescription
		}
	}

	func clearSelection() async {
		do {
			_ = try await cli.run(["account", "clear", "--json"])
			await refresh()
		} catch {
			notice = error.localizedDescription
		}
	}

	func logout(_ account: CodexAccount) async {
		do {
			_ = try await cli.run(["account", "logout", account.selector, "--json"])
			await refresh()
		} catch {
			notice = error.localizedDescription
		}
	}

	func login() async {
		isLoggingIn = true
		loginTranscript = ""
		notice = nil

		do {
			try await cli.runStreaming(["account", "login"]) { [weak self] chunk in
				self?.loginTranscript += chunk
			}
			await refresh()
		} catch {
			notice = error.localizedDescription
		}

		isLoggingIn = false
	}
}
