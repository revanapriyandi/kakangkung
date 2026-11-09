const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const source = fs.readFileSync(path.join(__dirname, "adblock.js"), "utf8");
const player = () => ({
  videoDetails: { videoId: "content-video", title: "Actual content" },
  streamingData: { adaptiveFormats: [{ url: "https://video.example/content", mimeType: "video/mp4" }] },
  playabilityStatus: { status: "OK" }, captions: { captionTracks: [{ languageCode: "id" }] },
  playerAds: [{ ad: true }], adPlacements: [{ placement: true }], adSlots: [{ slot: true }],
});
function page(host = "www.youtube.com", config = { enabled: true, allowedSites: [] }) {
  const listeners = new Map(), requests = [];
  class StyleSheet { replaceSync(css) { this.css = css; } }
  const location = new URL(`https://${host}/watch?v=content-video`);
  const context = {
    location, URL, CSSStyleSheet: StyleSheet,
    document: { adoptedStyleSheets: [], addEventListener: (name, fn) => listeners.set(name, fn),
      removeEventListener: name => listeners.delete(name) },
    fetch(...args) { requests.push(args); return Promise.resolve(new Response(JSON.stringify(player()), {
      status: 200, headers: { "content-type": "application/json", "x-fixture": "kept" },
    })); },
    setInterval() { throw new Error("No polling is allowed"); },
    setTimeout() { throw new Error("No polling is allowed"); },
    MutationObserver: class { constructor() { throw new Error("No whole-page observers are allowed"); } },
  };
  context.window = context;
  context.top = context;
  vm.createContext(context);
  const originalParse = vm.runInContext("JSON.parse", context), originalFetch = context.fetch;
  const apply = vm.runInContext(`(${source})`, context);
  apply(config);
  const parse = input => vm.runInContext(`JSON.parse(${JSON.stringify(input)})`, context);
  return { context, parse, apply, requests, originalParse, originalFetch,
    currentParse: () => vm.runInContext("JSON.parse", context) };
}

test("initial YouTube player assignment removes only ad instructions", () => {
  const p = page(), value = player(), content = JSON.stringify(value.streamingData), captions = value.captions;
  p.context.ytInitialPlayerResponse = value;
  assert.equal(value.playerAds, undefined);
  assert.equal(value.adPlacements, undefined);
  assert.equal(value.adSlots, undefined);
  assert.equal(JSON.stringify(value.streamingData), content);
  assert.equal(value.captions, captions);
  assert.equal(value.playabilityStatus.status, "OK");
});

test("JSON parsing handles playerResponse and preserves unrelated payloads and revivers", () => {
  const p = page();
  const result = p.parse(JSON.stringify({ playerResponse: player(), account: { adSlots: ["preference"] } }));
  assert.equal(result.playerResponse.playerAds, undefined);
  assert.deepEqual(Array.from(result.account.adSlots), ["preference"]);
  const unrelated = { adSlots: ["calendar slots"], playerAds: "an ordinary property", items: [{ sponsored: true }] };
  assert.equal(JSON.stringify(p.parse(JSON.stringify(unrelated))), JSON.stringify(unrelated));
  assert.throws(() => p.parse("{invalid-json"), /JSON|property|Unexpected/);
  assert.equal(vm.runInContext(`JSON.parse('{"n":2}', (key, value) => key === "n" ? value * 3 : value).n`, p.context), 6);
});

test("only same-origin player endpoints have their native fetch JSON filtered", async () => {
  const p = page();
  const result = await p.context.fetch("/youtubei/v1/player?prettyPrint=false", { method: "POST" });
  assert.equal(result.status, 200);
  assert.equal(result.headers.get("x-fixture"), "kept");
  assert.equal((await result.clone().json()).adPlacements, undefined);
  assert.equal((await result.json()).videoDetails.videoId, "content-video");
  for (const url of ["https://api.other.example/youtubei/v1/player", "/account", "/youtubei/v1/player/lookalike"]) {
    const untouched = await p.context.fetch(url);
    assert.ok((await untouched.json()).playerAds);
  }
  assert.equal(p.requests[0][1].method, "POST");
});

