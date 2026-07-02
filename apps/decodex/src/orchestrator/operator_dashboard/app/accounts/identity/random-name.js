

				function codexAccountRandomNameKey(account) {
					const serverKey = String(account?.random_name_key || "").trim();
					if (serverKey) {
						return serverKey;
					}

					const identity =
						codexAccountIdentity(account) ||
						codexAccountEmail(account) ||
						account?.plan_type ||
						"account";

					return codexAccountIdentityHash(identity).toString(16).padStart(8, "0");
				}

				function codexAccountPendingRandomNameOffset(account) {
					const key = codexAccountRandomNameKey(account);
					if (!Object.prototype.hasOwnProperty.call(pendingAccountNameOffsets, key)) {
						return null;
					}

					return normalizeAccountNameOffset(pendingAccountNameOffsets[key]);
				}

				function codexAccountServerRandomNameOffset(account) {
					const value = Number(account?.random_name_offset);

					return Number.isInteger(value) ? normalizeAccountNameOffset(value) : null;
				}

				function codexAccountRandomNameOffset(account) {
					return (
						codexAccountPendingRandomNameOffset(account) ??
						codexAccountServerRandomNameOffset(account) ??
						0
					);
				}

				function codexAccountRandomName(account) {
					const pendingOffset = codexAccountPendingRandomNameOffset(account);
					const serverName = String(account?.random_name || "").trim();
					if (pendingOffset == null && serverName) {
						return serverName;
					}

					const seed =
						codexAccountIdentity(account) ||
						codexAccountEmail(account) ||
						account?.plan_type ||
						"account";
					const hash = codexAccountIdentityHash(seed);
					const index =
						(hash + codexAccountRandomNameOffset(account)) % ACCOUNT_RANDOM_NAMES.length;

					return ACCOUNT_RANDOM_NAMES[index];
				}