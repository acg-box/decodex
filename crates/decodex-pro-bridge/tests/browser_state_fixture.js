// Execute the real page adapter against a native-like storage boundary.
var location = {origin: "https://chatgpt.com"};
var origin = "https://chatgpt.com";
var crypto = {randomUUID: () => "research-scope-a"};
var sessionStorage = {getItem: key => sessionStorage[key] ?? null, setItem: (key, value) => { sessionStorage[key] = value; }};
class IDBObjectStore {
  constructor(database = "codex-browser-host", name = "records") {
    this.transaction = {db: {name: database}};
    this.name = name;
    this.values = new Map();
  }
  get(key) { return this.values.get(key); }
  delete(key) { this.values.delete(key); }
  put(value, key) { this.values.set(arguments.length > 1 ? key : value.id, value); }
  add(value, key) { if (this.values.has(key)) throw Error("Duplicate key"); this.put(...arguments); }
}
const setupBrowserState = SCRIPT;
const originalKey = "codex.browser.persistedAtomState";
const editor = new IDBObjectStore();
editor.put({draft: "An unrelated user draft"}, originalKey);
editor.put({value: "Account access stays shared"}, "account-record");
const originalDraft = editor.get(originalKey);
setupBrowserState("owned-a");
function check(value, message) { if (!value) throw Error(message); }
check(editor.get(originalKey) === undefined, "A new research tab inherited the user's draft");
editor.put({draft: "Owned question"}, originalKey);
check(editor.get(originalKey).draft === "Owned question", "Research did not retain its own draft");
check(editor.values.get(originalKey) === originalDraft, "Research changed the user's original state");
check(editor.get("account-record").value === "Account access stays shared", "A separate account record was isolated");
setupBrowserState("owned-a");
check(editor.get(originalKey).draft === "Owned question", "Repeated setup discarded the research draft");
check(globalThis[Symbol.for("decodex.research.browser-state")].scope === "owned-a", "Owned-tab identity was not retained");
editor.delete(originalKey);
check(editor.get(originalKey) === undefined && editor.values.get(originalKey) === originalDraft, "Removing a research draft removed the user's draft");
editor.add({draft: "Next owned draft"}, originalKey);
check(editor.get(originalKey).draft === "Next owned draft", "Native add did not use the isolated record");
editor.put({id: "implicit-key", value: 3});
check(editor.get("implicit-key").value === 3, "An inline key changed");
for (const store of [new IDBObjectStore("another-database"), new IDBObjectStore("codex-browser-host", "another-store")]) {
  store.put(originalDraft, originalKey);
  check(store.values.get(originalKey) === originalDraft, "An unrelated store was redirected");
}
var pageDone = false, pageError = "";
var indexedDB = {open: () => {
  const request = {result: {transaction: () => {
    const transaction = {objectStore: () => ({delete: key => {
      editor.delete(key);
      Promise.resolve().then(() => transaction.oncomplete());
    }})};
    return transaction;
  }, close: () => {}}};
  Promise.resolve().then(() => request.onsuccess());
  return request;
}};
globalThis[Symbol.for("decodex.research.browser-state")].release().then(() => {
  check(editor.get(originalKey) === undefined, "A completed research tab retained its scoped state");
  check(editor.values.get(originalKey) === originalDraft, "Cleanup removed the original draft");
}).then(() => { pageDone = true; }, error => { pageError = String(error); pageDone = true; });
