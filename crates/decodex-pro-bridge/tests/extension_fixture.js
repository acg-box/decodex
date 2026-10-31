// Browser API fixture. No network or user browser state is accessed.
var testDone = false, testError = "";
var URL = function(raw) { const p = parseURL(raw); if (!p) throw Error("Invalid URL"); Object.assign(this, p); };
var setTimeout = function() { return 1; }, clearTimeout = function() {};
const callbacks = {};
const event = name => ({addListener: cb => { callbacks[name] = cb; }});
const records = new Map([[1, {id: 1, url: "https://chatgpt.com/", windowId: 1}]]);
const storage = {};
const frames = [];
let created = 0, nativeConnections = 0, commands = 0, pendingReply;
var chrome = {
  storage: {session: {get: async () => storage, set: async value => Object.assign(storage, value)}},
  runtime: {
    id: "fixture",
    onInstalled: event("installed"), onStartup: event("startup"),
    connectNative: () => { nativeConnections++; return {onMessage: event("message"), onDisconnect: event("disconnect"), postMessage: value => frames.push(value), disconnect: () => callbacks.disconnect()}; }
  },
  action: {onClicked: event("action")},
  tabs: {
    create: async value => { created++; const tab = {id: 10 + created, windowId: 1, ...value}; records.set(tab.id, tab); return tab; },
    get: async id => { if (!records.has(id)) throw Error("Missing tab"); return records.get(id); },
    remove: async id => records.delete(id), update: async () => {}, onRemoved: event("removed")
  },
  windows: {update: async () => {}},
  debugger: {
    attach: async () => {},
    sendCommand: async (_, method, params) => { commands++; if (params.expression === "pending") return new Promise(resolve => { pendingReply = resolve; }); return {method, params}; },
    onEvent: event("event"), onDetach: event("detach")
  }
};

async function runChecks() {
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const rejects = async action => { try { await action(); } catch { return; } throw Error("Unsafe command accepted"); };
  await ready;
  await rejects(() => dispatch({method: "Target.attachToTarget", params: {targetId: "1"}}));
  assert((await dispatch({method: "Target.getTargets"})).targetInfos.length === 0, "Unrelated tab leaked");
  const {targetId} = await dispatch({method: "Target.createTarget", params: {url: "about:blank", background: true}});
  await dispatch({method: "Target.attachToTarget", params: {targetId}});
  const cdp = (method, params) => dispatch({method, params, sessionId: targetId});
  await cdp("Runtime.evaluate", {expression: "1 + 1"});
  const before = commands;
  for (const url of ["https://example.com/", "https://chatgpt.com.evil/", "https://chatgpt.com@evil/", "https://user:secret@chatgpt.com/", "file:///etc/passwd"]) {
    await rejects(() => cdp("Page.navigate", {url}));
  }
  await rejects(() => cdp("Browser.close", {}));
  assert(commands === before, "Disallowed command reached debugger");
  records.get(Number(targetId)).url = "https://example.com/";
  await rejects(() => cdp("Runtime.evaluate", {expression: "document.cookie"}));
  records.get(Number(targetId)).url = "https://chatgpt.com/";
  frames.length = 0;
  const text = "研究🦀\n".repeat(40_000);
  send({result: {text}});
  const packet = frames.map(frame => frame.text).join("");
  assert(frames.length > 1 && JSON.parse(packet).result.text === text, "Full Unicode answer changed");
  frames.length = 0;
  const request = JSON.stringify({id: 99, method: "Runtime.evaluate", sessionId: targetId, params: {expression: text}});
  receive({kind: "fragment", index: 0, last: false, text: request.slice(0, 1000)});
  receive({kind: "fragment", index: 1, last: true, text: request.slice(1000)});
  for (let tick = 0; tick < 50 && !frames.length; tick++) await Promise.resolve();
  const result = frames[0]?.kind === "fragment" ? JSON.parse(frames.map(frame => frame.text).join("")) : frames[0];
  assert(result?.id === 99 && result?.result?.params.expression === text, "Large incoming command changed");
  frames.length = 0;
  callbacks.event({tabId: 1}, "Network.responseReceived", {});
  assert(frames.length === 0, "Unrelated tab events leaked");
  receive({id: 123, method: "Runtime.evaluate", sessionId: targetId, params: {expression: "pending"}});
  for (let tick = 0; tick < 50 && !pendingReply; tick++) await Promise.resolve();
  assert(pendingReply, "Fixture did not start pending command");
  callbacks.disconnect(); connect();
  pendingReply({late: true});
  for (let tick = 0; tick < 50; tick++) await Promise.resolve();
  assert(frames.length === 0, "Old host response leaked into reconnected host");
  callbacks.detach({tabId: Number(targetId)}, "canceled_by_user");
  await rejects(() => cdp("Input.insertText", {text: "must not send"}));
  assert(created === 1 && nativeConnections === 2, "Reconnect created a tab or failed to reconnect");
  assert(storage.researchTabs.length === 1, "Owned scope was not preserved across worker reconnect");
}
