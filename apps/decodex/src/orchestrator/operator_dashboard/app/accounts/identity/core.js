				function codexAccountEmail(account) {
					return String(account?.account_email || account?.email || "").trim();
				}

				function compactAccountEmail(email) {
					const text = String(email || "").trim();
					const atIndex = text.indexOf("@");
					if (atIndex <= 0) {
						return compactAccountIdentity(text);
					}

					const local = text.slice(0, atIndex);
					const domain = text.slice(atIndex);
					if (local.length <= 6) {
						return `${local}${domain}`;
					}

					return `${local.slice(0, 3)}...${local.slice(-3)}${domain}`;
				}

				function trimLeadingEllipsis(value) {
					const text = String(value || "").trim();
					if (text.startsWith("...") && text.indexOf("...", 3) === -1) {
						return text.slice(3);
					}

					return text;
				}

				function compactAccountIdentity(value) {
					const text = trimLeadingEllipsis(value);
					if (!text || text === "unknown") {
						return text;
					}

					const edgeLength = Math.max(
						ACCOUNT_IDENTITY_MIN_EDGE_CHARS,
						Math.min(ACCOUNT_IDENTITY_EDGE_CHARS, Math.floor(text.length / 2)),
					);
					const headLength = edgeLength;
					const tailLength = edgeLength;
					return `${text.slice(0, headLength)}...${text.slice(-tailLength)}`;
				}

				function codexAccountIdentityHash(value) {
					const text = String(value || "account");
					let hash = 2_166_136_261;
					for (let index = 0; index < text.length; index += 1) {
						hash ^= text.charCodeAt(index);
						hash = Math.imul(hash, 16_777_619);
					}

					return hash >>> 0;
				}