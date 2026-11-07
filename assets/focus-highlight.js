// Accessibility: a strong outline on whatever has keyboard focus. Called as (script)(true|false);
// uses a constructed stylesheet, so it works on pages whose CSP forbids inline styles.
(on) => {
  const MARK = "__kkFocus";
  const sheets = document.adoptedStyleSheets;
  const old = sheets.find(s => s[MARK]);
  if (!on) { if (old) document.adoptedStyleSheets = sheets.filter(s => s !== old); return; }
  if (old) return;
  const css = new CSSStyleSheet();
  css[MARK] = true;
  css.replaceSync(":focus { outline: 3px solid #f59e0b !important; outline-offset: 2px !important; }");
  document.adoptedStyleSheets = [...sheets, css];
}
