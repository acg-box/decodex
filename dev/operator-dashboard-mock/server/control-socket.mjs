import { accountMatchesSelector } from "../accounts.mjs";
import { buildSnapshot } from "../snapshot.mjs";
import { nowUnix } from "../time.mjs";
import {
	dashboardControlAck,
	decodeWebSocketFrames,
	sendWebSocketJson,
	websocketAcceptValue,
} from "../websocket.mjs";

const SUPPORTED_ACTIONS = [
	"subscribe",
	"focus",
	"clearFocus",
	"pauseProject",
	"resumeProject",
	"interruptRun",
	"selectAccount",
	"clearAccountSelection",
	"ack",
];

export function handleDashboardUpgrade(context, request, socket) {
	const { options } = context;
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
	sendControlReady(socket);
	sendSnapshot(socket, context);

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
			handleControlText(context, socket, text);
		}
	});
}

function sendControlReady(socket) {
	sendWebSocketJson(socket, {
		type: "controlReady",
		payload: {
			supportedActions: SUPPORTED_ACTIONS,
			subscription: {},
		},
	});
}

function sendSnapshot(socket, context) {
	const { state, staticAccounts } = context;
	sendWebSocketJson(socket, {
		type: "snapshot",
		payload: {
			snapshot: buildSnapshot(staticAccounts, state.fixedAccountSelector),
			snapshotPublishedAtUnixEpoch: state.lastPublishedAt,
		},
	});
}

function handleControlText(context, socket, text) {
	let message;
	try {
		message = JSON.parse(text);
	} catch (_error) {
		sendInvalidJson(socket);
		return;
	}

	if (message.type === "subscribe") {
		sendWebSocketJson(
			socket,
			dashboardControlAck(message, true, "accepted", "Mock subscription accepted."),
		);
		return;
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
		return;
	}

	handleControlAction(context, socket, message);
}

function sendInvalidJson(socket) {
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
}

function handleControlAction(context, socket, message) {
	const { state, staticAccounts } = context;
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
			return;
		}
		state.fixedAccountSelector = selector;
		state.lastPublishedAt = nowUnix();
		sendWebSocketJson(
			socket,
			dashboardControlAck(message, true, "accepted", "Mock account selection updated."),
		);
		sendSnapshot(socket, context);
		return;
	}

	if (message.action === "clearAccountSelection") {
		state.fixedAccountSelector = null;
		state.lastPublishedAt = nowUnix();
		sendWebSocketJson(
			socket,
			dashboardControlAck(
				message,
				true,
				"accepted",
				"Mock account selection returned to balanced mode.",
			),
		);
		sendSnapshot(socket, context);
		return;
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
