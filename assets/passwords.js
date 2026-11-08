// Kakangkung's password manager, page side (top frame only). It tells the browser when a login
// field gets focus (the browser offers the saved logins under it), and when a login went
// through (the browser offers to save it). It never sees a saved password unless the person
// picks one, and the browser only hands it over for the page's own origin.
(() => {
  const ipc = window.ipc;
  if (!ipc || window.top !== window || window.__kkPwFill) return;
  const post = ipc.postMessage.bind(ipc);
  const TEXTY = ["text", "email", "tel", ""];
  const isPw = (el) => el instanceof HTMLInputElement && el.type === "password";
  const shown = (el) => el.isConnected && el.getClientRects().length > 0;
  const scope = (el) => el.form || document;
  const pwIn = (root) => [...root.querySelectorAll("input[type=password]")].filter(shown);
  const hint = (el) => `${el.name} ${el.id} ${el.autocomplete} ${el.getAttribute("aria-label") || ""} ${el.placeholder || ""}`;
  const userish = (el) =>
    el instanceof HTMLInputElement && TEXTY.includes(el.type) &&
    (el.type === "email" || /user|mail|login|account|phone|identifier|nama|surel/i.test(hint(el)));
  // The username of a password field: the last text field before it in its form.
  const userFor = (pw) => {
    let best = null;
    for (const i of scope(pw).querySelectorAll("input")) {
      if (i === pw) break;
      if (TEXTY.includes(i.type) && shown(i)) best = i;
    }
    return best;
  };
  // A username typed on an earlier step (sign-in that asks for the email first).
  let lastUser = "";
  try { lastUser = sessionStorage.getItem("__kkUser") || ""; } catch (_) {}
  const remember = (v) => { lastUser = v; try { sessionStorage.setItem("__kkUser", v); } catch (_) {} };
  const hiddenUser = () => {
    const h = document.querySelector("input[autocomplete~=username]");
    return (h && h.value) || "";
  };

  let active = null;
  document.addEventListener("focusin", (e) => {
    const t = e.target;
    const login = isPw(t) || (userish(t) && (pwIn(scope(t)).length > 0 || /username|email/.test(t.autocomplete) || t.type === "email"));
    if (!login) return;
    active = t;
    const r = t.getBoundingClientRect();
    post("kk:pw:focus:" + JSON.stringify({ x: r.left, y: r.bottom, w: r.width, dpr: devicePixelRatio }));
  }, true);
  document.addEventListener("focusout", (e) => { if (e.target === active) post("kk:pw:blur"); }, true);
  document.addEventListener("input", (e) => { if (userish(e.target)) remember(e.target.value); }, true);

  // React and friends watch the value setter, not the attribute: use the native one.
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  const put = (el, v) => {
    if (!el) return;
    el.focus();
    setter.call(el, v);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  };
  window.__kkPwFill = (user, pass) => {
    const from = active && active.isConnected ? active : pwIn(document)[0];
    if (!from) return;
    const pw = isPw(from) ? from : pwIn(scope(from))[0];
    const u = userish(from) || !pw ? from : userFor(pw);
    if (u && u !== pw && user) put(u, user);
    if (pw) put(pw, pass);
    if (user) remember(user);
  };

  // A login counts as done when its password field goes away (the next page, or the form
  // replaced) within a few seconds of sending it; a field that stays means it was refused.
  let pending = null, timer = 0;
  const send = () => {
    clearInterval(timer);
    if (!pending) return;
    post("kk:pw:save:" + JSON.stringify({ u: pending.u, p: pending.p }));
    pending = null;
  };
  const capture = (pw) => {
    if (!pw || !pw.value) return;
    const u = userFor(pw);
    pending = { u: (u && u.value) || hiddenUser() || lastUser, p: pw.value, el: pw };
    clearInterval(timer);
    let n = 0;
    timer = setInterval(() => {
      if (!pending) return clearInterval(timer);
      if (!shown(pending.el)) send();
      else if (++n > 20) { pending = null; clearInterval(timer); }
    }, 500);
  };
  document.addEventListener("submit", (e) => capture(pwIn(e.target)[0]), true);
  document.addEventListener("keydown", (e) => { if (e.key === "Enter" && isPw(e.target)) capture(e.target); }, true);
  document.addEventListener("click", (e) => {
    const b = e.target.closest && e.target.closest("button, input[type=submit], input[type=button], [role=button]");
    if (b) capture(pwIn(scope(b))[0] || pwIn(document)[0]);
  }, true);
  addEventListener("pagehide", send);
})();
