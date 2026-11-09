const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, 'webapps.js'), 'utf8');
const valid = () => ({ name: 'Fixture App', start_url: '/app/start', scope: '/app/', id: '/app/', display: 'standalone', icons: [{ src: '/icon.png', sizes: '192x192' }] });

async function probe(manifest = valid(), options = {}) {
  const messages = [], timers = new Map(), events = new Map();
  let fetches = 0, disconnected = 0, observed;
  const context = { URL, AbortController, TextDecoder, Uint8Array,
    location: new URL('https://example.test/app/page'), isSecureContext: options.secure !== false,
    ipc: { postMessage: message => messages.push(message) },
    document: { readyState: 'complete', head: {}, querySelector: () => options.noManifest ? null : ({ href: 'https://example.test/manifest.json' }) },
    addEventListener: (name, handler) => events.set(name, handler),
    setTimeout: handler => { const id = timers.size + 1; timers.set(id, handler); return id; }, clearTimeout: id => timers.delete(id),
    MutationObserver: class { constructor(callback) { this.callback = callback; } observe(element, config) { observed = { element, config }; } disconnect() { disconnected++; } },
    fetch: async () => { fetches++; return new Response(options.oversize ? ' '.repeat(262145) : JSON.stringify(manifest)); },
  };
  context.window = context; context.top = options.frame ? {} : context;
  await vm.runInNewContext(source, context);
  return { messages, timers, events, fetches, get disconnected() { return disconnected; }, observed };
}

test('valid manifest offers an app with same-origin launch and stops watching the head', async () => {
  const p = await probe(); assert.equal(p.messages.length, 1);
  const app = JSON.parse(p.messages[0].slice('kk:webapp:'.length));
  assert.equal(app.start_url, 'https://example.test/app/start');
  assert.equal(app.scope, 'https://example.test/app/');
  assert.equal(app.icon_url, 'https://example.test/icon.png');
  assert.equal(p.timers.size, 0); assert.ok(p.disconnected > 0);
  assert.deepEqual(Array.from(p.observed.config.attributeFilter), ['rel', 'href']);
});
test('ordinary pages, insecure contexts and frames never offer installation', async () => {
  for (const options of [{ noManifest: true }, { secure: false }, { frame: true }]) {
    const p = await probe(valid(), options); assert.equal(p.messages.length, 0); assert.equal(p.fetches, 0);
  }
});
test('manifest eligibility rejects missing icons, browser display and out-of-origin launch', async () => {
  for (const change of [{ icons: [] }, { display: 'browser' }, { start_url: 'https://evil.test/app/' }, { scope: '/other/' }, { name: '\nSpoof' }, { icons: [{ src: '/icon.png', sizes: '192x512' }] }]) {
    assert.equal((await probe({ ...valid(), ...change })).messages.length, 0, JSON.stringify(change));
  }
});
test('large manifest bodies are bounded and pagehide releases the observer', async () => {
  const p = await probe(valid(), { oversize: true }); assert.equal(p.messages.length, 0);
  p.events.get('pagehide')(); assert.equal(p.timers.size, 0); assert.ok(p.disconnected > 0);
});
