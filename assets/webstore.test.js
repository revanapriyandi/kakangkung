const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");

test("store click rejects a stale SPA route and disables duplicate clicks", () => {
  const id = "ddkjiahejlhfcafbddmgiahcphecmpfh";
  const other = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
  const messages = [];
  const handlers = new Map();
  const button = {
    textContent: "Add to Chrome", dataset: {}, style: {}, disabled: false,
    addEventListener: (name, handler) => handlers.set(name, handler),
  };
  let tick;
  const location = { hostname: "chromewebstore.google.com", pathname: `/detail/example/${id}` };
  const window = { ipc: { postMessage: (message) => messages.push(message) } };
  window.top = window;
  const document = {
    querySelectorAll: (selector) => selector === "button" ? [button] : [],
    querySelector: () => ({ textContent: "Example extension" }),
  };
  vm.runInNewContext(fs.readFileSync(`${__dirname}/webstore.js`, "utf8"), {
    window, document, location, setInterval: (callback) => { tick = callback; },
  });
  tick();
  assert.deepEqual(messages, [`kk:store:check\t${id}`]);
  window.__kkStore(id, "ready", "Add to Kakangkung");
  const click = () => handlers.get("click")({ preventDefault() {}, stopImmediatePropagation() {} });
  location.pathname = `/detail/other/${other}`;
  click();
  assert.equal(messages.length, 1, "navigation before the next poll cannot install the previous extension");
  location.pathname = `/detail/example/${id}`;
  click();
  click();
  assert.equal(messages.filter((message) => message.startsWith("kk:store:add\t")).length, 1);
  assert.equal(button.disabled, true, "button is disabled before the IPC reply");
  window.__kkStore(id, "ready", "Try again");
  assert.equal(button.disabled, false, "a failed request can be retried");
});
