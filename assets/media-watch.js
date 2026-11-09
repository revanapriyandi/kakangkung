(() => {
  const md = navigator.mediaDevices, ipc = window.ipc;
  if (!ipc) return;
  let frame;
  try { frame = crypto.randomUUID(); } catch (_) {
    frame = "f" + Date.now().toString(36) + Math.random().toString(36).slice(2) + Math.random().toString(36).slice(2);
  }
  frame = frame.replace(/[^a-zA-Z0-9_-]/g, "").slice(0, 64);
  const post = ipc.postMessage.bind(ipc), tracks = new Set();
  const playing = new Set(), pip = new Set();
  const watched = new WeakSet();
  let live = false, timer = 0, hidden = false;
  const report = (force = false) => {
    for (const t of tracks) if (t.readyState !== "live") tracks.delete(t);
    const now = !hidden && (tracks.size > 0 || playing.size > 0 || pip.size > 0);
    if (now !== live || force === true) { live = now; post("kk:media:" + frame + ":" + (now ? 1 : 0)); }
    if (!tracks.size && timer) { clearInterval(timer); timer = 0; }
  };
  window.addEventListener("pagehide", () => { hidden = true; report(true); });
  window.addEventListener("pageshow", () => { hidden = false; report(true); });
  const onMedia = ({ type, target }) => {
    if (!(target instanceof HTMLMediaElement)) return;
    watch(target);
    if (type === "enterpictureinpicture") pip.add(target);
    else if (type === "leavepictureinpicture") pip.delete(target);
    else if ((type === "play" || type === "playing") && !target.paused && !target.ended) playing.add(target);
    else if (type === "pause" || type === "ended" || type === "emptied") playing.delete(target);
    report();
  };
  const watch = (el) => {
    if (watched.has(el)) return;
    watched.add(el);
    // Detached media no longer reaches the document's capture listeners.
    for (const name of ["pause", "ended", "emptied", "leavepictureinpicture"]) {
      el.addEventListener(name, onMedia);
    }
  };
  for (const name of ["play", "playing", "pause", "ended", "emptied", "enterpictureinpicture", "leavepictureinpicture"]) {
    document.addEventListener(name, onMedia, true);
  }
  const scan = () => {
    for (const el of document.querySelectorAll("audio, video")) {
      watch(el);
      if (!el.paused && !el.ended) playing.add(el);
    }
    if (document.pictureInPictureElement) pip.add(document.pictureInPictureElement);
    report();
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", scan, { once: true });
  else scan();
  if (!md) return;
  for (const name of ["getUserMedia", "getDisplayMedia"]) {
    const orig = md[name] && md[name].bind(md);
    if (!orig) continue;
    md[name] = async (...args) => {
      const stream = await orig(...args);
      for (const t of stream.getTracks()) { tracks.add(t); t.addEventListener("ended", report); }
      report();
      if (tracks.size && !timer) timer = setInterval(report, 3000);
      return stream;
    };
  }
})();
