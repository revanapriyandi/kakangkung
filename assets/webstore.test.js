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
    textContent: "Tambahkan ke Chrome", dataset: {}, style: {}, disabled: false,
    getClientRects: () => [{}], getAttribute: () => null,
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

test("the Switch to Chrome banner hides alone, and its button is not the install button", () => {
  const id = "ddkjiahejlhfcafbddmgiahcphecmpfh";
  let tick;
  const selectors = [];
  // The banner: a link to the Chrome download page inside a short box.
  const banner = { style: {}, textContent: "Beralih ke Chrome? Instal Chrome", parentElement: null, getBoundingClientRect: () => ({ height: 80 }) };
  const notice = { style: {}, textContent: "Beralih ke Chrome untuk menginstal ekstensi dan tema Instal Chrome", parentElement: null, querySelector: () => null, getBoundingClientRect: () => ({ height: 56 }) };
  const link = { style: {}, parentElement: banner, closest: () => banner, getClientRects: () => [{}], getBoundingClientRect: () => ({ height: 20 }) };
  const install = { textContent: "Instal Chrome", dataset: {}, style: {}, getClientRects: () => [{}], parentElement: notice, getBoundingClientRect: () => ({ height: 40 }),
    getAttribute: () => "Beralih ke Chrome untuk menginstal ekstensi dan tema", addEventListener() {} };
  const location = { hostname: "chromewebstore.google.com", pathname: `/detail/example/${id}` };
  const window = { ipc: { postMessage() {} } };
  window.top = window;
  const floating = { style: {}, addEventListener() {}, remove() {} };
  const document = {
    body: {},
    querySelectorAll: (selector) => { selectors.push(selector); return selector === "button" ? [install] : [link]; },
    querySelector: () => null,
    createElement: () => floating,
    documentElement: { appendChild() {} },
  };
  vm.runInNewContext(fs.readFileSync(`${__dirname}/webstore.js`, "utf8"), {
    window, document, location, setInterval: (callback) => { tick = callback; },
  });
  tick();
  assert.equal(banner.style.display, "none");
  assert.equal(install.dataset.kk, undefined, "Install Chrome is not the install button");
  assert.equal(notice.style.display, "none", "the notice around Install Chrome hides");
  const banners = selectors.find((selector) => selector !== "button");
  assert.ok(!"https://support.google.com/chrome_webstore/answer/1".startsWith(banners.match(/"(.+)"/)[1]), "help links in extension cards are not banners");
  // Once hidden, the link has no boxes: nothing around the banner is hidden on later polls.
  link.getClientRects = () => [];
  banner.style.display = "";
  tick();
  assert.equal(banner.style.display, "");
});
