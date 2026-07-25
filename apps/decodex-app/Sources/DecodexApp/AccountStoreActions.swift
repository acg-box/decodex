import Foundation

extension AccountStore {
	func useInCodex(_ account: CodexAccount) async {
		let previousAccountList = accountList
		clearNotice(source: .accountAction)
		accountList = accountList?.updatingCodexAuth(account.authIdentity)

		do {
			let response = try await bridge.runJSON(
				.accountUse(selector: account.selector),
				as: CodexAuthUseResponse.self
			)
			accountList = accountList?.updatingCodexAuth(response.account)
			clearNotice(source: .accountAction)
		} catch {
			accountList = previousAccountList
			presentError("Couldn’t switch Codex account", error: error)
		}
	}

	func select(_ account: CodexAccount) async {
		do {
			if account.selected {
				applyAccountList(try await bridge.runJSON(.accountClear, as: AccountListResponse.self))
			} else {
				applyAccountList(try await bridge.runJSON(
					.accountSelect(selector: account.selector),
					as: AccountListResponse.self
				))
			}
			clearNotice(source: .accountAction)
		} catch {
			presentError("Couldn’t update account routing", error: error)
		}
	}

	func clearSelection() async {
		do {
			applyAccountList(try await bridge.runJSON(.accountClear, as: AccountListResponse.self))
			clearNotice(source: .accountAction)
		} catch {
			presentError("Couldn’t update account routing", error: error)
		}
	}

	func setFastMode(_ enabled: Bool) async {
		guard isSettingFastMode == false else {
			return
		}

		let previous = fastMode
		fastMode = CodexFastModeResponse(
			codexConfigPath: previous?.codexConfigPath ?? "",
			enabled: enabled
		)
		isSettingFastMode = true
		defer {
			isSettingFastMode = false
		}

		do {
			fastMode = try await bridge.runJSON(
				.codexFastModeSet(enabled: enabled),
				as: CodexFastModeResponse.self
			)
			clearNotice(source: .fastMode)
		} catch {
			fastMode = previous
			presentError("Couldn’t update fast mode", error: error, source: .fastMode)
		}
	}

	func logout(_ account: CodexAccount) async throws {
		beginOptimisticLogoutRemoval(account)

		do {
			applyAccountList(try await bridge.runJSON(
				.accountLogout(selector: account.selector),
				as: AccountListResponse.self
			))
			clearNotice(source: .accountAction)
		} catch {
			cancelOptimisticLogoutRemoval(account)
			throw error
		}
	}

	@discardableResult
	func login() async -> Bool {
		isLoggingIn = true
		loginTranscript = ""
		clearLoginNotice()
		defer {
			isLoggingIn = false
		}

		do {
			let codexBin = try bridge.codexExecutablePath()
			applyAccountList(
				try await bridge.runStreaming(
					.accountLogin(codexBin: codexBin),
					as: AccountListResponse.self
				) { [weak self] chunk in
					self?.loginTranscript += chunk
				}
			)
			clearLoginNotice()
			await refreshFastMode()
			return true
		} catch {
			presentError(
				"Sign-in failed",
				error: error,
				scope: .signIn,
				source: .signIn
			)
			return false
		}
	}

