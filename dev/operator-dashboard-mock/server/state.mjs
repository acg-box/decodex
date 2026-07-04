import { nowUnix } from "../time.mjs";

export function createMockServerState(staticAccounts) {
	return {
		fixedAccountSelector: initialFixedAccountSelector(staticAccounts),
		lastPublishedAt: nowUnix(),
	};
}

function initialFixedAccountSelector(staticAccounts) {
	return (
		staticAccounts.find((item) => item.status === "selected")?.account_email ||
		staticAccounts[0]?.account_email ||
		null
	);
}
