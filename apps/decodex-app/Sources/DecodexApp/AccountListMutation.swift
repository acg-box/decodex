import Foundation

extension AccountListResponse {
	func updatingCodexAuth(_ identity: CodexAuthIdentity) -> AccountListResponse {
		AccountListResponse(
			accountsPath: accountsPath,
			globalConfigPath: globalConfigPath,
			codexAuthPath: codexAuthPath,
			codexAuth: identity,
			control: control,
			accounts: accounts.map { account in
				account.withCodexActive(account.matchesSelector(identity.selector))
			},
			usageEstimate: usageEstimate,
			usageProbeError: usageProbeError
		)
	}
}
