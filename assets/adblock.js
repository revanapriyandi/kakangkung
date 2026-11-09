// Called at document start and after preference changes: (script)({enabled, allowedSites}).
// YouTube player filtering and a small cosmetic base; native EasyList element hiding adds
// sheets to window.__kkCosmetic, which this helper detaches and reattaches on pause/resume.
({ enabled: on = false, allowedSites: exceptions = [] } = {}) => {
  // Runtime preference updates execute in the top-level document too, so keep both paths scoped alike.
  if (window.top !== window) return;
  const host = location.hostname.toLowerCase().replace(/\.$/, "");
  const enabled = Boolean(on) && !exceptions.some(site => site.toLowerCase().replace(/\.$/, "") === host);
  if (window.__kkAdblock) { window.__kkAdblock.setEnabled(enabled); return; }

  const youtube = ["youtube.com", "www.youtube.com", "m.youtube.com", "music.youtube.com", "tv.youtube.com",
    "youtube-nocookie.com", "www.youtube-nocookie.com", "youtubekids.com", "www.youtubekids.com"].includes(host);
  const baseCss = 'ins.adsbygoogle,[data-ad-client][data-ad-slot],iframe[id^="google_ads_iframe_"]{display:none!important}';
  const youtubeCss = 'ytd-ad-slot-renderer,ytd-display-ad-renderer,ytd-promoted-sparkles-web-renderer,' +
    'ytd-promoted-video-renderer,ytd-in-feed-ad-layout-renderer,#player-ads,.ytp-ad-overlay-container,' +
    'ytd-enforcement-message-view-model,tp-yt-paper-dialog:has(> ytd-enforcement-message-view-model){display:none!important}';
  let active = false, sheet = null, style = null, prunedPlayerFields = 0, skippedAds = 0;
  let parsed = null, fetched = null, restoreInitial = null;
  let nativeParse = null, nativeFetch = null;
  const ownValue = (object, key) => {
    if (!object || typeof object !== "object") return undefined;
    return Object.getOwnPropertyDescriptor(object, key)?.value;
  };
  // Deliberately leave streamingData, playabilityStatus, captions, tracking and unrelated objects alone.
  // Reference (mechanism only, no copied scriptlet implementation):
  // https://github.com/uBlockOrigin/uAssets/blob/master/filters/filters.txt
  // These three initial-player keys are also targeted by its YouTube filters.
  const prunePlayer = player => {
    if (!player || typeof player !== "object" || Array.isArray(player)) return;
    if (!["streamingData", "videoDetails", "playabilityStatus"].some(key => ownValue(player, key))) return;
    for (const key of ["playerAds", "adPlacements", "adSlots"]) {
      try {
        if (Object.prototype.hasOwnProperty.call(player, key) && Reflect.deleteProperty(player, key)) {
          prunedPlayerFields = Math.min(Number.MAX_SAFE_INTEGER, prunedPlayerFields + 1);
        }
      } catch (_) { /* A page may freeze its own object. */ }
    }
  };
  const prune = value => {
    if (!active) return value;
    if (Array.isArray(value)) {
      for (const item of value.slice(0, 16)) prunePlayer(ownValue(item, "playerResponse"));
    } else {
      prunePlayer(value);
      prunePlayer(ownValue(value, "playerResponse"));
    }
    return value;
  };
  const playerRequest = input => {
    try {
      const url = new URL(typeof input === "string" || input instanceof URL ? input : input.url, location.href);
      return url.origin === location.origin && /^\/youtubei\/v1\/(?:player|next|reel\/reel_watch_sequence)\/?$/.test(url.pathname);
    } catch (_) { return false; }
  };
  const wrapResponse = response => {
    if (!active || !response) return response;
    return new Proxy(response, {
      get(target, key) {
        if (key === "json") return (...args) => target.json(...args).then(prune);
        if (key === "clone") return () => wrapResponse(target.clone());
        const value = Reflect.get(target, key, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
  };
  const installYoutube = () => {
    nativeParse = JSON.parse;
    nativeFetch = window.fetch;
    parsed = new Proxy(nativeParse, {
      apply(target, self, args) { return prune(Reflect.apply(target, self, args)); },
    });
    JSON.parse = parsed;
    if (typeof nativeFetch === "function") {
      fetched = new Proxy(nativeFetch, {
        apply(target, self, args) {
          const result = Reflect.apply(target, self, args);
          return active && playerRequest(args[0]) ? result.then(wrapResponse) : result;
        },
      });
      window.fetch = fetched;
    }
    const key = "ytInitialPlayerResponse";
    const descriptor = Object.getOwnPropertyDescriptor(window, key);
    if (!descriptor || (descriptor.configurable && descriptor.writable && "value" in descriptor)) {
      let value = prune(descriptor?.value);
      const get = () => value;
      const set = next => { value = prune(next); };
      try {
        Object.defineProperty(window, key, { configurable: true, enumerable: descriptor?.enumerable ?? true, get, set });
        restoreInitial = () => {
          if (Object.getOwnPropertyDescriptor(window, key)?.get !== get) return;
          Object.defineProperty(window, key, { configurable: true, enumerable: descriptor?.enumerable ?? true,
            writable: descriptor?.writable ?? true, value });
        };
      } catch (_) { /* Leave a nonconfigurable player property to the response filters. */ }
    }
  };
  // Fallback for ads the JSON filters miss (server-inserted or changed payloads): when the
  // player marks an ad, jump to its end and press skip. Media events only, no observers or timers.
  const skipAd = event => {
    const video = event.target;
    if (!active || video?.tagName !== "VIDEO") return;
    const player = video.closest?.(".html5-video-player");
    if (!player?.classList.contains("ad-showing")) return;
    player.querySelector(".ytp-skip-ad-button,.ytp-ad-skip-button,.ytp-ad-skip-button-modern")?.click();
    if (Number.isFinite(video.duration) && video.currentTime < video.duration - 0.25) {
      video.currentTime = video.duration;
      skippedAds = Math.min(Number.MAX_SAFE_INTEGER, skippedAds + 1);
    }
  };
  const skipEvents = ["loadedmetadata", "timeupdate"];
  const cosmetic = attach => {
    const sheets = Array.isArray(window.__kkCosmetic) ? window.__kkCosmetic : [];
    const others = document.adoptedStyleSheets.filter(current => !sheets.includes(current));
    try { document.adoptedStyleSheets = attach ? [...others, ...sheets] : others; } catch (_) { /* Native sheets only. */ }
  };
  const removeStyle = () => {
    if (sheet) {
      document.adoptedStyleSheets = document.adoptedStyleSheets.filter(current => current !== sheet);
      sheet = null;
    }
    style?.remove();
    style = null;
    document.removeEventListener("DOMContentLoaded", appendStyle);
  };
  const appendStyle = () => {
    if (!active || !style || style.isConnected) return;
    (document.head || document.documentElement)?.append(style);
  };
  const installStyle = () => {
    const css = baseCss + (youtube ? youtubeCss : "");
    try {
      sheet = new CSSStyleSheet();
      sheet.replaceSync(css);
      document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
    } catch (_) {
      sheet = null;
      style = document.createElement("style");
      style.textContent = css;
      appendStyle();
      if (!style.isConnected) document.addEventListener("DOMContentLoaded", appendStyle, { once: true });
    }
  };
  const setEnabled = next => {
    if (active === Boolean(next)) return;
    active = Boolean(next);
    if (active) {
      installStyle();
      cosmetic(true);
      if (youtube) {
        installYoutube();
        for (const name of skipEvents) document.addEventListener(name, skipAd, true);
      }
    } else {
      removeStyle();
      cosmetic(false);
      for (const name of skipEvents) document.removeEventListener(name, skipAd, true);
      if (JSON.parse === parsed) JSON.parse = nativeParse;
      if (window.fetch === fetched) window.fetch = nativeFetch;
      restoreInitial?.();
      restoreInitial = null;
    }
  };
  window.__kkAdblock = { setEnabled, status: () => ({ enabled: active, youtube, prunedPlayerFields, skippedAds }) };
  setEnabled(enabled);
}
