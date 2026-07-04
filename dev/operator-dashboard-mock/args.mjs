import path from "node:path";

export const DEFAULT_LISTEN_ADDRESS = "127.0.0.1:57399";

export function parseArgs(argv, repoRoot) {
	const options = {
		authDir: null,
		dashboardHtml: path.join(repoRoot, "apps/decodex/src/orchestrator/operator_dashboard.html"),
		listenAddress: DEFAULT_LISTEN_ADDRESS,
	};

	for (let index = 0; index < argv.length; index += 1) {
		const arg = argv[index];
		if (arg === "--help" || arg === "-h") {
			printHelp();
			process.exit(0);
		}
		if (arg === "--listen-address") {
			options.listenAddress = requiredValue(argv, (index += 1), arg);
			continue;
		}
		if (arg === "--dashboard-html") {
			options.dashboardHtml = path.resolve(requiredValue(argv, (index += 1), arg));
			continue;
		}
		if (arg === "--codex-auth-dir") {
			options.authDir = path.resolve(requiredValue(argv, (index += 1), arg));
			continue;
		}
		if (arg === "--use-codex-auth") {
			options.authDir = path.join(process.env.HOME || ".", ".codex");
			continue;
		}
		throw new Error(`Unknown argument: ${arg}`);
	}

	return options;
}

function requiredValue(argv, index, flag) {
	const value = argv[index];
	if (!value || value.startsWith("--")) {
		throw new Error(`${flag} requires a value`);
	}

	return value;
}

function printHelp() {
	console.log(`Usage: node dev/operator-dashboard-mock.mjs [options]

Serves the real operator dashboard HTML, /api/accounts, and mock dashboard WebSocket
snapshot/activity events from one local base URL. Use the same mock base URL for the
browser dashboard and Decodex App previews; do not start a second mock server for the
App. The dashboard authority is ws://HOST:PORT/dashboard/control.

Options:
  --listen-address HOST:PORT   Bind address (default ${DEFAULT_LISTEN_ADDRESS})
  --dashboard-html PATH        Dashboard HTML path
  --use-codex-auth             Load auth*.json accounts from ~/.codex
  --codex-auth-dir DIR         Load auth*.json accounts from DIR
  -h, --help                   Show this help
`);
}

export function splitListenAddress(value) {
	const [host, portText] = value.split(":");
	const port = Number(portText);
	if (!host || !Number.isInteger(port) || port <= 0 || port > 65_535) {
		throw new Error(`Invalid listen address: ${value}`);
	}

	return { host, port };
}