test("off and exact-host pause preserve page objects without installing hooks or styles", () => {
  for (const config of [{ enabled: false }, { enabled: true, allowedSites: ["WWW.YouTube.COM"] }]) {
    const p = page("www.youtube.com", config), value = player();
    p.context.ytInitialPlayerResponse = value;
    assert.equal(p.currentParse(), p.originalParse);
    assert.equal(p.context.fetch, p.originalFetch);
    assert.ok(value.playerAds);
    assert.equal(p.context.document.adoptedStyleSheets.length, 0);
  }
  const p = page("www.youtube.com", { enabled: true, allowedSites: ["youtube.com"] });
  assert.notEqual(p.currentParse(), p.originalParse, "parent exemption does not exempt subdomain");
});

test("runtime off restores hooks and styles, preserves later player assignments and reenables once", () => {
  const p = page();
  const stylesheet = p.context.document.adoptedStyleSheets[0];
  p.apply({ enabled: true });
  assert.equal(p.context.document.adoptedStyleSheets.length, 1);
  assert.equal(p.context.document.adoptedStyleSheets[0], stylesheet);
  p.apply({ enabled: false });
  assert.equal(p.currentParse(), p.originalParse);
  assert.equal(p.context.fetch, p.originalFetch);
  assert.equal(p.context.document.adoptedStyleSheets.length, 0);
  p.context.ytInitialPlayerResponse = player();
  assert.ok(p.context.ytInitialPlayerResponse.playerAds);
  p.apply({ enabled: true });
  assert.equal(p.context.document.adoptedStyleSheets.length, 1);
  assert.equal(p.context.ytInitialPlayerResponse.playerAds, undefined);
});

test("other sites get only clear ad cosmetics and no YouTube JSON or fetch hooks", () => {
  for (const host of ["news.example.com", "www.youtube.com.evil.example", "notyoutube.com"]) {
    const p = page(host);
    assert.equal(p.currentParse(), p.originalParse);
    assert.equal(p.context.fetch, p.originalFetch);
    const css = p.context.document.adoptedStyleSheets[0].css;
    assert.ok(css.includes("ins.adsbygoogle"));
    assert.equal(css.includes("ytd-"), false);
    assert.equal(css.includes("video"), false);
  }
});

test("frozen player objects and nonconfigurable page properties remain usable", () => {
  const p = page("www.youtube.com", { enabled: false });
  const value = Object.freeze(player());
  Object.defineProperty(p.context, "ytInitialPlayerResponse", { configurable: false, value });
  assert.doesNotThrow(() => p.apply({ enabled: true }));
  assert.equal(p.context.ytInitialPlayerResponse, value);
  assert.ok(value.streamingData);
});

test("an in-flight response stops filtering when native preference turns off", async () => {
  const p = page(), pending = p.context.fetch("/youtubei/v1/player");
  p.apply({ enabled: false });
  assert.ok((await (await pending).json()).playerAds);
});

test("a runtime site pause disables the same document and removes cosmetics", () => {
  const p = page();
  p.apply({ enabled: true, allowedSites: ["www.youtube.com"] });
  assert.equal(p.currentParse(), p.originalParse);
  assert.equal(p.context.fetch, p.originalFetch);
  assert.equal(p.context.document.adoptedStyleSheets.length, 0);
  p.context.ytInitialPlayerResponse = player();
  assert.ok(p.context.ytInitialPlayerResponse.adSlots);
});

test("document-start helper leaves subframes alone so top-level pause stays consistent", () => {
  const p = page("www.youtube.com", { enabled: false });
  p.context.top = {};
  p.apply({ enabled: true });
  assert.equal(p.currentParse(), p.originalParse);
  assert.equal(p.context.document.adoptedStyleSheets.length, 0);
});

test("diagnostics count only actual deleted ad fields and expose no page data", () => {
  const p = page();
  assert.equal(JSON.stringify(p.context.__kkAdblock.status()), '{"enabled":true,"youtube":true,"prunedPlayerFields":0}');
  p.context.ytInitialPlayerResponse = player();
  p.context.ytInitialPlayerResponse = p.context.ytInitialPlayerResponse;
  p.context.ytInitialPlayerResponse = Object.freeze(player());
  const status = p.context.__kkAdblock.status();
  assert.equal(status.prunedPlayerFields, 3, "absent and undeletable properties do not increment");
  status.prunedPlayerFields = 999;
  assert.equal(p.context.__kkAdblock.status().prunedPlayerFields, 3, "returned diagnostics are a snapshot");
  p.apply({ enabled: false });
  assert.equal(p.context.__kkAdblock.status().enabled, false);
});
