// Chrome runs this JavaScript directly. No Node runtime or transpiler is used.
// The native host requests high-level research operations; only owned tabs accept CDP.
const HOST = "box.acg.decodex.research";
const LIMIT = 32 * 1024 * 1024;
const METHODS = new Set([
  "Page.enable", "Page.navigate", "Page.reload", "Page.bringToFront",
  "Runtime.enable", "Runtime.evaluate", "Network.enable", "Network.getResponseBody",
  "Emulation.setFocusEmulationEnabled", "Input.insertText", "Input.dispatchKeyEvent",
  "Input.dispatchMouseEvent"
]);
let port;
let fragments = "";
let nextFragment = 0;
let reconnect;
const attached = new Set();
const owned = new Set();
// Session storage survives worker restarts but not browser restarts and tab-ID reuse.
const ready = chrome.storage.session.get("researchTabs").then(saved => {
  for (const id of saved.researchTabs || []) owned.add(id);
});

function chatUrl(value) {
  try { const u = new URL(value); return u.origin === "https://chatgpt.com" && !u.username && !u.password; }
  catch { return false; }
}

async function saveTabs() {
  await chrome.storage.session.set({researchTabs: [...owned]});
}

async function ownedTab(value) {
  await ready;
  const id = Number(value);
  if (!Number.isSafeInteger(id) || !owned.has(id)) throw Error("Not a Decodex research tab");
  const tab = await chrome.tabs.get(id);
  const url = tab.url || tab.pendingUrl;
  if (url !== "about:blank" && !chatUrl(url)) throw Error("Research tab left chatgpt.com; open it to sign in or reconnect");
  return tab;
}

async function dispatch(request) {
  await ready;
  const p = request.params || {};
  switch (request.method) {
    case "Decodex.ping": return {extensionId: chrome.runtime.id, protocol: 1};
    case "Target.getTargets": {
      const targets = [];
      for (const id of [...owned]) {
        try { const t = await ownedTab(id); targets.push({targetId: String(id), type: "page", url: t.url || "about:blank"}); }
        catch { /* Never expose unrelated tabs or login pages. */ }
      }
      return {targetInfos: targets};
    }
    case "Target.createTarget": {
      if (p.url !== "about:blank") throw Error("Research tabs must start empty");
      const tab = await chrome.tabs.create({url: "https://chatgpt.com/", active: !p.background});
      owned.add(tab.id);
      await saveTabs();
      return {targetId: String(tab.id)};
    }
    case "Target.attachToTarget": {
      const tab = await ownedTab(p.targetId);
      if (!attached.has(tab.id)) {
        await chrome.debugger.attach({tabId: tab.id}, "1.3");
        attached.add(tab.id);
      }
      return {sessionId: String(tab.id)};
    }
    case "Target.activateTarget": {
      const tab = await ownedTab(p.targetId);
      await chrome.tabs.update(tab.id, {active: true});
      await chrome.windows.update(tab.windowId, {focused: true});
      return {};
    }
    case "Target.closeTarget": {
      const tab = await ownedTab(p.targetId);
      await chrome.tabs.remove(tab.id);
      owned.delete(tab.id);
      attached.delete(tab.id);
      await saveTabs();
      return {success: true};
    }
    default: {
      if (!METHODS.has(request.method)) throw Error("Unsupported research command");
      const tab = await ownedTab(request.sessionId);
      if (!attached.has(tab.id)) throw Error("Research debugger disconnected; recover the saved request");
      if (request.method === "Page.navigate" && !chatUrl(p.url)) throw Error("Research navigation is limited to chatgpt.com");
      return chrome.debugger.sendCommand({tabId: tab.id}, request.method, p);
    }
  }
}

function send(value, recipient = port) {
  // A late command from a disconnected host must not answer a new host's reused ID.
  if (!recipient || recipient !== port) return;
  const packet = JSON.stringify(value);
  if (packet.length > LIMIT) throw Error("Research packet exceeds 32 MiB");
  if (packet.length <= 65536) { recipient.postMessage(value); return; }
  for (let start = 0, index = 0; start < packet.length; index++) {
    let end = Math.min(start + 65536, packet.length);
    // Do not split a UTF-16 surrogate pair between native JSON messages.
    const tail = packet.charCodeAt(end - 1);
    if (end < packet.length && tail >= 0xd800 && tail <= 0xdbff) end--;
    recipient.postMessage({kind: "fragment", index, last: end === packet.length, text: packet.slice(start, end)});
    start = end;
  }
}

function receive(value, source = port) {
  if (source !== port) return;
  if (value.kind === "fragment") {
    if (value.index !== nextFragment || typeof value.text !== "string" || fragments.length + value.text.length > LIMIT) {
      fragments = ""; nextFragment = 0;
      port.disconnect();
      return;
    }
    fragments += value.text; nextFragment++;
    if (!value.last) return;
    value = JSON.parse(fragments); fragments = ""; nextFragment = 0;
  } else if (nextFragment) { port.disconnect(); return; }
  Promise.resolve().then(() => dispatch(value)).then(
    result => send({id: value.id, result}, source),
    error => send({id: value.id, error: {message: String(error.message || error)}}, source)
  );
}

function connect() {
  if (port) return;
  clearTimeout(reconnect);
  fragments = ""; nextFragment = 0;
  const active = chrome.runtime.connectNative(HOST);
  port = active;
  active.onMessage.addListener(value => receive(value, active));
  active.onDisconnect.addListener(() => {
    void chrome.runtime.lastError;
    if (port !== active) return;
    port = undefined;
    // Keep owned tabs and their work. Installing or restarting the host does not resend.
    reconnect = setTimeout(connect, 5000);
  });
}

chrome.debugger.onEvent.addListener((source, method, params) => {
  if (owned.has(source.tabId)) send({sessionId: String(source.tabId), method, params});
});
chrome.debugger.onDetach.addListener((source, reason) => {
  attached.delete(source.tabId);
  if (owned.has(source.tabId)) send({sessionId: String(source.tabId), method: "Decodex.detached", params: {reason}});
});
chrome.tabs.onRemoved.addListener(id => {
  if (owned.delete(id)) { attached.delete(id); void saveTabs(); }
});
chrome.runtime.onInstalled.addListener(connect);
chrome.runtime.onStartup.addListener(connect);
chrome.action.onClicked.addListener(connect);
connect();
