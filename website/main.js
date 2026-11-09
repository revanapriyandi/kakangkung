// Kakangkung site behaviour. No libraries. CSS animates; this file sets states and variables.
// With JS off the page is a plain, complete document. Reduced motion gets the final states.
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

/* Headline: "go to sleep." loses weight after a moment. Hover or focus wakes it (CSS). */
const title = $("#sleepy");
if (title) setTimeout(() => title.classList.add("dozed"), reduced ? 0 : 1400);

/* Scroll-linked parts: stem growth + leaves, the tab story, the screenshot strip. */
const stemGrow = $(".stem-grow");
const nodes = $$(".stem .node");
const sections = nodes.map((n) => document.getElementById(n.dataset.for));
const story = $("[data-story]");
const moments = story ? $$(".moments li", story) : [];
const pages = $("#pages");
const gallery = $(".gallery");
let galleryOverflow = 0;

function layoutGallery() {
  if (!pages || !gallery) return;
  const pin = !reduced && innerWidth >= 900;
  pages.classList.toggle("pinned", pin);
  if (!pin) { pages.style.removeProperty("--pin-h"); gallery.style.removeProperty("--gx"); galleryOverflow = 0; return; }
  galleryOverflow = Math.max(0, gallery.scrollWidth - innerWidth);
  pages.style.setProperty("--pin-h", `${innerHeight + galleryOverflow}px`);
}

function onScroll() {
  const vh = innerHeight, max = document.documentElement.scrollHeight - vh;
  if (stemGrow) stemGrow.style.setProperty("--p", max > 0 ? (scrollY / max).toFixed(4) : 0);

  let here = 0;
  sections.forEach((s, i) => {
    const on = s && s.getBoundingClientRect().top < vh * 0.6;
    nodes[i].classList.toggle("on", !!on || i === 0);
    if (on) here = i;
  });
  nodes.forEach((n, i) => {
    n.classList.toggle("here", i === here);
    if (i === here) n.setAttribute("aria-current", "location"); else n.removeAttribute("aria-current");
  });

  if (story && !reduced) {
    const r = story.getBoundingClientRect();
    const p = clamp(-r.top / (r.height - vh), 0, 1);
    const at = String(Math.min(2, Math.floor(p * 3)));
    story.style.setProperty("--sp", p.toFixed(3));
    if (story.dataset.at !== at) {
      story.dataset.at = at;
      moments.forEach((li) => li.classList.toggle("on", li.dataset.at === at));
    }
  }

  if (galleryOverflow) {
    const r = pages.getBoundingClientRect();
    const p = clamp(-r.top / galleryOverflow, 0, 1);
    gallery.style.setProperty("--gx", `${(-p * galleryOverflow).toFixed(1)}px`);
  }
}
let ticking = false;
addEventListener("scroll", () => { if (!ticking) { ticking = true; requestAnimationFrame(() => { ticking = false; onScroll(); }); } }, { passive: true });
addEventListener("resize", () => { layoutGallery(); onScroll(); });
addEventListener("load", () => { layoutGallery(); onScroll(); });
if (story && reduced) { story.dataset.at = "1"; moments.forEach((li) => li.classList.add("on")); }
layoutGallery();
onScroll();

/* Screenshot strip buttons: move one screenshot. Pinned mode scrolls the page; swipe mode scrolls the strip. */
$$("[data-go]").forEach((b) => b.addEventListener("click", () => {
  const dir = Number(b.dataset.go);
  const items = $$("li", gallery);
  if (galleryOverflow) {
    const gx = -parseFloat(gallery.style.getPropertyValue("--gx") || 0);
    const lefts = items.map((li) => Math.min(galleryOverflow, li.offsetLeft - items[0].offsetLeft));
    const target = dir > 0 ? lefts.find((x) => x > gx + 4) ?? galleryOverflow : [...lefts].reverse().find((x) => x < gx - 4) ?? 0;
    scrollTo({ top: pages.offsetTop + target, behavior: reduced ? "auto" : "smooth" });
  } else {
    gallery.scrollBy({ left: dir * (items[0].offsetWidth + 20), behavior: reduced ? "auto" : "smooth" });
  }
}));

/* Hero demo: idle tabs fall asleep, the bar shrinks, then a sleeping tab is opened and wakes.
   Auto-moving content gets a pause button (WCAG 2.2.2) and stops when off screen. */
(() => {
  const win = $(".window"), tabs = $$("#tabs .tab");
  const fill = $("#fill"), note = $("#note"), addr = $("#addr"), toggle = $("#demo-toggle");
  if (!tabs.length || !fill) return;
  const sleepers = tabs.filter((t) => !t.hasAttribute("data-keep"));
  let paused = reduced, visible = true;
  const render = () => {
    const asleep = tabs.filter((t) => t.classList.contains("asleep")).length;
    // Illustrative bar lengths, labelled as such in the caption. Measurements live in docs/BENCHMARKS.md.
    fill.style.setProperty("--w", `${100 - asleep * 17}%`);
    note.textContent = asleep ? `${tabs.length - asleep} awake, ${asleep} asleep` : `${tabs.length} tabs awake`;
  };
  const activate = (tab) => {
    tabs.forEach((t) => t.classList.toggle("active", t === tab));
    tab.classList.remove("asleep");
    addr.textContent = tab === tabs[0] ? "kakangkung://newtab" : `${tab.textContent.trim().toLowerCase()}.example`;
    render();
  };
  const setPaused = (p) => {
    paused = p;
    win.classList.toggle("paused", p);
    toggle.textContent = p ? "Play demo" : "Pause demo";
    toggle.setAttribute("aria-pressed", String(p));
  };
  toggle.addEventListener("click", () => setPaused(!paused));
  new IntersectionObserver(([e]) => { visible = e.isIntersecting; }).observe(win);
  if (reduced) { sleepers.forEach((t) => t.classList.add("asleep")); render(); setPaused(true); }
  const step = async (ms) => { await wait(ms); while (paused || !visible) await wait(300); };
  (async () => {
    await step(1200);
    for (;;) {
      activate(tabs[0]);
      sleepers.forEach((t) => t.classList.remove("asleep"));
      render();
      await step(1600);
      for (const t of sleepers) { t.classList.add("asleep"); render(); await step(1200); }
      await step(1800);
      activate(sleepers[0]);
      await step(2600);
    }
  })();
})();

