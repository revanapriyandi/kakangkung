// Chrome Web Store pages: the store's own "Add to Chrome" needs Chrome's private store API, so
// on an extension's page that button becomes "Add to Kakangkung" and installs through
// Kakangkung; a floating button does the same when the store's button is not found (another
// language, a changed layout). Kakangkung answers through __kkStore().
(() => {
  if (location.hostname !== "chromewebstore.google.com" || window.top !== window) return;
  const post = (m) => window.ipc && window.ipc.postMessage(m);
  const idOf = () => (location.pathname.match(/\/detail\/(?:[^/]+\/)?([a-p]{32})(?:[/?#]|$)/) || [])[1];
  const STORE_LABELS = ["Add to Chrome", "Remove from Chrome"];
  let shown = { id: "", state: "busy", text: "Add to Kakangkung" };
  let floating = null;

  const add = (e) => {
    e.preventDefault();
    e.stopImmediatePropagation();
    if (shown.state !== "ready") return;
    const h1 = document.querySelector("h1");
    post("kk:store:add\t" + shown.id + "\t" + (h1 ? h1.textContent.trim().slice(0, 120) : ""));
  };
  const storeButton = () =>
    [...document.querySelectorAll("button")].find((b) => b.dataset.kk || STORE_LABELS.includes(b.textContent.trim()));
  const paint = (b) => {
    if (b.textContent !== shown.text) b.textContent = shown.text;
    b.disabled = shown.state !== "ready";
    b.style.opacity = shown.state === "ready" ? "1" : "0.7";
  };
  const render = () => {
    const sb = shown.id && storeButton();
    if (sb && !sb.dataset.kk) {
      sb.dataset.kk = "1";
      sb.addEventListener("click", add, true);
    }
    if (sb) paint(sb);
    if (shown.id && !sb && !floating) {
      floating = document.createElement("button");
      floating.style.cssText = "position:fixed;bottom:24px;right:24px;z-index:2147483647;max-width:420px;padding:10px 22px;" +
        "border:0;border-radius:20px;background:#0b57d0;color:#fff;font:500 14px 'Segoe UI',system-ui,sans-serif;" +
        "box-shadow:0 1px 3px rgba(0,0,0,.3),0 4px 8px rgba(0,0,0,.15);cursor:pointer";
      floating.addEventListener("click", add, true);
      document.documentElement.appendChild(floating);
    }
    if (floating && (sb || !shown.id)) { floating.remove(); floating = null; }
    if (floating) paint(floating);
  };
  window.__kkStore = (id, state, text) => {
    if (id !== shown.id) return;
    shown = { id, state, text };
    render();
  };
  // The store's "Switch to Chrome to install extensions" banner: wrong here, since the button
  // installs. Found by its link to the Chrome download page (any language), hidden with the
  // smallest box around it that is banner-sized.
  const hideSwitchBanner = () => {
    for (const a of document.querySelectorAll('a[href*="google.com/chrome"]')) {
      let box = a;
      while (box.parentElement && box.getBoundingClientRect().height < 40) box = box.parentElement;
      if (box !== document.body && box.textContent.length < 300 && box.style.display !== "none") box.style.display = "none";
    }
  };
  // The store is a single-page app: its address and content change without a page load.
  setInterval(() => {
    hideSwitchBanner();
    const id = idOf() || "";
    if (id !== shown.id) {
      shown = { id, state: "busy", text: "Add to Kakangkung" };
      if (id) post("kk:store:check\t" + id);
    }
    render();
  }, 500);
})();