	func consumeResetCredit(
		_ attempt: ResetCreditUseAttempt,
		for account: CodexAccount
	) async -> ResetCreditUseCompletion {
		cancelUsageRefillAnimation(for: account.accountFingerprint)
		guard attempt.target.accountID == account.accountFingerprint else {
			presentError(
				"Couldn’t use reset card",
				error: ResetCreditUseError.accountChanged,
				source: .resetCredit
			)
			return ResetCreditUseCompletion(resolved: false, creditID: attempt.creditID)
		}

		do {
			let currentAccount = try uniqueResetCreditAccount(matching: account)
			let refillAnimation = AccountUsageRefillAnimation.make(from: currentAccount)
			let codexHomeURL = try makeResetCreditCodexHome()
			defer {
				try? FileManager.default.removeItem(at: codexHomeURL)
			}

			let credentials = try await resetCreditCredentials(
				for: account,
				in: codexHomeURL
			)
			let codexExecutableURL = URL(fileURLWithPath: try bridge.codexExecutablePath())
			let result = try await CodexResetCreditBridge().consume(
				codexExecutableURL: codexExecutableURL,
				codexHomeURL: codexHomeURL,
				credentials: credentials,
				attempt: attempt
			)

			if result.outcome == .reset {
				beginUsageRefillAnimation(refillAnimation)
			}
			let refreshSucceeded = await refreshAccountsAfterResetCredit()
			finishUsageRefillAnimation(
				refillAnimation,
				refreshSucceeded: result.outcome == .reset && refreshSucceeded
			)
			if refreshSucceeded {
				presentNotice(.resetCreditOutcome(result.outcome))
			} else {
				let refreshError = notice?.copyText ?? "Account refresh failed."
				presentNotice(.resetCreditOutcome(result.outcome, refreshError: refreshError))
			}
			return ResetCreditUseCompletion(resolved: true, creditID: result.creditID)
		} catch {
			cancelUsageRefillAnimation(for: account.accountFingerprint)
			presentError("Couldn’t use reset card", error: error, source: .resetCredit)
			return ResetCreditUseCompletion(
				resolved: false,
				creditID: (error as? CodexResetCreditUseFailure)?.creditID ?? attempt.creditID
			)
		}
	}

	private func refreshAccountsAfterResetCredit() async -> Bool {
		while isRefreshing {
			guard Task.isCancelled == false else {
				return false
			}

			try? await Task.sleep(for: .milliseconds(25))
		}

		return await refresh(force: true)
	}

	private func resetCreditCredentials(
		for account: CodexAccount,
		in codexHomeURL: URL
	) async throws -> CodexResetCreditCredentials {
		let account = try uniqueResetCreditAccount(matching: account)
		let expectedEmail = normalizedEmail(account.email)

		let exportedAuthURL = codexHomeURL.appendingPathComponent("selected-account.json")
		defer {
			try? FileManager.default.removeItem(at: exportedAuthURL)
		}

		let authResponse = try await bridge.runJSON(
			.accountUse(
				selector: account.selector,
				authJsonPath: exportedAuthURL.path
			),
			as: CodexAuthUseResponse.self
		)
		if let expectedEmail,
			normalizedEmail(authResponse.account.email) != expectedEmail
		{
			throw ResetCreditUseError.accountChanged
		}
		guard pathsReferToSameLocation(authResponse.codexAuthPath, exportedAuthURL.path) else {
			throw ResetCreditUseError.authPathChanged
		}
		guard try exportedAuthURL
			.resourceValues(forKeys: [.isRegularFileKey])
			.isRegularFile == true
		else {
			throw ResetCreditUseError.authPathChanged
		}

		let authData = try Data(contentsOf: exportedAuthURL)
		let auth = try JSONDecoder().decode(ResetCreditStoredAuth.self, from: authData)
		let accessToken = auth.tokens.accessToken
			.trimmingCharacters(in: .whitespacesAndNewlines)
		let accountID = auth.tokens.accountID
			.trimmingCharacters(in: .whitespacesAndNewlines)
		guard accessToken.isEmpty == false, accountID.isEmpty == false else {
			throw ResetCreditUseError.invalidAuth
		}
		guard resetCreditFingerprint(for: accountID) == account.accountFingerprint else {
			throw ResetCreditUseError.accountChanged
		}

		let stagedAuthData = try auth.stagedData(
			refreshToken: "decodex-disabled-refresh-\(UUID().uuidString)"
		)
		let stagedAuthURL = codexHomeURL.appendingPathComponent("auth.json")
		guard FileManager.default.createFile(
			atPath: stagedAuthURL.path,
			contents: stagedAuthData,
			attributes: [.posixPermissions: 0o600]
		) else {
			throw ResetCreditUseError.authPathChanged
		}

		return CodexResetCreditCredentials(expectedEmail: expectedEmail)
	}

