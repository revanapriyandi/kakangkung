(() => {
  if (window.top !== window || !window.ipc || !window.isSecureContext) return;
  const post = window.ipc.postMessage.bind(window.ipc);
  let attempted = '', observer = null, stopTimer = 0, controller = null;
  const stop = () => { observer?.disconnect(); clearTimeout(stopTimer); };
  addEventListener('pagehide', () => { stop(); controller?.abort(); }, { once: true });
  const probe = async () => {
    const link = document.querySelector('link[rel~="manifest"]');
    if (!link?.href || attempted === link.href) return;
    attempted = link.href;
    controller?.abort(); controller = new AbortController();
    const current = controller, signal = current.signal, timeout = setTimeout(() => current.abort(), 4_000);
    try {
      const manifestUrl = new URL(link.href, location.href);
      if (!['https:', 'http:'].includes(manifestUrl.protocol) || manifestUrl.username || manifestUrl.password) return;
      const response = await fetch(manifestUrl.href, { credentials: link.crossOrigin === 'use-credentials' ? 'include' : 'same-origin', signal });
      if (!response.ok || !response.body) return;
      const reader = response.body.getReader(), chunks = [];
      let length = 0;
      try {
        for (;;) {
          const { value, done } = await reader.read(); if (done) break;
          length += value.byteLength;
          if (length > 262_144) { await reader.cancel(); return; }
          chunks.push(value);
        }
      } finally { reader.releaseLock(); }
      const bytes = new Uint8Array(length); let offset = 0;
      for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
      const manifest = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
      const base = response.url || manifestUrl.href;
      const name = manifest.short_name || manifest.name;
      const display = manifest.display_override?.find(mode => ['standalone', 'minimal-ui', 'fullscreen'].includes(mode)) || manifest.display;
      if (typeof name !== 'string' || !name.trim() || name.length > 80 || /[\x00-\x1f\x7f]/.test(name)
        || !['standalone', 'minimal-ui', 'fullscreen'].includes(display) || typeof manifest.start_url !== 'string') return;
      const start = new URL(manifest.start_url, base), scope = new URL(manifest.scope || './', start);
      const identity = new URL(manifest.id || start.href, start);
      if ([start, scope, identity].some(url => url.origin !== location.origin || url.username || url.password)
        || !start.pathname.startsWith(scope.pathname) || !location.pathname.startsWith(scope.pathname)) return;
      const icon = manifest.icons?.find(icon => typeof icon.src === 'string' && (!icon.purpose || icon.purpose.split(/\s+/).includes('any'))
        && (icon.sizes === 'any' || icon.sizes?.split(/\s+/).some(size => /^(192|256|384|512)x\1$/.test(size))));
      if (!icon) return;
      const iconUrl = new URL(icon.src, base);
      if (!['https:', 'http:'].includes(iconUrl.protocol) || iconUrl.username || iconUrl.password) return;
      post('kk:webapp:' + JSON.stringify({ name: name.trim(), start_url: start.href, scope: scope.href,
        manifest_url: base, manifest_id: identity.href, icon_url: iconUrl.href }));
      stop();
    } catch (_) { /* An invalid or unavailable manifest is not installable. */ }
    finally { clearTimeout(timeout); }
  };
  const start = () => {
    if (typeof MutationObserver !== 'undefined' && document.head) {
      observer = new MutationObserver(probe);
      observer.observe(document.head, { childList: true, subtree: true, attributes: true, attributeFilter: ['rel', 'href'] });
      stopTimer = setTimeout(stop, 30_000);
    }
    return probe();
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true });
  else return start();
})();
