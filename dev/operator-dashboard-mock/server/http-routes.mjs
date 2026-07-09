import {
	accountsWithSelection,
	usageEstimate,
} from "../accounts.mjs";
import { send } from "../websocket.mjs";

export async function handleDashboardRequest(context, request, response) {
	const { options } = context;
	try {
		if (request.method !== "GET") {
			send(response, 405, "text/plain; charset=utf-8", "method not allowed");
			return;
		}
		const url = new URL(request.url || "/", `http://${options.listenAddress}`);
		if (url.pathname === "/api/accounts") {
			sendAccountPayload(context, response);
			return;
		}
		if (url.pathname === "/livez") {
			send(response, 200, "text/plain; charset=utf-8", "ok");
			return;
		}

		send(response, 404, "text/plain; charset=utf-8", "not found");
	} catch (error) {
		send(response, 500, "text/plain; charset=utf-8", error?.message || "mock server error");
	}
}

function sendAccountPayload(context, response) {
	const { state, staticAccounts } = context;
	const controlledAccounts = accountsWithSelection(staticAccounts, state.fixedAccountSelector);
	send(
		response,
		200,
		"application/json; charset=utf-8",
		JSON.stringify({
			accounts_path: "/tmp/decodex-mock/accounts.jsonl",
			global_config_path: "/tmp/decodex-mock/config.toml",
			codex_auth_path: "/tmp/decodex-mock/auth.json",
			codex_auth: null,
			control: {
				mode: state.fixedAccountSelector ? "fixed" : "balanced",
				account_selector: state.fixedAccountSelector || null,
			},
			accounts: controlledAccounts,
			usage_estimate: usageEstimate(controlledAccounts),
			usage_probe_error: null,
		}),
	);
}