	func uniqueResetCreditAccount(matching account: CodexAccount) throws -> CodexAccount {
		let expectedEmail = normalizedEmail(account.email)
		let matches = accounts.filter { candidate in
			guard candidate.accountFingerprint == account.accountFingerprint else {
				return false
			}
			guard let expectedEmail else {
				return true
			}

			return normalizedEmail(candidate.email) == expectedEmail
		}

		guard matches.isEmpty == false else {
			throw ResetCreditUseError.accountChanged
		}
		guard matches.count == 1 else {
			throw ResetCreditUseError.accountAmbiguous
		}

		return matches[0]
	}

	private func normalizedEmail(_ value: String?) -> String? {
		guard let email = value?
			.trimmingCharacters(in: .whitespacesAndNewlines)
			.lowercased(),
			email.isEmpty == false
		else {
			return nil
		}

		return email
	}

	private func resetCreditFingerprint(for accountID: String) -> String {
		let tail = String(accountID.suffix(6))
		return tail.isEmpty ? "unknown" : "...\(tail)"
	}

	private func makeResetCreditCodexHome() throws -> URL {
		let url = FileManager.default.temporaryDirectory
			.appendingPathComponent("decodex-reset-credit-\(UUID().uuidString)", isDirectory: true)
		try FileManager.default.createDirectory(
			at: url,
			withIntermediateDirectories: false,
			attributes: [.posixPermissions: 0o700]
		)
		return url
	}

	private func pathsReferToSameLocation(_ left: String, _ right: String) -> Bool {
		URL(fileURLWithPath: left)
			.standardizedFileURL
			.resolvingSymlinksInPath()
			== URL(fileURLWithPath: right)
				.standardizedFileURL
				.resolvingSymlinksInPath()
	}

}

private enum ResetCreditUseError: LocalizedError {
	case accountAmbiguous
	case accountChanged
	case authPathChanged
	case invalidAuth

	var errorDescription: String? {
		switch self {
		case .accountAmbiguous:
			return "More than one stored account matches this reset card. Remove the duplicate account and try again."
		case .accountChanged:
			return "The account changed. Refresh and try again."
		case .authPathChanged:
			return "Could not create an isolated Codex account session."
		case .invalidAuth:
			return "The selected account has no usable Codex access token."
		}
	}
}

struct ResetCreditStoredAuth: Decodable {
	let email: String?
	let authMode: String?
	let lastRefresh: String?
	let tokens: Tokens

	struct Tokens: Decodable {
		let email: String?
		let idToken: String?
		let accessToken: String
		let refreshToken: String
		let accountID: String

		enum CodingKeys: String, CodingKey {
			case email
			case idToken = "id_token"
			case accessToken = "access_token"
			case refreshToken = "refresh_token"
			case accountID = "account_id"
		}
	}

	enum CodingKeys: String, CodingKey {
		case email
		case authMode = "auth_mode"
		case lastRefresh = "last_refresh"
		case tokens
	}

	func stagedData(refreshToken: String) throws -> Data {
		try JSONEncoder().encode(
			StagedAuth(
				email: email,
				authMode: "chatgpt",
				lastRefresh: lastRefresh,
				tokens: StagedAuth.Tokens(
					email: tokens.email,
					idToken: tokens.idToken,
					accessToken: tokens.accessToken,
					refreshToken: refreshToken,
					accountID: tokens.accountID
				)
			)
		)
	}

	private struct StagedAuth: Encodable {
		let email: String?
		let authMode: String
		let lastRefresh: String?
		let tokens: Tokens

		struct Tokens: Encodable {
			let email: String?
			let idToken: String?
			let accessToken: String
			let refreshToken: String
			let accountID: String

			enum CodingKeys: String, CodingKey {
				case email
				case idToken = "id_token"
				case accessToken = "access_token"
				case refreshToken = "refresh_token"
				case accountID = "account_id"
			}
		}

		enum CodingKeys: String, CodingKey {
			case email
			case authMode = "auth_mode"
			case lastRefresh = "last_refresh"
			case tokens
		}
	}
}
