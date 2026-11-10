// Chrome Web Store pages: the store's own "Add to Chrome" needs Chrome's private store API, so
// on an extension's page that button becomes "Add to Kakangkung" and installs through
// Kakangkung; a floating button does the same when the store's button is not found (another
// language, a changed layout). Kakangkung answers through __kkStore().
(() => {
  if (location.hostname !== "chromewebstore.google.com" || window.top !== window) return;
  const post = (m) => window.ipc && window.ipc.postMessage(m);
  const idOf = () => (location.pathname.match(/\/detail\/(?:[^/]+\/)?([a-p]{32})(?:[/?#]|$)/) || [])[1];
  // The Chrome download page, not the store's help pages (support.google.com/chrome_webstore).
  const DOWNLOAD = 'a[href^="https://www.google.com/chrome"]';
  let shown = { id: "", state: "busy", text: "Add to Kakangkung" };
  let floating = null;

  const add = (e) => {
    e.preventDefault();
    e.stopImmediatePropagation();
    if (shown.state !== "ready" || idOf() !== shown.id) return;
    // Disable synchronously; IPC acknowledgement can arrive after a second click.
    shown = { ...shown, state: "busy", text: "Checking..." };
    render();
    const h1 = document.querySelector("h1");
    post("kk:store:add\t" + shown.id + "\t" + (h1 ? h1.textContent.trim().slice(0, 120) : ""));
  };
  // Buttons naming Chrome, in any language ("Add to Chrome", "Tambahkan ke Chrome", "Chrome に追加").
  const chromeButtons = () => [...document.querySelectorAll("button")].filter((b) => !b.dataset.kk &&
    b.textContent.includes("Chrome") && b.textContent.trim().length < 48 && b.getClientRects().length > 0);
  // "Install Chrome" of a "Switch to Chrome" notice: its accessible name is the notice, not its label.
  const isSwitch = (b) => {
    const label = b.getAttribute("aria-label");
    return !!label && label.trim() !== b.textContent.trim();
  };
  const storeButton = () =>
    [...document.querySelectorAll("button")].find((b) => b.dataset.kk) || chromeButtons().find((b) => !isSwitch(b));
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
  // The store's "Switch to Chrome to install extensions" notices: wrong here, since the button
  // installs. One is found by its link to the Chrome download page, the one on an extension's
  // page by its "Install Chrome" button (any language). Hidden with the box around it: the
  // smallest that is banner-sized, widened while that adds no text (the bubble it sits in).
  const hideBox = (from) => {
    let box = from;
    while (box.parentElement && box.getBoundingClientRect().height < 40) box = box.parentElement;
    while (box.parentElement && box.parentElement !== document.body &&
      (box.parentElement.textContent.length === box.textContent.length ||
        (box.parentElement.textContent.length < 300 && !box.parentElement.querySelector("h1")))) box = box.parentElement;
    if (box !== document.body && box.textContent.length < 300 && box.style.display !== "none") box.style.display = "none";
  };
  const hideSwitchBanner = () => {
    // (A link without boxes is hidden already: climbing from it would hide what surrounds it.)
    for (const a of document.querySelectorAll(DOWNLOAD)) if (a.getClientRects().length) hideBox(a.closest("aside") || a);
    for (const b of chromeButtons()) if (isSwitch(b)) hideBox(b);
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