/* Video player with loading and error states. */
for (const p of $$("[data-player]")) {
  const v = $("video", p), scrub = $(".scrub", p), time = $(".time", p), state = $(".player-state", p);
  const btn = (a) => $(`[data-act="${a}"]`, p);
  const fmt = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
  const say = (html) => { state.innerHTML = html; };
  const sync = () => {
    const pct = v.duration ? (v.currentTime / v.duration) * 100 : 0;
    scrub.style.setProperty("--v", `${pct}%`);
    scrub.setAttribute("aria-valuenow", Math.round(pct));
    scrub.setAttribute("aria-valuetext", `${fmt(v.currentTime)} of ${fmt(v.duration || 0)}`);
    time.textContent = fmt(v.currentTime);
  };
  const start = () => { p.classList.add("started"); v.play().catch(() => {}); };
  const toggle = () => (v.paused ? v.play().catch(() => {}) : v.pause());
  const setState = () => {
    p.classList.toggle("playing", !v.paused);
    btn("play").setAttribute("aria-label", v.paused ? "Play" : "Pause");
  };
  $(".big-play", p).addEventListener("click", () => { start(); btn("play").focus({ preventScroll: true }); });
  v.addEventListener("click", () => (p.classList.contains("started") ? toggle() : start()));
  btn("play").addEventListener("click", toggle);
  btn("mute").addEventListener("click", () => {
    v.muted = !v.muted;
    p.classList.toggle("muted", v.muted);
    btn("mute").setAttribute("aria-label", v.muted ? "Unmute" : "Mute");
  });
  btn("full").addEventListener("click", () => {
    if (document.fullscreenElement) document.exitFullscreen();
    else (p.requestFullscreen || p.webkitRequestFullscreen)?.call(p);
  });
  v.addEventListener("waiting", () => say("Loading the video…"));
  v.addEventListener("playing", () => say(""));
  v.addEventListener("error", () => say('The video could not load. <a href="/assets/kakangkung-promo.mp4">Download it (6 MB)</a> instead.'), true);
  ["play", "pause", "ended"].forEach((ev) => v.addEventListener(ev, setState));
  v.addEventListener("timeupdate", sync);
  v.addEventListener("loadedmetadata", sync);

  const seekTo = (x) => {
    const r = scrub.getBoundingClientRect();
    if (v.duration) v.currentTime = clamp((x - r.left) / r.width, 0, 1) * v.duration;
    sync();
  };
  scrub.addEventListener("pointerdown", (e) => { scrub.setPointerCapture(e.pointerId); scrub.dataset.drag = "1"; seekTo(e.clientX); });
  scrub.addEventListener("pointermove", (e) => { if (scrub.dataset.drag) seekTo(e.clientX); });
  scrub.addEventListener("pointerup", () => { delete scrub.dataset.drag; });
  scrub.addEventListener("keydown", (e) => {
    const d = { ArrowRight: 5, ArrowUp: 5, ArrowLeft: -5, ArrowDown: -5 }[e.key];
    if (!v.duration) return;
    if (d) v.currentTime = clamp(v.currentTime + d, 0, v.duration);
    else if (e.key === "Home") v.currentTime = 0;
    else if (e.key === "End") v.currentTime = v.duration;
    else return;
    sync(); e.preventDefault();
  });
  setState();
}

/* Download: point at the right file for this PC, and say why. */
(async () => {
  const out = $("#detect");
  if (!out) return;
  if (!/Windows/i.test(navigator.userAgent)) {
    out.textContent = "Kakangkung runs on Windows 10 and 11. You can still download it here for a Windows PC.";
    return;
  }
  let arch = "x64";
  try {
    const h = await navigator.userAgentData?.getHighEntropyValues(["architecture"]);
    if (h?.architecture === "arm") arch = "arm64";
  } catch { /* browser does not report it; x64 is the common case */ }
  $(`.dl[data-arch="${arch}"]`)?.classList.add("rec");
  out.textContent = arch === "arm64"
    ? "This PC reports an ARM processor, so the ARM64 installer is marked."
    : "This looks like a Windows PC with an Intel or AMD processor, so the x64 installer is marked.";
})();

/* The sync diagram's packets (SMIL) run only while visible, and never with reduced motion. */
const syncSvg = $(".sync-art svg");
if (syncSvg) {
  syncSvg.pauseAnimations?.();
  if (!reduced) new IntersectionObserver(([e]) => (e.isIntersecting ? syncSvg.unpauseAnimations() : syncSvg.pauseAnimations())).observe(syncSvg);
}
