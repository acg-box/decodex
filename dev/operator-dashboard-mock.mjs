#!/usr/bin/env node

import http from "node:http";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
	accountMatchesSelector,
	accountsWithSelection,
	usageEstimate,
} from "./operator-dashboard-mock/accounts.mjs";
import { parseArgs, splitListenAddress } from "./operator-dashboard-mock/args.mjs";
import { codexAuthAccounts } from "./operator-dashboard-mock/auth.mjs";
import { mockAccounts } from "./operator-dashboard-mock/fixtures.mjs";
import { buildSnapshot } from "./operator-dashboard-mock/snapshot.mjs";
import { nowUnix } from "./operator-dashboard-mock/time.mjs";
import {
	dashboardControlAck,
	decodeWebSocketFrames,
	send,
	sendWebSocketJson,
	websocketAcceptValue,
} from "./operator-dashboard-mock/websocket.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function main() {
	const options = parseArgs(process.argv.slice(2), repoRoot);
	const staticAccounts = options.authDir
		? await codexAuthAccounts(options.authDir)
		: mockAccounts();
	let fixedAccountSelector =
		staticAccounts.find((item) => item.status === "selected")?.account_email ||
		staticAccounts[0]?.account_email ||
		null;
	let lastPublishedAt = nowUnix();
	const { host, port } = splitListenAddress(options.listenAddress);
	const server = http.createServer(async (request, response) => {
		try {
			if (request.method !== "GET") {
				send(response, 405, "text/plain; charset=utf-8", "method not allowed");
				return;
			}
			const url = new URL(request.url || "/", `http://${options.listenAddress}`);
			if (url.pathname === "/" || url.pathname === "/dashboard") {
				const html = await fs.readFile(options.dashboardHtml, "utf8");
				send(response, 200, "text/html; charset=utf-8", html);
				return;
			}
			if (url.pathname === "/api/accounts") {
				const controlledAccounts = accountsWithSelection(staticAccounts, fixedAccountSelector);
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
							mode: fixedAccountSelector ? "fixed" : "balanced",
							account_selector: fixedAccountSelector || null,
						},
						accounts: controlledAccounts,
						usage_estimate: usageEstimate(controlledAccounts),
						usage_probe_error: null,
					}),
				);
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
	});

	server.on("upgrade", (request, socket) => {
		const url = new URL(request.url || "/", `http://${options.listenAddress}`);
		if (url.pathname !== "/dashboard/control") {
			socket.destroy();
			return;
		}

		const key = request.headers["sec-websocket-key"];
		if (!key) {
			socket.destroy();
			return;
		}

		socket.write(
			[
				"HTTP/1.1 101 Switching Protocols",
				"Upgrade: websocket",
				"Connection: Upgrade",
				`Sec-WebSocket-Accept: ${websocketAcceptValue(key)}`,
				"",
				"",
			].join("\r\n"),
		);
		sendWebSocketJson(socket, {
			type: "controlReady",
			payload: {
				supportedActions: [
					"subscribe",
					"focus",
					"clearFocus",
					"pauseProject",
					"resumeProject",
					"interruptRun",
					"selectAccount",
					"clearAccountSelection",
					"ack",
				],
				subscription: {},
			},
		});
		sendWebSocketJson(socket, {
			type: "snapshot",
			payload: {
				snapshot: buildSnapshot(staticAccounts, fixedAccountSelector),
				snapshotPublishedAtUnixEpoch: lastPublishedAt,
			},
		});

		let buffered = Buffer.alloc(0);
		socket.on("data", (chunk) => {
			buffered = Buffer.concat([buffered, chunk]);
			const decoded = decodeWebSocketFrames(buffered);
			buffered = decoded.remaining;
			if (decoded.closed) {
				socket.end();
				return;
			}

			for (const text of decoded.messages) {
				let message;
				try {
					message = JSON.parse(text);
				} catch (_error) {
					sendWebSocketJson(socket, {
						type: "controlAck",
						payload: {
							requestId: null,
							action: "control",
							accepted: false,
							status: "invalid_json",
							message: "Mock dashboard control received invalid JSON.",
						},
					});
					continue;
				}

				if (message.type === "subscribe") {
					sendWebSocketJson(
						socket,
						dashboardControlAck(message, true, "accepted", "Mock subscription accepted."),
					);
					continue;
				}

				if (message.type !== "control") {
					sendWebSocketJson(
						socket,
						dashboardControlAck(
							message,
							false,
							"unsupported",
							"Mock dashboard control type is unsupported.",
						),
					);
					continue;
				}

				if (message.action === "selectAccount") {
					const selector = String(message.accountSelector || "").trim();
					if (!staticAccounts.some((item) => accountMatchesSelector(item, selector))) {
						sendWebSocketJson(
							socket,
							dashboardControlAck(
								message,
								false,
								"unknown_account",
								"Mock account selector was not found.",
							),
						);
						continue;
					}
					fixedAccountSelector = selector;
					lastPublishedAt = nowUnix();
					sendWebSocketJson(
						socket,
						dashboardControlAck(message, true, "accepted", "Mock account selection updated."),
					);
					sendWebSocketJson(socket, {
						type: "snapshot",
						payload: {
							snapshot: buildSnapshot(staticAccounts, fixedAccountSelector),
							snapshotPublishedAtUnixEpoch: lastPublishedAt,
						},
					});
					continue;
				}

				if (message.action === "clearAccountSelection") {
					fixedAccountSelector = null;
					lastPublishedAt = nowUnix();
					sendWebSocketJson(
						socket,
						dashboardControlAck(
							message,
							true,
							"accepted",
							"Mock account selection returned to balanced mode.",
						),
					);
					sendWebSocketJson(socket, {
						type: "snapshot",
						payload: {
							snapshot: buildSnapshot(staticAccounts, fixedAccountSelector),
							snapshotPublishedAtUnixEpoch: lastPublishedAt,
						},
					});
					continue;
				}

				sendWebSocketJson(
					socket,
					dashboardControlAck(
						message,
						false,
						"unsupported",
						"Mock dashboard control action is unsupported.",
					),
				);
			}
		});
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
