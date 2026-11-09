const { test } = require("node:test");
const assert = require("node:assert/strict");
const vm = require("node:vm");
const fs = require("node:fs");
const path = require("node:path");

function page() {
  const listeners = new Map(), messages = [], fields = [];
  const on = (name, fn) => listeners.set(name, fn);
  const document = {
    addEventListener: on,
    querySelectorAll: (selector) => fields.filter((f) => selector === "input" || f.type === "password"),
    querySelector: () => fields.find((f) => f.autocomplete === "username"),
  };
  class Input {
    constructor(type, autocomplete = "", value = "") {
      Object.assign(this, { type, autocomplete, name: "", id: "", placeholder: "", isConnected: true, visible: true, form: null, _value: value });
      fields.push(this);
    }
    get value() { return this._value; }
    set value(v) { this._value = v; }
    getClientRects() { return this.visible ? [{}] : []; }
    getBoundingClientRect() { return { left: 0, bottom: 20, width: 200 }; }
    getAttribute() { return ""; }
    focus() { listeners.get("focusin")?.({ target: this }); }
    dispatchEvent() {}
  }
  const window = { ipc: { postMessage: (m) => messages.push(m) } };
  window.top = window;
  const context = { window, document, HTMLInputElement: Input, sessionStorage: { getItem: () => null, setItem() {} },
    devicePixelRatio: 1, Event: class {}, addEventListener: on, setInterval: () => 1, clearInterval() {} };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "passwords.js"), "utf8"), context);
  const form = (...inputs) => {
    const f = { querySelectorAll: (selector) => inputs.filter((i) => selector === "input" || i.type === "password") };
    inputs.forEach((i) => { i.form = f; });
    return f;
  };
  return { Input, form, window, messages, emit: (name, event = {}) => listeners.get(name)?.(event) };
}

test("explicit fill clears an old username for a login without one", () => {
  const p = page(), u = new p.Input("email", "username", "other@example.com"), pw = new p.Input("password", "current-password");
  p.form(u, pw);
  pw.focus();
  p.window.__kkPwFill("", "saved");
  assert.equal(u.value, "");
  assert.equal(pw.value, "saved");
});

test("new-password and detached fields never receive a saved password", () => {
  const p = page(), pw = new p.Input("password", "new-password");
  pw.focus();
  p.window.__kkPwFill("ana", "saved");
  assert.equal(pw.value, "");
  pw.autocomplete = "current-password";
  pw.focus();
  pw.isConnected = false;
  p.window.__kkPwFill("ana", "saved");
  assert.equal(pw.value, "");
});

test("unrelated toolbar click does not offer the login form password", () => {
  const p = page(), pw = new p.Input("password", "current-password", "secret");
  p.form(pw);
  p.emit("click", { target: { closest: () => ({ form: null }) } });
  p.emit("pagehide");
  assert.equal(p.messages.some((m) => m.startsWith("kk:pw:save:")), false);
});

test("submitted password change captures the new password", () => {
  const p = page(), u = new p.Input("email", "username", "ana@example.com");
  const old = new p.Input("password", "current-password", "old"), fresh = new p.Input("password", "new-password", "new");
  const f = p.form(u, old, fresh);
  p.emit("submit", { target: f });
  p.emit("pagehide");
  const saved = p.messages.find((m) => m.startsWith("kk:pw:save:"));
  assert.deepEqual(JSON.parse(saved.slice("kk:pw:save:".length)), { u: "ana@example.com", p: "new" });
});
