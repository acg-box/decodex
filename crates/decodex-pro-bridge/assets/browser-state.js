// ChatGPT shares its persisted editor state across new-chat and Project pages.
// Give each owned research tab its own record before the application starts.
// Cookies, account access and other records keep their normal behavior.
// The user's original editor state is never overwritten.
(scope => {
  // A same-origin about:blank teardown document retains the window origin.
  if (globalThis.origin !== "https://chatgpt.com") return;
  const marker = Symbol.for("decodex.research.browser-state");
  if (globalThis[marker]) return;
  const originalKey = "codex.browser.persistedAtomState";
  const sessionKey = "decodex.research.state-scope";
  let stateId = sessionStorage.getItem(sessionKey);
  if (!stateId) {
    stateId = crypto.randomUUID();
    sessionStorage.setItem(sessionKey, stateId);
  }
  const scopedKey = `${originalKey}.decodex-research.${stateId}`;
  const applies = store => store.name === "records" && store.transaction.db.name === "codex-browser-host";
  const keyFor = (store, key) => applies(store) && key === originalKey ? scopedKey : key;
  const prototype = IDBObjectStore.prototype;
  for (const method of ["get", "delete"]) {
    const original = prototype[method];
    prototype[method] = function(key) {
      return Reflect.apply(original, this, [keyFor(this, key)]);
    };
  }
  for (const method of ["put", "add"]) {
    const original = prototype[method];
    prototype[method] = function(value, key) {
      return Reflect.apply(original, this, arguments.length > 1 ? [value, keyFor(this, key)] : [value]);
    };
  }
  const release = () => new Promise((resolve, reject) => {
    const request = indexedDB.open("codex-browser-host", 1);
    request.onerror = () => reject(request.error);
    request.onsuccess = () => {
      const database = request.result;
      const transaction = database.transaction("records", "readwrite");
      transaction.oncomplete = () => { database.close(); resolve(true); };
      transaction.onabort = () => { database.close(); reject(transaction.error); };
      transaction.objectStore("records").delete(scopedKey);
    };
  });
  Object.defineProperty(globalThis, marker, {value: {scope, release}});
})
