import http from "node:http";

import { handleDashboardUpgrade } from "./control-socket.mjs";
import { handleDashboardRequest } from "./http-routes.mjs";

export function createMockDashboardServer(context) {
	const server = http.createServer((request, response) => {
		handleDashboardRequest(context, request, response);
	});
	server.on("upgrade", (request, socket) => {
		handleDashboardUpgrade(context, request, socket);
	});
	return server;
}
