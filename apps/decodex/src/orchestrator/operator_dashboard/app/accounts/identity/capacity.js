

				function codexAccountCapacityMultiplier(account) {
					const explicit = codexAccountNumber(account?.capacity_multiplier);
					if (explicit != null && explicit > 0) {
						return explicit;
					}

					const planType = String(account?.plan_type || "").trim().toLowerCase();
					return planType === "pro" ? 20 : 1;
				}

				function codexAccountCapacityLabel(account) {
					return `${codexAccountCapacityMultiplier(account)}x`;
				}

				function codexAccountUsageRecordCapacityMultiplier(account, record) {
					const explicit = codexAccountNumber(record?.capacity_multiplier);

					return explicit != null && explicit > 0
						? explicit
						: codexAccountCapacityMultiplier(account);
				}