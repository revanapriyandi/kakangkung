const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

function fixture({ capture = true } = {}) {
  const messages = [], rawMessages = [], listeners = new Map(), windowListeners = new Map(), intervals = new Map();
  let nextTimer = 1;
  class Media {
    constructor() { this.paused = true; this.ended = false; this.muted = true; this.listeners = new Map(); }
    addEventListener(type, fn) {
      if (!this.listeners.has(type)) this.listeners.set(type, []);
      this.listeners.get(type).push(fn);
    }
    emitDetached(type) {
      for (const fn of this.listeners.get(type) || []) fn({ type, target: this });
    }
  }
  const document = {
    readyState: 'loading', pictureInPictureElement: null,
    addEventListener(type, fn, options) {
      if (!listeners.has(type)) listeners.set(type, []);
      listeners.get(type).push({ fn, capture: options === true });
    },
    querySelectorAll() { return []; },
  };
  const track = { readyState: 'live', addEventListener() {} };
  const navigator = capture ? { mediaDevices: { getUserMedia: async () => ({ getTracks: () => [track] }) } } : {};
  vm.runInNewContext(fs.readFileSync(__dirname + '/media-watch.js', 'utf8'), {
    navigator, window: {
      ipc: { postMessage: message => {
        rawMessages.push(message);
        assert.match(message, /^kk:media:[a-zA-Z0-9_-]{1,64}:[01]$/);
        messages.push('kk:media:' + message.split(':').at(-1));
      } },
      addEventListener: (type, fn) => windowListeners.set(type, fn),
    },
    crypto: require('node:crypto').webcrypto,
    document, HTMLMediaElement: Media,
    setInterval: fn => { const id = nextTimer++; intervals.set(id, fn); return id; },
    clearInterval: id => intervals.delete(id),
  });
  return {
    messages, rawMessages, intervals, navigator, track, document,
    lifecycle(type) { windowListeners.get(type)?.(); },
    media: () => new Media(),
    emit(type, target) {
      // Media lifecycle events do not bubble: only document capture listeners run.
      for (const listener of listeners.get(type) || []) {
        if (listener.capture || target === document) listener.fn({ type, target });
      }
    },
    tick() { for (const fn of [...intervals.values()]) fn(); },
  };
}

test('muted playback protects a page and pause releases it without polling', () => {
  const f = fixture(), video = f.media();
  video.paused = false; f.emit('play', video); f.emit('playing', video);
  assert.deepEqual(f.messages, ['kk:media:1']);
  assert.equal(f.intervals.size, 0);
  video.paused = true; f.emit('pause', video);
  assert.deepEqual(f.messages, ['kk:media:1', 'kk:media:0']);
});

test('sibling documents have distinct stable frame signals', () => {
  const a = fixture(), b = fixture(), av = a.media(), bv = b.media();
  av.paused = bv.paused = false;
  a.emit('playing', av); b.emit('playing', bv);
  const aid = a.rawMessages[0].split(':')[2], bid = b.rawMessages[0].split(':')[2];
  assert.notEqual(aid, bid);
  av.paused = true; a.emit('pause', av);
  assert.equal(a.rawMessages[1], `kk:media:${aid}:0`);
  assert.equal(b.rawMessages[0], `kk:media:${bid}:1`);
  assert.equal(b.rawMessages.length, 1);
});

test('pagehide releases its frame and pageshow restores retained playback', () => {
  const f = fixture(), video = f.media();
  video.paused = false; f.emit('playing', video);
  f.lifecycle('pagehide');
  f.lifecycle('pageshow');
  assert.deepEqual(f.messages, ['kk:media:1', 'kk:media:0', 'kk:media:1']);
  assert.equal(new Set(f.rawMessages.map(message => message.split(':')[2])).size, 1);
});

test('one paused element does not clear another playing element', () => {
  const f = fixture(), a = f.media(), b = f.media();
  a.paused = b.paused = false; f.emit('playing', a); f.emit('playing', b);
  a.paused = true; f.emit('pause', a);
  assert.deepEqual(f.messages, ['kk:media:1']);
  b.ended = true; f.emit('ended', b);
  assert.deepEqual(f.messages, ['kk:media:1', 'kk:media:0']);
});

test('capture polling detects silent track.stop and stops while playback continues', async () => {
  const f = fixture(), video = f.media();
  await f.navigator.mediaDevices.getUserMedia();
  video.paused = false; f.emit('playing', video);
  assert.equal(f.intervals.size, 1);
  f.track.readyState = 'ended'; f.tick();
  assert.equal(f.intervals.size, 0);
  assert.deepEqual(f.messages, ['kk:media:1']);
  video.paused = true; f.emit('pause', video);
  assert.deepEqual(f.messages, ['kk:media:1', 'kk:media:0']);
});

test('capture remains protected when playback pauses', async () => {
  const f = fixture(), video = f.media();
  video.paused = false; f.emit('playing', video);
  await f.navigator.mediaDevices.getUserMedia();
  video.paused = true; f.emit('pause', video);
  assert.deepEqual(f.messages, ['kk:media:1']);
  f.track.readyState = 'ended'; f.tick();
  assert.deepEqual(f.messages, ['kk:media:1', 'kk:media:0']);
});

test('playback works without mediaDevices and emptied releases it', () => {
  const f = fixture({ capture: false }), video = f.media();
  video.paused = false; f.emit('playing', video); f.emit('emptied', video);
  assert.deepEqual(f.messages, ['kk:media:1', 'kk:media:0']);
});

test('paused picture-in-picture stays protected until it leaves', () => {
  const f = fixture(), video = f.media();
  f.document.pictureInPictureElement = video; f.emit('enterpictureinpicture', video);
  f.emit('pause', video);
  assert.deepEqual(f.messages, ['kk:media:1']);
  f.document.pictureInPictureElement = null; f.emit('leavepictureinpicture', video);
  assert.deepEqual(f.messages, ['kk:media:1', 'kk:media:0']);
});

test('DOMContentLoaded discovers existing playback once without starting a timer', () => {
  const f = fixture(), video = f.media();
  video.paused = false;
  f.document.querySelectorAll = () => [video];
  f.emit('DOMContentLoaded', f.document);
  assert.deepEqual(f.messages, ['kk:media:1']);
  assert.equal(f.intervals.size, 0);
  video.paused = true; f.emit('pause', video);
  assert.deepEqual(f.messages, ['kk:media:1', 'kk:media:0']);
});

test('detached playing media releases protection through its own stop listener', () => {
  const f = fixture(), video = f.media();
  video.paused = false; f.emit('playing', video);
  video.paused = true; video.emitDetached('pause');
  assert.deepEqual(f.messages, ['kk:media:1', 'kk:media:0']);
  assert.equal(f.intervals.size, 0);
});
