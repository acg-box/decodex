import path from "node:path";

export const DEFAULT_LISTEN_ADDRESS = "127.0.0.1:57399";

export function parseArgs(argv) {
	const options = {
		authDir: null,
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

Serves /api/accounts, /livez, and the mock operator WebSocket used by Decodex App
previews. The operator stream is ws://HOST:PORT/dashboard/control.

Options:
  --listen-address HOST:PORT   Bind address (default ${DEFAULT_LISTEN_ADDRESS})
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
