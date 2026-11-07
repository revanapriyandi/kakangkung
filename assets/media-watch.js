(() => {
  const md = navigator.mediaDevices, ipc = window.ipc;
  if (!md || !ipc) return;
  const post = ipc.postMessage.bind(ipc), tracks = new Set();
  let live = false, timer = 0;
  const report = () => {
    for (const t of tracks) if (t.readyState !== "live") tracks.delete(t);
    const now = tracks.size > 0;
    if (now !== live) { live = now; post("kk:media:" + (now ? 1 : 0)); }
    if (!now && timer) { clearInterval(timer); timer = 0; }
  };
  for (const name of ["getUserMedia", "getDisplayMedia"]) {
    const orig = md[name] && md[name].bind(md);
    if (!orig) continue;
    md[name] = async (...args) => {
      const stream = await orig(...args);
      for (const t of stream.getTracks()) { tracks.add(t); t.addEventListener("ended", report); }
      report();
      if (live && !timer) timer = setInterval(report, 3000);
      return stream;
    };
  }
})();
