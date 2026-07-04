#!/usr/bin/env node

import path from "node:path";
import { fileURLToPath } from "node:url";

import { parseArgs, splitListenAddress } from "./operator-dashboard-mock/args.mjs";
import { codexAuthAccounts } from "./operator-dashboard-mock/auth.mjs";
import { mockAccounts } from "./operator-dashboard-mock/fixtures.mjs";
import { createMockDashboardServer } from "./operator-dashboard-mock/server/index.mjs";
import { createMockServerState } from "./operator-dashboard-mock/server/state.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function main() {
	const options = parseArgs(process.argv.slice(2), repoRoot);
	const staticAccounts = options.authDir
		? await codexAuthAccounts(options.authDir)
		: mockAccounts();
	const state = createMockServerState(staticAccounts);
	const { host, port } = splitListenAddress(options.listenAddress);
	const server = createMockDashboardServer({
		options,
		staticAccounts,
		state,
	});

	server.listen(port, host, () => {
		const baseUrl = `http://${host}:${port}`;
		const webSocketUrl = `ws://${host}:${port}/dashboard/control`;
		console.log(`operator dashboard mock: ${baseUrl}/dashboard`);
		console.log(`operator dashboard websocket: ${webSocketUrl}`);
		console.log(`Decodex App mock base: DECODEX_APP_SERVER_URL=${baseUrl}`);
		console.log("preview invariant: browser dashboard and Decodex App must use this same mock server");
		console.log(
			options.authDir
				? `accounts: ${options.authDir} (${staticAccounts.length} loaded)`
				: `accounts: ${staticAccounts.length} synthetic fixture accounts`,
		);
	});
}

main().catch((error) => {
	console.error(error?.message || error);
	process.exit(1);
});
