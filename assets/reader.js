(() => {
  const old = document.getElementById("__kk_reader");
  if (old) { old.remove(); document.documentElement.style.overflow = ""; return; }
  const text = el => [...el.querySelectorAll(":scope > p, :scope > div > p")].reduce((n, p) => n + p.innerText.length, 0);
  let best = document.querySelector("article"), max = best ? text(best) : 0;
  if (max < 500) for (const el of document.querySelectorAll("main, article, section, div")) {
    const n = text(el);
    if (n > max) { max = n; best = el; }
  }
  if (!best) return;
  const host = document.createElement("div");
  host.id = "__kk_reader";
  host.style.cssText = "position:fixed;inset:0;z-index:2147483647;overflow:auto";
  const root = host.attachShadow({ mode: "closed" });
  const css = new CSSStyleSheet();
  css.replaceSync(`:host { all: initial; } .page { min-height: 100%; background: #faf7f2; color: #222; }
    .col { max-width: 680px; margin: 0 auto; padding: 32px 24px 80px; font: 19px/1.7 Georgia, "Times New Roman", serif; }
    h1 { font: 700 32px/1.25 "Segoe UI", system-ui, sans-serif; } h2, h3, h4 { font-family: "Segoe UI", system-ui, sans-serif; }
    img { max-width: 100%; height: auto; border-radius: 8px; } pre { white-space: pre-wrap; font-size: 15px; }
    blockquote { border-left: 3px solid #a78bfa; margin-left: 0; padding-left: 16px; color: #555; }
    button { position: fixed; top: 16px; right: 20px; font: 13px "Segoe UI", system-ui, sans-serif; border: 0;
      border-radius: 16px; padding: 7px 14px; background: #7c3aed; color: #fff; cursor: pointer; }
    @media (prefers-color-scheme: dark) { .page { background: #1d1b22; color: #e8e6ee; } blockquote { color: #aaa; } }`);
  root.adoptedStyleSheets = [css];
  const page = document.createElement("div"), col = document.createElement("div"), close = document.createElement("button");
  page.className = "page"; col.className = "col"; close.textContent = "Close reading mode";
  const h1 = document.createElement("h1");
  h1.textContent = document.title;
  col.append(h1);
  for (const n of best.querySelectorAll("h1, h2, h3, h4, p, li, blockquote, pre, img")) {
    if (n.closest("nav, aside, footer, form")) continue;
    if (n.tagName === "P" && n.parentElement.closest("li, blockquote")) continue;
    if (n.tagName === "IMG") {
      if (n.naturalWidth >= 200 && /^https?:/.test(n.currentSrc)) { const i = new Image(); i.src = n.currentSrc; i.alt = n.alt; col.append(i); }
      continue;
    }
    const t = n.innerText.trim();
    if (!t) continue;
    const e = document.createElement(n.tagName === "LI" ? "p" : n.tagName.toLowerCase());
    e.textContent = n.tagName === "LI" ? "• " + t : t;
    col.append(e);
  }
  const shut = () => { host.remove(); document.documentElement.style.overflow = ""; removeEventListener("keydown", esc, true); };
  const esc = e => { if (e.key === "Escape") shut(); };
  close.addEventListener("click", shut);
  addEventListener("keydown", esc, true);
  page.append(close, col);
  root.append(page);
  document.documentElement.append(host);
  document.documentElement.style.overflow = "hidden";
})();
