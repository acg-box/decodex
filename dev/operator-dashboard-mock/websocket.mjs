import crypto from "node:crypto";

export function send(response, statusCode, contentType, body, headers = {}) {
	response.writeHead(statusCode, {
		"content-type": contentType,
		"content-length": Buffer.byteLength(body),
		"cache-control": "no-store",
		...headers,
	});
	response.end(body);
}

export function websocketAcceptValue(key) {
	return crypto
		.createHash("sha1")
		.update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
		.digest("base64");
}

export function encodeWebSocketText(payload) {
	const body = Buffer.from(JSON.stringify(payload), "utf8");
	if (body.length <= 125) {
		return Buffer.concat([Buffer.from([0x81, body.length]), body]);
	}
	if (body.length <= 65_535) {
		const header = Buffer.alloc(4);
		header[0] = 0x81;
		header[1] = 126;
		header.writeUInt16BE(body.length, 2);
		return Buffer.concat([header, body]);
	}

	const header = Buffer.alloc(10);
	header[0] = 0x81;
	header[1] = 127;
	header.writeBigUInt64BE(BigInt(body.length), 2);
	return Buffer.concat([header, body]);
}

export function sendWebSocketJson(socket, payload) {
	if (socket.destroyed) {
		return;
	}
	socket.write(encodeWebSocketText(payload));
}

export function decodeWebSocketFrames(buffer) {
	const messages = [];
	let offset = 0;
	let closed = false;

	while (buffer.length - offset >= 2) {
		const first = buffer[offset];
		const second = buffer[offset + 1];
		const opcode = first & 0x0f;
		const masked = (second & 0x80) === 0x80;
		let length = second & 0x7f;
		let headerLength = 2;

		if (length === 126) {
			if (buffer.length - offset < 4) {
				break;
			}
			length = buffer.readUInt16BE(offset + 2);
			headerLength = 4;
		} else if (length === 127) {
			if (buffer.length - offset < 10) {
				break;
			}
			length = Number(buffer.readBigUInt64BE(offset + 2));
			headerLength = 10;
		}

		const maskLength = masked ? 4 : 0;
		const frameLength = headerLength + maskLength + length;
		if (buffer.length - offset < frameLength) {
			break;
		}

		const mask = masked
			? buffer.subarray(offset + headerLength, offset + headerLength + 4)
			: null;
		const payloadStart = offset + headerLength + maskLength;
		const payload = Buffer.from(buffer.subarray(payloadStart, payloadStart + length));
		if (mask) {
			for (let index = 0; index < payload.length; index += 1) {
				payload[index] ^= mask[index % 4];
			}
		}

		if (opcode === 0x8) {
			closed = true;
			offset += frameLength;
			break;
		}
		if (opcode === 0x1) {
			messages.push(payload.toString("utf8"));
		}
		offset += frameLength;
	}

	return {
		closed,
		messages,
		remaining: buffer.subarray(offset),
	};
}

export function dashboardControlAck(message, accepted, status, copy) {
	return {
		type: "controlAck",
		payload: {
			requestId: message.requestId || null,
			action: message.action || message.type || "control",
			accepted,
			status,
			message: copy,
			projectId: message.projectId || null,
			issueId: message.issueId || null,
			runId: message.runId || null,
		},
	};
}

